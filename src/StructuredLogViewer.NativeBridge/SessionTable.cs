using System;
using System.Collections.Concurrent;
using System.Runtime;
using System.Threading;

namespace StructuredLogViewer.NativeBridge;

/// <summary>Thrown for an unknown/closed handle; mapped to status 3.</summary>
internal sealed class BadHandleException : Exception
{
    public BadHandleException(long handle)
        : base($"Unknown or closed build handle: {handle}")
    {
    }
}

/// <summary>
/// Registry of open builds keyed by opaque int64 handles. Calls rent the
/// session (refcounted); close waits for in-flight calls to drain, then
/// drops the build graph and forces a compacting GC so the working set
/// tracks what's actually open.
/// </summary>
internal static class SessionTable
{
    internal sealed class Entry
    {
        public BridgeSession Session;
        public int RefCount;
        public bool Closing;
        public readonly ManualResetEventSlim Drained = new(initialState: true);
    }

    private static readonly ConcurrentDictionary<long, Entry> Sessions = new();
    private static long nextHandle;

    public readonly struct Lease : IDisposable
    {
        private readonly Entry entry;

        internal Lease(Entry entry)
        {
            this.entry = entry;
        }

        public BridgeSession Session => entry.Session;

        public void Dispose()
        {
            lock (entry)
            {
                entry.RefCount--;
                if (entry.RefCount == 0)
                {
                    entry.Drained.Set();
                }
            }
        }
    }

    public static long Add(BridgeSession session)
    {
        long handle = Interlocked.Increment(ref nextHandle);
        Sessions[handle] = new Entry { Session = session };
        return handle;
    }

    public static Lease Rent(long handle)
    {
        if (!Sessions.TryGetValue(handle, out var entry))
        {
            throw new BadHandleException(handle);
        }

        lock (entry)
        {
            if (entry.Closing)
            {
                throw new BadHandleException(handle);
            }

            entry.RefCount++;
            entry.Drained.Reset();
        }

        return new Lease(entry);
    }

    public static void Close(long handle)
    {
        if (!Sessions.TryRemove(handle, out var entry))
        {
            throw new BadHandleException(handle);
        }

        lock (entry)
        {
            entry.Closing = true;
            if (entry.RefCount == 0)
            {
                entry.Drained.Set();
            }
        }

        // Wait for in-flight calls on this session to finish before
        // releasing the graph. Callers should cancel long operations first.
        entry.Drained.Wait();
        entry.Session = null;
        ForceCollect();
    }

    private static void ForceCollect()
    {
        // Help large binlog object graphs get reclaimed promptly. Binlogs
        // allocate many large arrays/strings, so ask the next full blocking
        // GC to compact the LOH as well; this matters under Server GC.
        GCSettings.LargeObjectHeapCompactionMode = GCLargeObjectHeapCompactionMode.CompactOnce;
        GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
        GC.WaitForPendingFinalizers();
        GCSettings.LargeObjectHeapCompactionMode = GCLargeObjectHeapCompactionMode.CompactOnce;
        GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
    }
}
