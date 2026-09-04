// The ONLY JavaScript in this spike. It boots the .NET runtime (dotnet.js does the
// WebAssembly.instantiate of dotnet.native.wasm, which now contains the Rust code too)
// and mirrors C# log lines into the page. It is NOT part of the Rust <-> C# call path.
import { dotnet } from './_framework/dotnet.js'

const out = document.getElementById('out');
const lines = [];
const display = {
    log: (line) => { lines.push(line); out.textContent = lines.join('\n'); },
    ready: () => { document.getElementById('spin').disabled = false; },
};

const { setModuleImports, getAssemblyExports, getConfig, runMainAndExit } = await dotnet
    .withDiagnosticTracing(false)
    .create();

setModuleImports('main.js', { display });

const config = getConfig();
const exports = await getAssemblyExports(config.mainAssemblyName);

// rAF counter so the blocking test is observable: if the main thread is blocked, ticks stop.
let ticks = 0;
const tickEl = document.getElementById('tick');
(function tick() { ticks++; tickEl.textContent = 'rAF ticks: ' + ticks; requestAnimationFrame(tick); })();

document.getElementById('spin').addEventListener('click', () => {
    const before = ticks;
    const t0 = performance.now();
    const msg = exports.Display.SpinOnMainThread(3);
    const dt = performance.now() - t0;
    display.log(`[js] button handler: C#/Rust call returned after ${dt.toFixed(0)} ms; rAF ticks during the call: ${ticks - before} (0 means the page was frozen)`);
    console.log(msg);
});

// Run C# Main(). Main awaits forever so the JSExport above stays callable.
lines.length = 0;
await dotnet.run();
