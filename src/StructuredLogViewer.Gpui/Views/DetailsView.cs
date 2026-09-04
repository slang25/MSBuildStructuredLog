using Gpui;
using StructuredLogViewer.NativeBridge;
using static Gpui.Units;

namespace StructuredLogViewer.Gpui.Views;

/// <summary>
/// Inspector for the selected node: kind, title, timing, source location,
/// the per-kind extras the bridge exposes as props, and the full text.
/// Stands in for the Mac viewer's document well until GPUI.NET grows a
/// multi-line editor in its base package.
/// </summary>
[GpuiView]
internal sealed partial class DetailsView : View<ViewerProps>
{
    protected override void OnMounted(ref ViewContext context)
    {
        Props.Session.SelectionChanged += OnSelectionChanged;
    }

    protected override void OnUnmounted()
    {
        Props.Session.SelectionChanged -= OnSelectionChanged;
    }

    private void OnSelectionChanged()
    {
        try
        {
            Invalidate();
        }
        catch (InvalidOperationException)
        {
        }
    }

    protected override Element Render(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;
        var details = Props.Session.Selected;

        if (details is null)
        {
            return ui.VStack(
                    ui.Text("Select a node in the build tree to inspect it."u8).TextColor(colors.TextPlaceholder)
                )
                .Grow()
                .Width(Percent(100))
                .Height(Percent(100))
                .ItemsCenter()
                .JustifyCenter()
                .Padding(Px(20))
                .Background(colors.PanelBackground);
        }

        var node = details.Node;
        var style = NodeStyle.For(node);

        var header = ui.HStack(
                ui.Text(style.Glyph).TextColor(style.Color).FontSize(Px(theme.Typography.Title)),
                ui.Badge(ui.Text(node.Kind).FontSize(Px(theme.Typography.Caption)).TextColor(style.Color))
                    .Background(colors.ElementBackground)
                    .PaddingX(Px(6))
                    .PaddingY(Px(2))
                    .Radius(Px(4)),
                node.State is "none" ? ui.Div() : ui.Text(node.State).FontSize(Px(theme.Typography.Caption)).TextColor(NodeStyle.StateAccent(node) ?? colors.Success)
            )
            .Gap(Px(8))
            .ItemsCenter();

        var title = ui.Text(node.Title)
            .FontSize(Px(theme.Typography.Body))
            
            .TextColor(colors.Text);

        var facts = new List<Element>();
        AddFact(ref ui, facts, "Node id", node.Id);
        AddFact(ref ui, facts, "Duration", node.DurationMs is > 0 and var ms ? ViewerSession.FormatDuration(ms) : null);
        AddFact(ref ui, facts, "Start", details.StartTime);
        AddFact(ref ui, facts, "End", details.EndTime);
        AddFact(ref ui, facts, "Children", node.HasChildren ? node.ChildCount.ToString("N0") : null);
        AddFact(ref ui, facts, "Source", details.SourceFile is { } file ? (details.SourceLine is { } line ? $"{file}:{line}" : file) : null);
        if (node.Props is { } props)
        {
            foreach (var (key, value) in props.OrderBy(static p => p.Key, StringComparer.Ordinal))
            {
                AddFact(ref ui, facts, key, value);
            }
        }

        var fullText = details.FullText ?? string.Empty;
        var body = ui.VStack(
                ui.Text("Full text"u8).TextColor(colors.TextMuted).FontSize(Px(theme.Typography.Caption)),
                ui.Text(fullText.Length > 20_000 ? fullText[..20_000] + "\n… (truncated)" : fullText)
                    
                    .FontSize(Px(theme.Typography.Detail))
                    .TextColor(colors.Text)
                    .Width(Percent(100))
            )
            .Gap(Px(6))
            .Padding(Px(10))
            .Width(Percent(100))
            .Background(colors.SurfaceBackground)
            .BorderWidth(Px(1))
            .BorderColor(colors.BorderVariant)
            .Radius(Px(6));

        var content = ui.VStack(
                header,
                title,
                ui.Divider(),
                ui.VStack(facts.ToArray()).Gap(Px(4)).Width(Percent(100)),
                ui.Divider(),
                body
            )
            .Gap(Px(10))
            .Padding(Px(12))
            .Width(Percent(100));

        return ui.Scroll("details-scroll", ScrollAxis.Vertical, new ScrollOptions(scrollbarGutter: true), content)
            .Grow()
            .Width(Percent(100))
            .Height(Percent(100))
            .Background(colors.PanelBackground);
    }

    private static void AddFact(ref RenderContext ui, List<Element> facts, string label, string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return;
        }

        var theme = ui.Theme;
        facts.Add(ui.HStack(
                ui.Text(label).Width(Px(120)).Shrink(0).TextColor(theme.Colors.TextMuted).FontSize(Px(theme.Typography.Detail)),
                ui.Text(value).TextColor(theme.Colors.Text).FontSize(Px(theme.Typography.Detail)).MinWidth(Px(0))
            )
            .Gap(Px(8))
            .ItemsStart()
            .Width(Percent(100)));
    }
}
