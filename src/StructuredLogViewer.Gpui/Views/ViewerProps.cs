using StructuredLogViewer.NativeBridge;

namespace StructuredLogViewer.Gpui.Views;

/// <summary>
/// What every pane needs: the shared session and the open build. Panes are
/// keyed by the build's path, so a newly opened file gets fresh pane
/// instances rather than a props change.
/// </summary>
internal sealed record ViewerProps(ViewerSession Session, BridgeSession Bridge);
