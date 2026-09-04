// The engine's only JavaScript: an ES-module Web Worker that boots the .NET runtime and forwards
// {id, method, args} messages to the C# `Engine.Call(method, argsJson)` dispatcher.
//   new Worker('./engine-worker.js', { type: 'module' })
// Events posted to the page: {event:'ready'}, {event:'progress', ratio}, and replies
// {id, ok:true, result} / {id, ok:false, error:{code, message}}.
import { dotnet } from './_framework/dotnet.js';

const queue = [];
let call = null;   // (method, argsJson) => string
let fs = null;     // emscripten FS (Module.FS)

self.onmessage = (e) => { call ? handle(e.data) : queue.push(e.data); };

try {
    const runtime = await dotnet.withDiagnosticTracing(false).create();
    runtime.setModuleImports('engine-worker', {
        progress: (ratio) => postMessage({ event: 'progress', ratio }),
    });
    const exports = await runtime.getAssemblyExports(runtime.getConfig().mainAssemblyName);
    fs = runtime.Module.FS;
    fs.mkdirTree ? fs.mkdirTree('/binlogs') : fs.mkdir('/binlogs');
    call = exports.StructuredLogViewer.WebEngine.Engine.Call;
    postMessage({ event: 'ready' });
    for (const m of queue.splice(0)) handle(m);
} catch (err) {
    postMessage({ event: 'error', error: { code: 'BootFailed', message: String(err?.message ?? err) } });
}

async function handle(msg) {
    const { id, method } = msg;
    let args = msg.args ?? {};
    try {
        if (method === 'open') args = await stage(args);
        const result = JSON.parse(call(method, JSON.stringify(args)));
        if (result && typeof result === 'object' && 'error' in result) {
            postMessage({ id, ok: false, error: result.error });
        } else {
            postMessage({ id, ok: true, result });
        }
    } catch (err) {
        postMessage({ id, ok: false, error: { code: err?.name ?? 'Error', message: String(err?.message ?? err) } });
    }
}

// open: {url} or {file} (a File) or {path} (already in the runtime FS). Writes the bytes into the
// emscripten in-memory filesystem and returns the args the C# side expects.
async function stage(args) {
    if (args.path && !args.url && !args.file) return { path: args.path };
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
    try { fs.unlink(path); } catch { /* not there yet */ }
    fs.writeFile(path, bytes);
    return { path };
}
