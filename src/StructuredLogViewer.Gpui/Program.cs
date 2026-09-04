using System.Diagnostics;
using Gpui;
using StructuredLogViewer.Gpui;
using StructuredLogViewer.Gpui.Views;

// Usage: StructuredLogViewer.Gpui [path.binlog] [--light] [--search <query>] [--reveal <nodeId>]
//   --search runs a query once the build opens (debug aid, like the Mac app's -reveal).
var light = args.Contains("--light", StringComparer.Ordinal);
int searchIndex = Array.IndexOf(args, "--search");
if (searchIndex >= 0 && searchIndex + 1 < args.Length)
{
    LaunchOptions.InitialSearch = args[searchIndex + 1];
}

int revealIndex = Array.IndexOf(args, "--reveal");
if (revealIndex >= 0 && revealIndex + 1 < args.Length)
{
    LaunchOptions.InitialReveal = args[revealIndex + 1];
}

var path = args
    .Where((a, i) => !a.StartsWith("--", StringComparison.Ordinal) && i != searchIndex + 1 && i != revealIndex + 1)
    .FirstOrDefault();

var application = new GpuiApplication();
application.SetTheme(ViewerTheme.Create(light ? GpuiThemeAppearance.Light : GpuiThemeAppearance.Dark));

var session = new ViewerSession();
var shell = new ShellView { Application = application, Session = session };

application.SetMenuBar(
    new GpuiMenu(
        "Structured Log Viewer",
        GpuiMenuItem.Command("About", () => shell.ShowAbout()),
        GpuiMenuItem.Separator(),
        GpuiMenuItem.Command("Quit", () => Environment.Exit(0))
    ),
    new GpuiMenu(
        "File",
        GpuiMenuItem.Command("Open…", () => shell.OpenFileDialog()),
        GpuiMenuItem.Command("Close Build", () => shell.CloseBuild())
    ),
    new GpuiMenu(
        "View",
        GpuiMenuItem.Command("Toggle Light/Dark", () => shell.ToggleTheme())
    )
);

var window = application.OpenWindow(
    shell,
    new GpuiWindowOptions
    {
        Title = "Structured Log Viewer (GPUI)",
        Width = 1400,
        Height = 900,
        TitleBarStyle = WindowTitleBarStyle.System,
    }
);
shell.Window = window;

if (path is not null)
{
    session.Load(Path.GetFullPath(path));
}

application.Run();

/// <summary>Debug-only launch arguments.</summary>
internal static class LaunchOptions
{
    public static string? InitialSearch { get; set; }
    public static string? InitialReveal { get; set; }
}
