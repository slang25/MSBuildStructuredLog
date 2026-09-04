using System.Diagnostics;
using Gpui;
using static Gpui.Units;

namespace StructuredLogViewer.Gpui.Views;

/// <summary>
/// The window root: welcome / loading / failed / loaded, and the three-pane
/// dock once a build is open. Mirrors the Mac viewer's BinlogDocumentView +
/// MainWindowView.
/// </summary>
[GpuiView]
internal sealed partial class ShellView : View
{
    private bool showAbout;
    private string? dialogError;

    internal GpuiApplication Application { get; init; } = null!;
    internal ViewerSession Session { get; init; } = null!;
    internal GpuiWindow? Window { get; set; }

    protected override void OnMounted(ref ViewContext context)
    {
        Session.Changed += OnSessionChanged;
    }

    protected override void OnUnmounted()
    {
        Session.Changed -= OnSessionChanged;
    }

    // Raised from the load thread as well as the UI thread; Invalidate is
    // the one entry point that is safe from anywhere.
    private void OnSessionChanged()
    {
        try
        {
            Invalidate();
        }
        catch (InvalidOperationException)
        {
            // Window already closed.
        }
    }

    internal void CloseBuild()
    {
        Session.Close();
    }

    internal void ShowAbout()
    {
        showAbout = true;
        Invalidate();
    }

    internal void ToggleTheme()
    {
        var next = Application.Theme.Appearance == GpuiThemeAppearance.Dark
            ? GpuiThemeAppearance.Light
            : GpuiThemeAppearance.Dark;
        Application.SetTheme(ViewerTheme.Create(next));
    }

    /// <summary>
    /// GPUI.NET has no file dialog yet; on macOS AppleScript's `choose file`
    /// is a perfectly good native one. Runs off-thread and posts back.
    /// </summary>
    internal void OpenFileDialog()
    {
        if (!OperatingSystem.IsMacOS())
        {
            dialogError = "Open… is macOS-only in this spike. Pass the .binlog path on the command line or drop it on the window.";
            Invalidate();
            return;
        }

        _ = Task.Run(() =>
        {
            try
            {
                var psi = new ProcessStartInfo("osascript")
                {
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                };
                psi.ArgumentList.Add("-e");
                psi.ArgumentList.Add("POSIX path of (choose file with prompt \"Open MSBuild binary log\" of type {\"binlog\"})");

                using var process = Process.Start(psi)!;
                string output = process.StandardOutput.ReadToEnd().Trim();
                process.WaitForExit();

                if (process.ExitCode == 0 && output.Length > 0)
                {
                    Dispatcher.Post(() => Session.Load(output));
                }
            }
            catch (Exception ex)
            {
                Dispatcher.Post(() =>
                {
                    dialogError = ex.Message;
                    Invalidate();
                });
            }
        });
    }

    private void OnFileDrop(FileDropEvent drop)
    {
        var binlog = drop.Paths.FirstOrDefault(p => p.EndsWith(".binlog", StringComparison.OrdinalIgnoreCase));
        if (binlog is not null)
        {
            Session.Load(binlog);
        }
    }

    private void OnKeyDown(KeyEvent key)
    {
        if (key.IsHeld)
        {
            return;
        }

        if (key.Matches("o", platform: true))
        {
            OpenFileDialog();
        }
        else if (key.Matches("w", platform: true) && Session.Phase == LoadPhase.Loaded)
        {
            CloseBuild();
        }
    }

    protected override Element Render(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var content = Session.Phase switch
        {
            LoadPhase.Idle => RenderWelcome(ref ui),
            LoadPhase.Loading => RenderLoading(ref ui),
            LoadPhase.Failed => RenderFailed(ref ui),
            LoadPhase.Loaded => RenderLoaded(ref ui),
            _ => ui.Text("?"u8),
        };

        var root = ui.VStack(content)
            .Grow()
            .Width(Percent(100))
            .Height(Percent(100))
            .Background(theme.Colors.Background)
            .TextColor(theme.Colors.Text)
            .FontSize(Px(theme.Typography.BodySmall))
            .OnFileDrop(this, (view, drop) => view.OnFileDrop(drop))
            .OnKeyDown(this, (view, key) => view.OnKeyDown(key));

        if (!showAbout)
        {
            return root;
        }

        var about = ui.VStack(
                ui.Text("Structured Log Viewer"u8)
                    .FontSize(Px(theme.Typography.Heading))
                    .TextColor(theme.Colors.Text),
                ui.Text("GPUI.NET spike — the Mac viewer's layout on Zed's GPUI, driven by the existing StructuredLogger engine in-process."u8)
                    .TextColor(theme.Colors.TextMuted),
                ui.Text($"GPUI.NET 0.2.0-preview.1 · .NET {Environment.Version}")
                    .FontSize(Px(theme.Typography.Detail))
                    .TextColor(theme.Colors.TextMuted),
                ui.Button("about-close", "Close")
                    .OnClick(this, (view, _) =>
                    {
                        view.showAbout = false;
                        view.Invalidate();
                    })
                    .Padding(Px(8))
                    .SelfEnd()
            )
            .Gap(Px(12))
            .Padding(Px(20))
            .Width(Px(440))
            .Background(theme.Colors.ElevatedSurfaceBackground)
            .BorderWidth(Px(1))
            .BorderColor(theme.Colors.Border)
            .Radius(Px(12));

        var dialog = ui.Dialog("about"u8, about, new OverlayOptions(backdrop: theme.Colors.Background.WithAlpha(160)))
            .OnDismiss(this, (view, _) =>
            {
                view.showAbout = false;
                view.Invalidate();
            });

        return ui.VStack(root, dialog).Grow().Width(Percent(100)).Height(Percent(100));
    }

    private Element RenderWelcome(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var card = ui.VStack(
                ui.Text("MSBuild Structured Log Viewer"u8)
                    .FontSize(Px(theme.Typography.Large))
                    .TextColor(theme.Colors.Text),
                ui.Text("Drop a .binlog on this window, press ⌘O, or pass a path on the command line."u8)
                    .TextColor(theme.Colors.TextMuted),
                ui.HStack(
                        ui.Button("welcome-open", "Open…")
                            .OnClick(this, (view, _) => view.OpenFileDialog())
                            .Padding(Px(10))
                            .Background(theme.Colors.Accent)
                            .HoverBackground(theme.Colors.AccentHover)
                            .ActiveBackground(theme.Colors.AccentActive)
                            .TextColor(theme.Colors.TextOnAccent)
                            .Radius(Px(6))
                    )
                    .Gap(Px(8)),
                dialogError is null
                    ? ui.Div()
                    : ui.Text(dialogError).TextColor(theme.Colors.Error).FontSize(Px(theme.Typography.Detail))
            )
            .Gap(Px(14))
            .Padding(Px(28))
            .Width(Px(520))
            .Background(theme.Colors.SurfaceBackground)
            .BorderWidth(Px(1))
            .BorderColor(theme.Colors.BorderVariant)
            .Radius(Px(14));

        return ui.VStack(card).Grow().Width(Percent(100)).Height(Percent(100)).ItemsCenter().JustifyCenter();
    }

    private Element RenderLoading(ref RenderContext ui)
    {
        var theme = ui.Theme;
        float percent = (float)Math.Clamp(Session.Progress * 100, 0, 100);
        var bar = ui.Div(
                ui.Div().Width(Percent(percent)).Height(Percent(100)).Background(theme.Colors.Accent).Radius(Px(3))
            )
            .Width(Px(360))
            .Height(Px(6))
            .Background(theme.Colors.ElementBackground)
            .Radius(Px(3));

        var card = ui.VStack(
                ui.Text($"Loading {Path.GetFileName(Session.Path ?? string.Empty)}…").TextColor(theme.Colors.Text),
                bar,
                ui.Text("Reading, analyzing and indexing the build log"u8)
                    .FontSize(Px(theme.Typography.Detail))
                    .TextColor(theme.Colors.TextMuted)
            )
            .Gap(Px(12))
            .ItemsCenter();

        // Progress arrives from the load thread via Invalidate; Dynamic keeps
        // the frame cadence smooth between those ticks.
        return ui.Dynamic(true, ui.VStack(card).Grow().Width(Percent(100)).Height(Percent(100)).ItemsCenter().JustifyCenter());
    }

    private Element RenderFailed(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var card = ui.VStack(
                ui.Text("Could not open build log"u8)
                    .FontSize(Px(theme.Typography.Heading))
                    .TextColor(theme.Colors.Error),
                ui.Text(Session.Error ?? "Unknown error").TextColor(theme.Colors.TextMuted),
                ui.Button("retry", "Try Again")
                    .OnClick(this, (view, _) =>
                    {
                        if (view.Session.Path is { } path)
                        {
                            view.Session.Load(path);
                        }
                    })
                    .Padding(Px(8))
            )
            .Gap(Px(12))
            .Padding(Px(24))
            .Width(Px(520))
            .Background(theme.Colors.SurfaceBackground)
            .BorderWidth(Px(1))
            .BorderColor(theme.Colors.Error)
            .Radius(Px(12));

        return ui.VStack(card).Grow().Width(Percent(100)).Height(Percent(100)).ItemsCenter().JustifyCenter();
    }

    private Element RenderLoaded(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var bridge = Session.Bridge!;
        var info = Session.Info!;
        var props = new ViewerProps(Session, bridge);
        string key = info.FilePath;

        var center = ui.DockTabs(
            panels: ui.DockPanel(
                "tree",
                "Build Log",
                ui.Child<BuildTreeView, ViewerProps>($"tree:{key}", in props),
                new DockPanelOptions(closable: false)
            )
        );

        var left = ui.DockRegion(
                DockSide.Left,
                ui.DockTabs(
                    panels: ui.DockPanel(
                        "search",
                        "Search Log",
                        ui.Child<SearchPaneView, ViewerProps>($"search:{key}", in props),
                        new DockPanelOptions(closable: false)
                    )
                )
            )
            .InitialSize(400);

        var right = ui.DockRegion(
                DockSide.Right,
                ui.DockTabs(
                    panels: ui.DockPanel(
                        "details",
                        "Details",
                        ui.Child<DetailsView, ViewerProps>($"details:{key}", in props),
                        new DockPanelOptions(closable: false)
                    )
                )
            )
            .InitialSize(420);

        var dock = ui.DockArea("viewer-dock", center, [left, right])
            .Grow()
            .Width(Percent(100))
            .Height(Percent(100));

        return ui.VStack(RenderStatusBar(ref ui, info), ui.Divider(), dock)
            .Grow()
            .Width(Percent(100))
            .Height(Percent(100));
    }

    private Element RenderStatusBar(ref RenderContext ui, BuildInfo info)
    {
        var theme = ui.Theme;
        var status = info.Succeeded ? "Build succeeded" : "Build failed";
        var statusColor = info.Succeeded ? theme.Colors.Success : theme.Colors.Error;

        return ui.HStack(
                ui.Text(status).TextColor(statusColor),
                Pill(ref ui, $"{info.ErrorCount} error{(info.ErrorCount == 1 ? "" : "s")}", info.ErrorCount > 0 ? theme.Colors.Error : theme.Colors.TextMuted, info.ErrorCount > 0 ? theme.Colors.ErrorBackground : theme.Colors.ElementBackground),
                Pill(ref ui, $"{info.WarningCount} warning{(info.WarningCount == 1 ? "" : "s")}", info.WarningCount > 0 ? theme.Colors.Warning : theme.Colors.TextMuted, info.WarningCount > 0 ? theme.Colors.WarningBackground : theme.Colors.ElementBackground),
                ui.Text(ViewerSession.FormatDuration(info.Duration.TotalMilliseconds))
                    
                    .TextColor(theme.Colors.TextMuted),
                ui.Spacer(),
                ui.Text(Path.GetFileName(info.FilePath)).TextColor(theme.Colors.Text),
                ui.Text($"{ViewerSession.FormatBytes(info.FileSize)} · {info.TimedNodeCount:N0} timed nodes · MSBuild {info.MSBuildVersion ?? "?"}")
                    .FontSize(Px(theme.Typography.Detail))
                    .TextColor(theme.Colors.TextMuted)
            )
            .Gap(Px(10))
            .ItemsCenter()
            .PaddingX(Px(12))
            .PaddingY(Px(6))
            .Width(Percent(100))
            .Background(theme.Colors.StatusBarBackground);
    }

    private static Element Pill(ref RenderContext ui, string text, Color foreground, Color background) =>
        ui.Badge(ui.Text(text).FontSize(Px(ui.Theme.Typography.Caption)).TextColor(foreground))
            .Background(background)
            .PaddingX(Px(7))
            .PaddingY(Px(2))
            .Radius(Px(9));
}
