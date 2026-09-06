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
// worker owns it (and so may unlink it later).
async function stage(args) {
    if (args.path && !args.url && !args.file) return { path: args.path, owned: false };
    let bytes, name;
    if (args.file) {
        bytes = new Uint8Array(await args.file.arrayBuffer());
        name = args.file.name || 'upload.binlog';
    } else if (args.url) {
        const res = await fetch(args.url);
        if (!res.ok) throw new Error(`fetch ${args.url}: HTTP ${res.status}`);
        bytes = new Uint8Array(await res.arrayBuffer());
        name = decodeURIComponent(new URL(args.url, self.location.href).pathname.split('/').pop() || 'download.binlog');
    } else {
        throw new Error("open needs {url}, {file} or {path}");
    }
    const path = '/binlogs/' + name.replace(/[^\w.\-]+/g, '_');
    unlink(path);
    fs.writeFile(path, bytes);
    return { path, owned: true };
}
