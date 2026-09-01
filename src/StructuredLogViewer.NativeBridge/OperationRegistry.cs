using System;
using System.Collections.Concurrent;
using System.Threading;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Maps caller-chosen operation ids to cancellation sources so
/// <c>mslog_cancel(opId)</c> can abort an in-flight load or search from
/// any thread. An opId of 0 means "not cancellable".
/// </summary>
internal static class OperationRegistry
{
    private static readonly ConcurrentDictionary<long, CancellationTokenSource> Operations = new();

    public readonly struct Scope : IDisposable
    {
        private readonly long opId;
        private readonly CancellationTokenSource cts;

        internal Scope(long opId, CancellationTokenSource cts)
        {
            this.opId = opId;
            this.cts = cts;
        }

        public CancellationToken Token => cts?.Token ?? CancellationToken.None;

        public void Dispose()
        {
            if (cts == null)
            {
                return;
            }

            // Only remove our own registration — a reused opId may have
            // already replaced it with a newer operation's source.
            ((System.Collections.Generic.ICollection<System.Collections.Generic.KeyValuePair<long, CancellationTokenSource>>)Operations)
                .Remove(new System.Collections.Generic.KeyValuePair<long, CancellationTokenSource>(opId, cts));
            cts.Dispose();
        }
    }

    public static Scope Begin(long opId)
    {
        if (opId == 0)
        {
            return new Scope(0, null);
        }

        var cts = new CancellationTokenSource();
        var registered = Operations.AddOrUpdate(
            opId,
            cts,
            (_, existing) =>
            {
                // Reused id: cancel the stale operation, take its place.
                existing.Cancel();
                return cts;
            });
        return new Scope(opId, registered);
    }

    public static void Cancel(long opId)
    {
        if (Operations.TryRemove(opId, out var cts))
        {
            try
            {
                cts.Cancel();
            }
            catch (ObjectDisposedException)
            {
                // Operation completed concurrently; nothing to cancel.
            }
        }
    }
}
