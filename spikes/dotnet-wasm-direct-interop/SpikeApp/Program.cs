// C# half of the spike.
//
//   .NET -> Rust : [DllImport("libspike")] of #[no_mangle] extern "C" fns in libspike.a
//   Rust -> .NET : [UnmanagedCallersOnly(EntryPoint = "engine_*")] methods, which the Mono
//                  wasm SDK turns into real C symbols inside dotnet.native.wasm
//
// The only JavaScript involved is wwwroot/main.js, which boots the runtime and (for display
// only) mirrors our log lines into the page. It is never in the Rust<->C# call path.

using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.JavaScript;
using System.Text;

Log("[c#] Main started; " + RuntimeInformation.FrameworkDescription + " on " + RuntimeInformation.OSDescription + ", ProcessorCount=" + Environment.ProcessorCount);

// --- 1. Plain .NET -> Rust call --------------------------------------------------------------
int added = Native.spike_add(20, 22);
Log($"[c#] spike_add(20, 22) = {added}");

// --- 2. C#-owned memory read directly by Rust (same linear memory) ---------------------------
unsafe
{
    byte[] managedBytes = Enumerable.Range(0, 256).Select(i => (byte)i).ToArray();
    fixed (byte* p = managedBytes)
    {
        uint sum = Native.spike_sum_bytes(p, (nuint)managedBytes.Length);
        Log($"[c#] spike_sum_bytes(pinned managed byte[256] @ 0x{(nint)p:x}) = {sum} (expected {managedBytes.Sum(b => (int)b)})");
    }

    byte* native = (byte*)NativeMemory.Alloc(16);
    for (int i = 0; i < 16; i++) native[i] = 1;
    uint sum2 = Native.spike_sum_bytes(native, 16);
    Log($"[c#] spike_sum_bytes(NativeMemory.Alloc(16) @ 0x{(nint)native:x}) = {sum2} (expected 16)");
    NativeMemory.Free(native);
}

// --- 3. The round trip: C# -> Rust -> C# (x3) -> Rust -> C# ---------------------------------
//
// NOTE: no "preparation" of the UnmanagedCallersOnly methods is done here on purpose. The C thunk
// that the .NET 10 wasm SDK generates for each EntryPoint (see obj/.../pinvoke-table.h) resolves the
// managed method itself on first call via mono_wasm_marshal_get_managed_wrapper(). Older SDKs
// required the managed side to take `&Engine.Version` first; this build verifies that is no longer needed.

unsafe
{
    byte[] query = Encoding.UTF8.GetBytes("Csc");
    fixed (byte* q = query)
    {
        var sw = Stopwatch.StartNew();
        byte* report = Native.spike_run(q, (nuint)query.Length);
        sw.Stop();
        string text = Marshal.PtrToStringUTF8((nint)report) ?? "<null>";
        Native.spike_free(report);
        Log($"[c#] spike_run returned in {sw.Elapsed.TotalMilliseconds:F2} ms; report (allocated by Rust @ 0x{(nint)report:x}, freed by Rust):");
        foreach (var line in text.Split('\n'))
            Log("      " + line);
    }
}

Log($"[c#] Engine callbacks observed from Rust: engine_add x{Engine.AddCalls}, engine_version x{Engine.VersionCalls}, engine_search x{Engine.SearchCalls}, engine_free x{Engine.FreeCalls}");
Log("[c#] ROUND TRIP OK" + (Engine.VersionCalls == 1 && Engine.SearchCalls == 1 && Engine.FreeCalls == 2 && Engine.AddCalls == 1 ? "" : " (unexpected call counts!)"));

// --- 4. Threading / blocking probe ----------------------------------------------------------
Log($"[c#] Thread: ManagedThreadId={Environment.CurrentManagedThreadId}, IsThreadPoolThread={Thread.CurrentThread.IsThreadPoolThread}, ProcessorCount={Environment.ProcessorCount}");
Display.Ready();
Log("[c#] Main finished; page button will run a 3s synchronous Rust spin on the main thread.");

// Keep the runtime alive so the button can call back in.
await Task.Delay(-1);

/// <summary>C# functions exported as C symbols, called directly by Rust.</summary>
public static unsafe class Engine
{
    public static int VersionCalls, SearchCalls, FreeCalls, AddCalls;

    [UnmanagedCallersOnly(EntryPoint = "engine_add")]
    public static int Add(int a, int b)
    {
        AddCalls++;
        Program.Log($"[c#]   engine_add({a}, {b}) called from Rust");
        return a + b;
    }

    [UnmanagedCallersOnly(EntryPoint = "engine_version")]
    public static byte* Version()
    {
        VersionCalls++;
        Program.Log("[c#]   engine_version() called from Rust");
        return AllocUtf8("StructuredLogger stub engine 0.1 on " + RuntimeInformation.FrameworkDescription);
    }

    [UnmanagedCallersOnly(EntryPoint = "engine_search")]
    public static byte* Search(byte* query, nuint length)
    {
        // Exceptions must NEVER escape an UnmanagedCallersOnly method: Mono's wasm interpreter
        // asserts (interp.c interp_entry) and the whole runtime dies. Catch everything here.
        try
        {
            SearchCalls++;
            string q = Encoding.UTF8.GetString(query, (int)length);
            Program.Log($"[c#]   engine_search(0x{(nint)query:x}, {length}) called from Rust; query=\"{q}\"");
            // Stub engine: pretend to search a build tree. The real engine would be StructuredLogger's
            // Build/TreeNode search; the shape of the boundary (ptr+len in, malloc'd JSON out) is the same.
            var hits = new[] { "Csc", "CscTask", "CoreCompile", "ResolveAssemblyReferences" }
                .Where(n => n.Contains(q, StringComparison.OrdinalIgnoreCase))
                .ToArray();
            var sb = new StringBuilder();
            sb.Append("{\"query\":\"").Append(q.Replace("\"", "\\\"")).Append("\",\"count\":").Append(hits.Length).Append(",\"results\":[");
            for (int i = 0; i < hits.Length; i++)
            {
                if (i > 0) sb.Append(',');
                sb.Append("{\"name\":\"").Append(hits[i]).Append("\",\"kind\":\"Task\"}");
            }
            sb.Append("]}");
            return AllocUtf8(sb.ToString());
        }
        catch (Exception ex)
        {
            Program.Log("[c#]   engine_search FAILED: " + ex);
            return AllocUtf8("{\"error\":\"" + ex.GetType().Name + "\"}");
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "engine_free")]
    public static void Free(void* p)
    {
        FreeCalls++;
        Program.Log($"[c#]   engine_free(0x{(nint)p:x}) called from Rust");
        NativeMemory.Free(p);
    }

    /// <summary>malloc's a NUL-terminated UTF-8 copy (same emscripten malloc that Rust's allocator uses).</summary>
    private static byte* AllocUtf8(string s)
    {
        int n = Encoding.UTF8.GetByteCount(s);
        byte* p = (byte*)NativeMemory.Alloc((nuint)n + 1);
        fixed (char* c = s) Encoding.UTF8.GetBytes(c, s.Length, p, n);
        p[n] = 0;
        return p;
    }
}

/// <summary>Rust functions imported into C#. Module name == NativeFileReference file name without extension.</summary>
public static unsafe partial class Native
{
    [DllImport("libspike")] public static extern int spike_add(int a, int b);
    [DllImport("libspike")] public static extern uint spike_sum_bytes(byte* p, nuint len);
    [DllImport("libspike")] public static extern byte* spike_run(byte* query, nuint len);
    [DllImport("libspike")] public static extern void spike_free(byte* p);
    [DllImport("libspike")] public static extern ulong spike_spin(ulong iters);
}

/// <summary>Display-only JS glue (not part of the interop path).</summary>
public static partial class Display
{
    [JSImport("display.log", "main.js")]
    public static partial void AppendLine(string line);

    [JSImport("display.ready", "main.js")]
    public static partial void Ready();

    /// <summary>Called from the page button: runs a long synchronous Rust call on whatever thread .NET runs on.</summary>
    [JSExport]
    public static string SpinOnMainThread(double seconds)
    {
        var sw = Stopwatch.StartNew();
        ulong x = 0;
        // Calibrate roughly: ~ 1e8 iterations/second on a laptop; loop until elapsed.
        while (sw.Elapsed.TotalSeconds < seconds)
            x ^= Native.spike_spin(50_000_000);
        var msg = $"[c#] SpinOnMainThread({seconds}s): Rust spun for {sw.Elapsed.TotalSeconds:F2}s synchronously on ManagedThreadId={Environment.CurrentManagedThreadId} (x={x:x})";
        Program.Log(msg);
        return msg;
    }
}

public static partial class Program
{
    public static void Log(string line)
    {
        Console.WriteLine(line);
        try { Display.AppendLine(line); } catch { /* before JS imports are wired, ignore */ }
    }
}
