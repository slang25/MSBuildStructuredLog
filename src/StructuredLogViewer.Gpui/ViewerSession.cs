using System.Globalization;
using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer.NativeBridge;

namespace StructuredLogViewer.Gpui;

public enum LoadPhase
{
    Idle,
    Loading,
    Loaded,
    Failed,
}

/// <summary>
/// Header facts about an open build; the in-process twin of the bridge's
/// BuildInfo JSON.
/// </summary>
public sealed record BuildInfo(
    string RootId,
    bool Succeeded,
    int ErrorCount,
    int WarningCount,
    int TimedNodeCount,
    string FilePath,
    long FileSize,
    TimeSpan Duration,
    string? MSBuildVersion
);

/// <summary>
/// One open (or opening) binlog, shared by every view in the window. Owns
/// the <see cref="BridgeSession"/> and the small amount of cross-pane state
/// (selection, reveal requests). Load runs on the thread pool and raises
/// <see cref="Changed"/> from there; views react by calling
/// <c>Invalidate()</c>, which is the one thread-safe ingress GPUI.NET
/// guarantees. Everything else is touched on the UI thread only.
/// </summary>
public sealed class ViewerSession
{
    private CancellationTokenSource? loadCts;

    public LoadPhase Phase { get; private set; } = LoadPhase.Idle;
    public string? Path { get; private set; }
    public double Progress { get; private set; }
    public string? Error { get; private set; }
    public BridgeSession? Bridge { get; private set; }
    public BuildInfo? Info { get; private set; }

    /// <summary>The node whose details the inspector shows.</summary>
    public NodeDetailsDto? Selected { get; private set; }

    /// <summary>Raised on any thread whenever observable state changed.</summary>
    public event Action? Changed;

    /// <summary>Raised on the UI thread when the selection changes.</summary>
    public event Action? SelectionChanged;

    /// <summary>Raised on the UI thread: expand to and select this node id.</summary>
    public event Action<string>? RevealRequested;

    public void Load(string path)
    {
        // Close cancels the previous load; the new source must come after it.
        Close();
        var cts = loadCts = new CancellationTokenSource();

        Path = path;
        Phase = LoadPhase.Loading;
        Progress = 0;
        Error = null;
        Changed?.Invoke();

        var token = cts.Token;
        _ = System.Threading.Tasks.Task.Run(() =>
        {
            try
            {
                var progress = new Progress();
                double last = -1;
                progress.Updated += update =>
                {
                    // The reader reports per buffer; throttle so the UI
                    // thread isn't flooded with invalidations.
                    if (update.Ratio >= 1 || update.Ratio - last >= 0.005)
                    {
                        last = update.Ratio;
                        Progress = update.Ratio;
                        Changed?.Invoke();
                    }
                };

                var bridge = BridgeSession.Load(path, progress, token);
                token.ThrowIfCancellationRequested();

                Bridge = bridge;
                Info = CreateInfo(bridge);
                Phase = LoadPhase.Loaded;
            }
            catch (OperationCanceledException)
            {
                // Superseded by a newer Load or by Close.
                return;
            }
            catch (Exception ex)
            {
                Error = ex.Message;
                Phase = LoadPhase.Failed;
            }

            Changed?.Invoke();
        }, token);
    }

    public void Close()
    {
        loadCts?.Cancel();
        Bridge = null;
        Info = null;
        Selected = null;
        Phase = LoadPhase.Idle;
        Path = null;
        Changed?.Invoke();
    }

    public void Select(BaseNode? node)
    {
        if (Bridge is null || node is null)
        {
            Selected = null;
        }
        else
        {
            Selected = NodeFormatter.CreateDetails(Bridge, node);
        }

        SelectionChanged?.Invoke();
    }

    public void RequestReveal(string nodeId) => RevealRequested?.Invoke(nodeId);

    private static BuildInfo CreateInfo(BridgeSession bridge)
    {
        var build = bridge.Build;
        return new BuildInfo(
            RootId: BinlogMcp.NodeId.Get(build),
            Succeeded: build.Succeeded,
            ErrorCount: bridge.ErrorCount,
            WarningCount: bridge.WarningCount,
            TimedNodeCount: bridge.IndexMap.Length,
            FilePath: bridge.Path,
            FileSize: bridge.FileSize,
            Duration: build.Duration,
            MSBuildVersion: build.MSBuildVersion
        );
    }

    public static string FormatDuration(double milliseconds)
    {
        if (milliseconds >= 60_000)
        {
            int minutes = (int)(milliseconds / 60_000);
            double seconds = (milliseconds - minutes * 60_000) / 1000;
            return string.Format(CultureInfo.InvariantCulture, "{0}:{1:00.000}", minutes, seconds);
        }

        if (milliseconds >= 1000)
        {
            return string.Format(CultureInfo.InvariantCulture, "{0:0.000} s", milliseconds / 1000);
        }

        return string.Format(CultureInfo.InvariantCulture, "{0:0} ms", milliseconds);
    }

    public static string FormatBytes(long bytes)
    {
        if (bytes >= 1L << 30) return $"{bytes / (double)(1L << 30):0.0} GB";
        if (bytes >= 1L << 20) return $"{bytes / (double)(1L << 20):0.0} MB";
        if (bytes >= 1L << 10) return $"{bytes / (double)(1L << 10):0.0} KB";
        return $"{bytes} B";
    }
}
