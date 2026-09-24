// GET /gha/{owner}/{repo}/{artifactId}
//
// Lets the viewer open a GitHub Actions artifact with one click:
//   https://<viewer>/?binlog=/gha/{owner}/{repo}/{artifactId}
//
// GitHub's artifact download API wants a token even for public repositories, so the browser can't
// call it anonymously. This asks with a server-side token and answers with a 302 to the signed,
// 10-minute, read-only blob URL GitHub hands back. The browser follows that itself (the blob serves
// Access-Control-Allow-Origin: *), so the binlog's bytes never pass through here and nothing is
// stored.
//
// Public repositories only. Their artifacts can already be downloaded by anyone signed in to GitHub,
// so this adds no exposure. The repository's visibility is checked explicitly, so a token with more
// reach than it should have still can't leak a private artifact.
//
// Configure with: npx wrangler pages secret put GITHUB_TOKEN --project-name <project>
// A fine-grained token with "Public repositories (read-only)" access and no permissions is enough.

const NAME = /^[A-Za-z0-9_.-]+$/;

export async function onRequestGet({ params, env }) {
    const parts = Array.isArray(params.path) ? params.path : [];
    if (parts.length !== 3) {
        return text(404, 'Expected /gha/{owner}/{repo}/{artifactId}.');
    }
    const [owner, repo, id] = parts;
    if (!NAME.test(owner) || !NAME.test(repo) || !/^\d+$/.test(id)) {
        return text(400, 'Malformed owner, repository or artifact id.');
    }
    if (!env.GITHUB_TOKEN) {
        return text(503, 'This viewer has no GitHub token configured, so it cannot fetch Actions artifacts yet.');
    }

    const gh = (path) => fetch(`https://api.github.com/repos/${owner}/${repo}${path}`, {
        headers: {
            Authorization: `Bearer ${env.GITHUB_TOKEN}`,
            Accept: 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28',
            'User-Agent': 'structured-log-viewer',
        },
        redirect: 'manual',
    });

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
