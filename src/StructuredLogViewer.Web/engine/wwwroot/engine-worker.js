// The engine's only JavaScript: an ES-module Web Worker that boots the .NET runtime and forwards
// {id, method, args} messages to the C# `Engine.Call(method, argsJson)` dispatcher.
//   new Worker('./engine-worker.js', { type: 'module' })
// Events posted to the page: {event:'ready'}, {event:'progress', ratio, id}, {event:'error', error}
// on a boot failure, and replies {id, ok:true, result} / {id, ok:false, error:{code, message}}.
import { dotnet } from './_framework/dotnet.js';

const queue = [];
let call = null;   // (method, argsJson) => string
let fs = null;     // emscripten FS (Module.FS)
// The id of the `open` currently inside the engine. Progress events carry it so the page can tell
// which request they belong to — otherwise a second open reports into the first one's sink.
let activeOpenId = null;

self.onmessage = (e) => { call ? dispatch(e.data) : queue.push(e.data); };

try {
    const runtime = await dotnet.withDiagnosticTracing(false).create();
    runtime.setModuleImports('engine-worker', {
        progress: (ratio) => postMessage({ event: 'progress', ratio, id: activeOpenId }),
    });
    const exports = await runtime.getAssemblyExports(runtime.getConfig().mainAssemblyName);
    fs = runtime.Module.FS;
    fs.mkdirTree ? fs.mkdirTree('/binlogs') : fs.mkdir('/binlogs');
    call = exports.StructuredLogViewer.WebEngine.Engine.Call;
    postMessage({ event: 'ready' });
    for (const m of queue.splice(0)) dispatch(m);
} catch (err) {
    postMessage({ event: 'error', error: { code: 'BootFailed', message: String(err?.message ?? err) } });
}

// `handle` awaits while staging bytes, so two overlapping `open` messages can reach the singleton
// C# engine out of order and leave the page attached to a session it never asked for. Opens and
// closes therefore run one at a time, in arrival order; everything else is a synchronous C# call
// and goes straight through.
let lifecycle = Promise.resolve();

function dispatch(msg) {
    if (msg.method === 'open' || msg.method === 'close') {
        lifecycle = lifecycle.then(() => handle(msg), () => handle(msg));
        return lifecycle;
    }
    return handle(msg);
}

// The MEMFS file backing the currently open session, when this worker wrote it. MEMFS lives in the
// wasm heap and never shrinks on its own, so a staged binlog has to be unlinked once its session
// goes away — otherwise opening a few files in a row exhausts the heap until the tab is reloaded.
let stagedPath = null;

function unlink(path) {
    if (!path) return;
    try { fs.unlink(path); } catch { /* already gone */ }
}

async function handle(msg) {
    const { id, method } = msg;
    let args = msg.args ?? {};
    let staged = null;
    let reachedEngine = false;
    try {
        if (method === 'open') {
            staged = await stage(args);
            args = { path: staged.path };
        }
        reachedEngine = true;
        if (method === 'open') activeOpenId = id;
        let result;
        try {
            result = JSON.parse(call(method, JSON.stringify(args)));
        } finally {
            if (method === 'open') activeOpenId = null;
        }
        if (result && typeof result === 'object' && 'error' in result) {
            retire(method, staged, reachedEngine);
            postMessage({ id, ok: false, error: result.error });
            return;
        }
        if (method === 'open') {
            // C# Engine.Open closes the previous session before opening this one, so the file it
            // was reading is now dead weight — unless this open is re-reading that very file.
            const ours = staged.owned || stagedPath === staged.path;
            if (stagedPath !== staged.path) unlink(stagedPath);
            stagedPath = ours ? staged.path : null;
        } else if (method === 'close') {
            unlink(stagedPath);
            stagedPath = null;
        }
        postMessage({ id, ok: true, result });
    } catch (err) {
        retire(method, staged, reachedEngine);
        postMessage({ id, ok: false, error: { code: err?.name ?? 'Error', message: String(err?.message ?? err) } });
    }
}

// Drops what a failed request left behind. Engine.Open closes any previous session before opening,
// so once the call has been made there is no session left to protect: the file the old one was
// reading goes too. A failure before that (staging threw) leaves the old session intact.
function retire(method, staged, reachedEngine) {
    if (staged?.owned) unlink(staged.path);
    if (method === 'open' && reachedEngine) {
        if (stagedPath !== staged?.path) unlink(stagedPath);
        stagedPath = null;
    }
}

// open: {url} or {file} (a File) or {path} (already in the runtime FS). Writes the bytes into the
// emscripten in-memory filesystem and returns the path the C# side expects, plus whether this
// worker owns it (and so may unlink it later). A zip is unpacked to the binlog inside it first.
async function stage(args) {
    if (args.path && !args.url && !args.file) return { path: args.path, owned: false };
    let bytes, name;
    if (args.file) {
        bytes = new Uint8Array(await args.file.arrayBuffer());
        name = args.file.name || 'upload.binlog';
    } else if (args.url) {
        const res = await fetch(args.url);
        if (!res.ok) throw new Error(await failureMessage(args.url, res));
        bytes = new Uint8Array(await res.arrayBuffer());
        // A GitHub Actions artifact arrives via /gha/… and a redirect to blob storage, so neither URL
        // ends in the file name. The blob's Content-Disposition carries it.
        name = dispositionFileName(res.headers.get('content-disposition'))
            || decodeURIComponent(new URL(args.url, self.location.href).pathname.split('/').pop() || '');
    } else {
        throw new Error("open needs {url}, {file} or {path}");
    }
    // Zipped artifacts (upload-artifact's default, and every download from the Actions UI) open as
    // the binlog inside them.
    if (isZip(bytes)) ({ bytes, name } = await unzipBinlog(bytes, name || 'download.zip'));
    // StructuredLogger picks its reader by extension, and a URL like /gha/owner/repo/123 has none.
    if (!/\.(binlog|buildlog|xml)$/i.test(name)) name = (name || 'download') + '.binlog';
    const path = '/binlogs/' + name.replace(/[^\w.\-]+/g, '_');
    unlink(path);
    fs.writeFile(path, bytes);
    return { path, owned: true };
}

// /gha/… explains a refusal in the response body (expired artifact, private repo, …). Pass that on
// rather than leaving the page with a bare status code.
async function failureMessage(url, res) {
    let detail = '';
    try { detail = (await res.text()).trim().slice(0, 300); } catch { /* no body */ }
    return `fetch ${url}: HTTP ${res.status}` + (detail && !detail.startsWith('<') ? ` — ${detail}` : '');
}

function dispositionFileName(header) {
    if (!header) return null;
    const star = /filename\*\s*=\s*UTF-8''([^;]+)/i.exec(header);
    let name = null;
    if (star) { try { name = decodeURIComponent(star[1].trim()); } catch { /* malformed */ } }
    if (!name) {
        const plain = /filename\s*=\s*(?:"([^"]*)"|([^;]+))/i.exec(header);
        name = plain ? (plain[1] ?? plain[2]).trim() : null;
    }
    return name ? name.split(/[\\/]/).pop() : null;
}

function isZip(bytes) {
    return bytes.length >= 4 && bytes[0] === 0x50 && bytes[1] === 0x4b && bytes[2] === 0x03 && bytes[3] === 0x04;
}

// Just enough zip to get a binlog out. Sizes come from the central directory, because
// upload-artifact streams its entries with data descriptors and leaves zeros in the local headers.
// Stored or deflate only, and no zip64: a binlog near 4 GB would not fit in a tab anyway.
async function unzipBinlog(bytes, zipName) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const bad = (why) => new Error(`${zipName}: ${why}`);
    let eocd = -1;
    for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 22 - 0xffff); i--) {
        if (view.getUint32(i, true) === 0x06054b50) { eocd = i; break; }
    }
    if (eocd < 0) throw bad('not a readable zip (no end-of-central-directory record)');
    const count = view.getUint16(eocd + 10, true);
    let p = view.getUint32(eocd + 16, true);
    if (p === 0xffffffff) throw bad('zip64 archives are not supported');

    const binlogs = [];
    for (let n = 0; n < count; n++) {
        if (view.getUint32(p, true) !== 0x02014b50) throw bad('corrupt central directory');
        const nameLen = view.getUint16(p + 28, true);
        const entry = {
            name: new TextDecoder().decode(bytes.subarray(p + 46, p + 46 + nameLen)),
            method: view.getUint16(p + 10, true),
            compressedSize: view.getUint32(p + 20, true),
            size: view.getUint32(p + 24, true),
            offset: view.getUint32(p + 42, true),
        };
        if (/\.binlog$/i.test(entry.name)) binlogs.push(entry);
        p += 46 + nameLen + view.getUint16(p + 30, true) + view.getUint16(p + 32, true);
    }
    if (binlogs.length === 0) throw bad('no .binlog inside');

    // Several binlogs in one artifact: take the largest, which is usually the build rather than a
    // restore.
    const e = binlogs.reduce((a, b) => (b.size > a.size ? b : a));
    if (e.compressedSize === 0xffffffff || e.size === 0xffffffff || e.offset === 0xffffffff) {
        throw bad('zip64 archives are not supported');
    }
    if (view.getUint32(e.offset, true) !== 0x04034b50) throw bad(`corrupt local header for ${e.name}`);
    const start = e.offset + 30 + view.getUint16(e.offset + 26, true) + view.getUint16(e.offset + 28, true);
    const data = bytes.subarray(start, start + e.compressedSize);
    let out;
    if (e.method === 0) {
        out = data.slice();
    } else if (e.method === 8) {
        const inflated = new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
        out = new Uint8Array(await new Response(inflated).arrayBuffer());
    } else {
        throw bad(`${e.name} uses compression method ${e.method}; only stored and deflate are supported`);
    }
    return { bytes: out, name: e.name.split('/').pop() };
}
