using Gpui;
using static Gpui.Units;

namespace StructuredLogViewer.Gpui.Views;

/// <summary>
/// Shared look for tree-ish rows. GPUI.NET's Button ships with card chrome
/// (1px border, 6px radius, element fill) because it is the only element
/// that can carry OnClick — and OnClick is the only pointer binding allowed
/// inside virtual list rows. So every row is built from buttons, and this
/// strips them back to flat hit areas so the list reads as a tree, not a
/// stack of buttons.
/// </summary>
internal static class RowChrome
{
    private static readonly Color Transparent = Colors.Rgba(0, 0, 0, 0);

    /// <summary>A click target with no chrome of its own; the row paints selection/hover.</summary>
    public static Element<ButtonTag> Flat(this Element<ButtonTag> button, Color hover) =>
        button
            .BorderWidth(Px(0))
            .BorderColor(Transparent)
            .Radius(Px(0))
            .Padding(Px(0))
            .Background(Transparent)
            .HoverBackground(hover)
            .ActiveBackground(hover);

    /// <summary>
    /// Indent guides: one cell per ancestor level with a hairline down the
    /// middle, the way Zed's project panel and Xcode's navigator show depth.
    /// </summary>
    public static Element Guides(ref RenderContext ui, int depth, float cell, float rowHeight, Color color)
    {
        if (depth <= 0)
        {
            return ui.Div().Width(Px(0)).Height(Px(rowHeight)).Shrink(0);
        }

        var cells = new Element[depth];
        for (int i = 0; i < depth; i++)
        {
            cells[i] = ui.Div(ui.Div().Width(Px(1)).Height(Percent(100)).Background(color))
                .Width(Px(cell))
                .Height(Px(rowHeight))
                .PaddingLeft(Px((float)Math.Floor(cell / 2)))
                .Shrink(0);
        }

        return ui.HStack(cells).Shrink(0).Height(Px(rowHeight));
    }

    /// <summary>The disclosure triangle, or an equally wide gap for leaves.</summary>
    public static Element Chevron<TView>(
        ref RenderContext ui,
        TView view,
        bool hasChildren,
        bool expanded,
        float cell,
        float rowHeight,
        ulong payload,
        Action<TView, ClickEvent> onClick,
        GpuiThemeColors colors
    )
        where TView : ViewBase
    {
        if (!hasChildren)
        {
            return ui.Div().Width(Px(cell)).Height(Px(rowHeight)).Shrink(0);
        }

        return ui.Button("tw", expanded ? "▾" : "▸")
            .OnClick(view, onClick, payload)
            .Flat(Transparent)
            .HoverTextColor(colors.Text)
            .Width(Px(cell))
            .Height(Px(rowHeight))
            .Shrink(0)
            .TextAlign(TextAlignment.Center)
            .FontSize(Px(ui.Theme.Typography.Caption))
            .TextColor(colors.TextPlaceholder);
    }

    public static Color RowBackground(GpuiThemeColors colors, bool selected) =>
        selected ? colors.ElementSelected : Transparent;

    public static Color RowHover(GpuiThemeColors colors, bool selected) =>
        selected ? Transparent : colors.GhostElementHover;
}
