using Gpui;

namespace StructuredLogViewer.Gpui;

/// <summary>
/// The window's palette. GPUI.NET's default dark theme is indigo-on-slate;
/// this leans it towards the neutral greys the other viewers use so node
/// tints (purple targets, blue tasks, mint items) carry the colour.
/// </summary>
public static class ViewerTheme
{
    public static GpuiTheme Create(GpuiThemeAppearance appearance)
    {
        if (appearance == GpuiThemeAppearance.Light)
        {
            return GpuiTheme.CreateDefault(GpuiThemeAppearance.Light, "Structured Log Light");
        }

        var colors = new GpuiThemeColors
        {
            Background = Colors.Hex("#1E1E1E"),
            SurfaceBackground = Colors.Hex("#252526"),
            ElevatedSurfaceBackground = Colors.Hex("#2D2D30"),
            PanelBackground = Colors.Hex("#1B1B1C"),
            ElementBackground = Colors.Hex("#2D2D30"),
            ElementHover = Colors.Hex("#2A2D2E"),
            ElementActive = Colors.Hex("#37373D"),
            ElementSelected = Colors.Hex("#094771"),
            GhostElementBackground = Colors.Rgba(0, 0, 0, 0),
            GhostElementHover = Colors.Hex("#2A2D2E"),
            GhostElementActive = Colors.Hex("#37373D"),
            GhostElementSelected = Colors.Hex("#094771"),
            Border = Colors.Hex("#3C3C3C"),
            BorderVariant = Colors.Hex("#2B2B2B"),
            BorderFocused = Colors.Hex("#007FD4"),
            BorderSelected = Colors.Hex("#007FD4"),
            Text = Colors.Hex("#D4D4D4"),
            TextMuted = Colors.Hex("#8B949E"),
            TextPlaceholder = Colors.Hex("#6E7681"),
            TextDisabled = Colors.Hex("#6E7681"),
            TextAccent = Colors.Hex("#79C0FF"),
            TextOnAccent = Colors.Hex("#FFFFFF"),
            Accent = Colors.Hex("#0E639C"),
            AccentHover = Colors.Hex("#1177BB"),
            AccentActive = Colors.Hex("#0B4F7A"),
            Icon = Colors.Hex("#C5C5C5"),
            IconMuted = Colors.Hex("#8B949E"),
            TitleBarBackground = Colors.Hex("#1B1B1C"),
            TitleBarInactiveBackground = Colors.Hex("#1B1B1C"),
            TitleBarText = Colors.Hex("#D4D4D4"),
            TitleBarHover = Colors.Hex("#2A2D2E"),
            ToolbarBackground = Colors.Hex("#252526"),
            TabBarBackground = Colors.Hex("#1B1B1C"),
            TabInactiveBackground = Colors.Hex("#1B1B1C"),
            TabActiveBackground = Colors.Hex("#252526"),
            StatusBarBackground = Colors.Hex("#1B1B1C"),
            ScrollbarThumbBackground = Colors.Hex("#4A4A4A"),
            ScrollbarThumbHoverBackground = Colors.Hex("#5F5F5F"),
            ScrollbarThumbActiveBackground = Colors.Hex("#7A7A7A"),
            ScrollbarThumbBorder = Colors.Hex("#3C3C3C"),
            ScrollbarTrackBorder = Colors.Hex("#2B2B2B"),
            Success = Colors.Hex("#3FB950"),
            SuccessBackground = Colors.Hex("#1B3A22"),
            Warning = Colors.Hex("#E3B341"),
            WarningBackground = Colors.Hex("#3D3117"),
            Error = Colors.Hex("#F85149"),
            ErrorBackground = Colors.Hex("#3D1D1D"),
            Info = Colors.Hex("#58A6FF"),
            InfoBackground = Colors.Hex("#1B2B3D"),
            LinkTextHover = Colors.Hex("#79C0FF"),
        };

        return new GpuiTheme("Structured Log Dark", colors, GpuiThemeAppearance.Dark);
    }
}
