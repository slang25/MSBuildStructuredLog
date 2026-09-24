// GET /gha/{owner}/{repo}/{artifactId}
//
// Lets the viewer open a GitHub Actions artifact with one click:
//   https://<viewer>/?binlog=/gha/{owner}/{repo}/{artifactId}
//
// GitHub's artifact download API wants a token even for public repositories, so the browser can't
// call it anonymously. This asks as the Structured Log Viewer GitHub App
// (github.com/apps/structured-log-viewer) and answers with a 302 to the signed, 10-minute, read-only
// blob URL GitHub hands back. The browser follows that itself (the blob serves
// Access-Control-Allow-Origin: *), so the binlog's bytes never pass through here and nothing is
// stored.
//
// The app is installed once, on its owner's account. That installation's token reads the artifacts
// of any public repository, installed on or not (checked against dotnet/maui), so one installation
// serves every public repo and nobody else has to install anything.
//
// Public repositories only. Their artifacts can already be downloaded by anyone signed in to GitHub,
// so this adds no exposure. The repository's visibility is checked explicitly, because the
// installation token *can* read any private repo the app is installed on.
//
// Configure, each with `npx wrangler pages secret put <NAME> --project-name <project>`:
//   GITHUB_APP_ID               the app's numeric id
//   GITHUB_APP_INSTALLATION_ID  its installation on the owner's account
//   GITHUB_APP_PRIVATE_KEY      a private key .pem from the app's settings page, as downloaded

const NAME = /^[A-Za-z0-9_.-]+$/;

export async function onRequestGet({ params, env }) {
    const parts = Array.isArray(params.path) ? params.path : [];
    if (parts.length !== 3) {
        return text(404, 'Expected /gha/{owner}/{repo}/{artifactId}.');
    }
    const [owner, repo, id] = parts;
    // Dot segments would be resolved inside the API URL. Cloudflare's edge normalizes them before
    // they get here, and the host is fixed, but don't lean on either.
    if (!NAME.test(owner) || !NAME.test(repo) || !/^\d+$/.test(id) || /^\.\.?$/.test(owner) || /^\.\.?$/.test(repo)) {
        return text(400, 'Malformed owner, repository or artifact id.');
    }
    if (!env.GITHUB_APP_ID || !env.GITHUB_APP_INSTALLATION_ID || !env.GITHUB_APP_PRIVATE_KEY) {
        return text(503, 'This viewer has no GitHub App configured, so it cannot fetch Actions artifacts yet.');
    }

    let token;
    try {
        token = await installationToken(env);
    } catch (err) {
        return text(502, `Could not authenticate as the GitHub App: ${err.message}`);
    }
    const gh = (path) => api(`/repos/${owner}/${repo}${path}`, token);

    const repoRes = await gh('');
    if (repoRes.status === 404) {
        return text(404, `${owner}/${repo} was not found, or is private. Only public repositories are supported so far.`);
    }
    if (!repoRes.ok) return upstreamFailure('repository lookup', repoRes);
    const repoInfo = await repoRes.json();
    if (repoInfo.private || repoInfo.visibility !== 'public') {
        return text(403, `${owner}/${repo} is not public. Only public repositories are supported so far.`);
    }

    const artRes = await gh(`/actions/artifacts/${id}`);
    if (artRes.status === 404) return text(404, `No artifact ${id} in ${owner}/${repo}.`);
    if (!artRes.ok) return upstreamFailure('artifact lookup', artRes);
    const artifact = await artRes.json();
    if (artifact.expired) {
        return text(410, `Artifact "${artifact.name}" has expired. GitHub deletes artifacts after the repository's retention period.`);
    }
    // Keeps this from being a general-purpose anonymous artifact downloader running on our token.
    if (!/binlog/i.test(artifact.name)) {
        return text(403, `Artifact "${artifact.name}" doesn't look like a binlog; only artifacts with "binlog" in the name are served.`);
    }

    const dl = await gh(`/actions/artifacts/${id}/zip`);
    const location = dl.headers.get('location');
    if (dl.status !== 302 || !location) return upstreamFailure('artifact download', dl);

    return new Response(null, {
        status: 302,
        headers: {
            Location: location,
            // The signed URL expires in minutes; never let a cache hand out a dead one.
            'Cache-Control': 'no-store',
            'Referrer-Policy': 'no-referrer',
        },
    });
}

function api(path, bearer, init = {}) {
    return fetch(`https://api.github.com${path}`, {
        ...init,
        headers: {
            Authorization: `Bearer ${bearer}`,
            Accept: 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28',
            'User-Agent': 'structured-log-viewer',
        },
        redirect: 'manual',
    });
}

// --- GitHub App authentication ---------------------------------------------------------------
// App JWT (RS256, signed with the app's private key) -> installation access token, good for an hour.
// Both are cached per isolate, so a warm Function mints a token about once an hour, not per click.

let cachedToken = null;   // { token, expires }
let cachedKey = null;     // { pem, key }

async function installationToken(env) {
    if (cachedToken && cachedToken.expires - Date.now() > 5 * 60_000) return cachedToken.token;
    const res = await api(`/app/installations/${env.GITHUB_APP_INSTALLATION_ID}/access_tokens`,
        await appJwt(env), { method: 'POST' });
    if (!res.ok) throw new Error(`installation token request returned HTTP ${res.status}`);
    const { token, expires_at } = await res.json();
    cachedToken = { token, expires: Date.parse(expires_at) };
    return token;
}

async function appJwt(env) {
    const now = Math.floor(Date.now() / 1000);
    // iat a minute early and a 9-minute life: GitHub allows 10, and rejects clocks that run ahead.
    const body = `${b64url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }))}.` +
        b64url(JSON.stringify({ iat: now - 60, exp: now + 540, iss: String(env.GITHUB_APP_ID) }));
    const sig = await crypto.subtle.sign('RSASSA-PKCS1-v1_5', await signingKey(env.GITHUB_APP_PRIVATE_KEY),
        new TextEncoder().encode(body));
    return `${body}.${b64url(new Uint8Array(sig))}`;
}

async function signingKey(pem) {
    if (cachedKey?.pem === pem) return cachedKey.key;
    const der = Uint8Array.from(atob(pem.replace(/-----[^-]+-----/g, '').replace(/\s+/g, '')), (c) => c.charCodeAt(0));
    // GitHub hands out PKCS#1 ("BEGIN RSA PRIVATE KEY"); WebCrypto only imports PKCS#8. Accept both,
    // so a freshly downloaded key goes straight into the secret without an openssl step. Told apart
    // by structure, not by the header, which a one-line .dev.vars value may have lost.
    const pkcs8 = isPkcs1(der) ? pkcs1ToPkcs8(der) : der;
    const key = await crypto.subtle.importKey('pkcs8', pkcs8, { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' },
        false, ['sign']);
    cachedKey = { pem, key };
    return key;
}

// Both start SEQUENCE { INTEGER 0, ... }. Next comes the modulus (an INTEGER) in PKCS#1, and the
// AlgorithmIdentifier (a SEQUENCE) in PKCS#8.
function isPkcs1(der) {
    const body = 2 + (der[1] & 0x80 ? der[1] & 0x7f : 0);
    return der[body + 3] === 0x02;
}

// PrivateKeyInfo ::= SEQUENCE { version 0, AlgorithmIdentifier { rsaEncryption, NULL }, OCTET STRING pkcs1 }
function pkcs1ToPkcs8(pkcs1) {
    const rsaEncryption = [0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05, 0x00];
    const inner = [0x02, 0x01, 0x00, ...rsaEncryption, 0x04, ...derLength(pkcs1.length), ...pkcs1];
    return Uint8Array.from([0x30, ...derLength(inner.length), ...inner]);
}

function derLength(n) {
    if (n < 0x80) return [n];
    const bytes = [];
    for (; n > 0; n >>= 8) bytes.unshift(n & 0xff);
    return [0x80 | bytes.length, ...bytes];
}

function b64url(data) {
    const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
    let bin = '';
    for (const b of bytes) bin += String.fromCharCode(b);
    return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function text(status, message) {
    return new Response(message, {
        status,
        headers: { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'no-store' },
    });
}

async function upstreamFailure(what, res) {
    const remaining = res.headers.get('x-ratelimit-remaining');
    const detail = remaining === '0' ? ' (GitHub API rate limit reached; try again later)' : '';
    return text(502, `GitHub ${what} failed: HTTP ${res.status}${detail}.`);
}
