using Gpui;
using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer.NativeBridge;
using static Gpui.Units;

namespace StructuredLogViewer.Gpui.Views;

/// <summary>
/// The build tree as a virtual list over <see cref="TreeModel"/>: GPUI
/// owns scrolling and measurements, managed code renders visible rows in
/// batches. Expand/collapse splice the native list so nothing off-screen
/// is re-measured. Arrow keys walk and fold the tree like NSOutlineView.
/// </summary>
[GpuiView]
internal sealed partial class BuildTreeView : View<ViewerProps>
{
    private const float RowHeight = 22;
    private const float IndentPerLevel = 16;

    private ListController list;
    private TreeModel? model;
    private int selected = -1;
    private ulong revision;

    private TreeModel Model => model ??= new TreeModel(Props.Bridge);

    protected override void OnMounted(ref ViewContext context)
    {
        Props.Session.RevealRequested += OnRevealRequested;
        if (LaunchOptions.InitialReveal is { Length: > 0 } reveal)
        {
            LaunchOptions.InitialReveal = null;
            context.Dispatcher.Post(() => OnRevealRequested(reveal));
        }
    }

    protected override void OnUnmounted()
    {
        Props.Session.RevealRequested -= OnRevealRequested;
    }

    // ----- interaction -----

    private void ToggleRow(ClickEvent e)
    {
        int index = Model.IndexOf(e.Payload);
        if (index >= 0)
        {
            Toggle(index);
        }
    }

    private void SelectRow(ClickEvent e)
    {
        int index = Model.IndexOf(e.Payload);
        if (index >= 0)
        {
            Select(index);
        }
    }

    private void Toggle(int index)
    {
        var splice = Model.Toggle(index);
        if (splice is var (start, removed, inserted))
        {
            revision++;
            // A collapse can swallow the selection; park it on the folded row.
            if (removed > 0 && selected > index && selected < index + 1 + removed)
            {
                selected = index;
                Props.Session.Select(Model[index].Node);
            }

            list.Splice(start, removed, inserted);
            list.Refresh(index, 1);
        }
    }

    private void Select(int index)
    {
        int previous = selected;
        selected = index;
        revision++;
        if (previous >= 0 && previous < Model.Count && previous != index)
        {
            list.RefreshRanges((previous, 1), (index, 1));
        }
        else
        {
            list.Refresh(index, 1);
        }

        Props.Session.Select(Model[index].Node);
    }

    private void OnKeyDown(KeyEvent key)
    {
        if (Model.Count == 0 || key.Modifiers != 0)
        {
            return;
        }

        int current = Math.Clamp(selected, 0, Model.Count - 1);
        switch (key.Key.ToLowerInvariant())
        {
            case "down":
                MoveSelection(Math.Min(current + 1, Model.Count - 1));
                break;
            case "up":
                MoveSelection(Math.Max(current - 1, 0));
                break;
            case "right":
                if (selected < 0)
                {
                    MoveSelection(0);
                }
                else if (Model[current].HasChildren && !Model[current].Expanded)
                {
                    Toggle(current);
                }
                else if (Model[current].Expanded && current + 1 < Model.Count)
                {
                    MoveSelection(current + 1);
                }

                break;
            case "left":
                if (selected >= 0 && Model[current].Expanded)
                {
                    Toggle(current);
                }
                else if (selected >= 0 && Model.ParentIndex(current) is >= 0 and var parent)
                {
                    MoveSelection(parent);
                }

                break;
            case "enter":
            case "space":
                if (selected >= 0)
                {
                    Toggle(current);
                }

                break;
        }
    }

    private void MoveSelection(int index)
    {
        Select(index);
        list.ScrollToItem(index);
    }

    private void OnRevealRequested(string nodeId)
    {
        BaseNode node;
        try
        {
            node = Props.Bridge.ResolveNode(nodeId);
        }
        catch (Exception)
        {
            return;
        }

        int index = Model.Reveal(node);
        if (index < 0)
        {
            return;
        }

        selected = index;
        revision++;
        // Reveal may have expanded several ancestors at once; re-seed the
        // native list rather than replaying each splice.
        list.Reset(Model.Count);
        list.ScrollToItem(index);
        Props.Session.Select(node);
    }

    // ----- rendering -----

    protected override Element Render(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var tree = ui.List(
                ref list,
                new ListDataSource(Model.Count, revision),
                Rows.TreeRow,
                new ListOptions(
                    batchSize: 96,
                    overdraw: 480,
                    estimatedItemHeight: RowHeight,
                    scrollbarGutter: true
                )
            )
            .Grow()
            .Width(Percent(100));

        return ui.VStack(tree)
            .Grow()
            .Width(Percent(100))
            .Height(Percent(100))
            .Background(theme.Colors.SurfaceBackground)
            .OnKeyDown(this, (view, key) => view.OnKeyDown(key));
    }

    [GpuiListItem]
    private Element TreeRow(int index, ref RenderContext ui)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;
        var row = Model[index];
        var summary = row.Summary(Props.Bridge);
        var style = NodeStyle.For(summary);
        bool isSelected = index == selected;

        var guides = RowChrome.Guides(ref ui, row.Depth, IndentPerLevel, RowHeight, colors.BorderVariant);
        var chevron = RowChrome.Chevron(
            ref ui, this, row.HasChildren, row.Expanded, IndentPerLevel, RowHeight, row.Id,
            static (view, e) => view.ToggleRow(e), colors);

        var glyph = ui.Text(style.Glyph)
            .Width(Px(18))
            .Shrink(0)
            .TextAlign(TextAlignment.Center)
            .TextColor(NodeStyle.StateAccent(summary) ?? style.Color)
            .FontSize(Px(theme.Typography.BodySmall));

        var text = RenderSegments(ref ui, summary, style)
            .Gap(Px(6))
            .ItemsCenter()
            .Grow()
            .MinWidth(Px(0))
            .OverflowHidden();

        // The body is a flat full-width hit area; selection and hover are
        // painted on the row so they run edge to edge like an outline view.
        var body = ui.Button("row", ui.HStack(glyph, text).Gap(Px(2)).ItemsCenter().Width(Percent(100)).MinWidth(Px(0)))
            .OnClick(this, (view, e) => view.SelectRow(e), row.Id)
            .Flat(RowChrome.RowHover(colors, isSelected))
            .Grow()
            .MinWidth(Px(0))
            .Height(Px(RowHeight))
            .PaddingRight(Px(8))
            .TextColor(colors.Text);

        return ui.HStack(guides, chevron, body)
            .ItemId(row.Id)
            .ItemsCenter()
            .Width(Percent(100))
            .Height(Px(RowHeight))
            .PaddingLeft(Px(8))
            .Background(RowChrome.RowBackground(colors, isSelected))
            .Opacity(summary.IsLowRelevance && !isSelected ? 0.55f : 1f);
    }

    private static Element<DivTag> RenderSegments(ref RenderContext ui, NodeSummaryDto summary, NodeStyle.Style style)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;
        var segments = NodeRowText.Segments(summary);
        var parts = new Element[segments.Count];
        var primaryColor = summary.Kind switch
        {
            "Error" => colors.Error,
            "Warning" => colors.Warning,
            _ => NodeStyle.StateAccent(summary) ?? colors.Text,
        };

        for (int i = 0; i < segments.Count; i++)
        {
            var segment = segments[i];
            parts[i] = segment.Style switch
            {
                SegmentStyle.KindLabel => ui.Text(segment.Text).TextColor(style.Color).LineClamp(1).Shrink(0),
                SegmentStyle.Primary => ui.Text(segment.Text)
                    .TextColor(primaryColor)
                    .LineClamp(1)
                    .Shrink(1)
                    .MinWidth(Px(0)),
                SegmentStyle.Secondary => ui.Text(segment.Text).TextColor(colors.TextMuted).LineClamp(1).Shrink(0),
                SegmentStyle.Chip => ui.Badge(ui.Text(segment.Text).FontSize(Px(theme.Typography.Caption)).TextColor(colors.Warning))
                    .Background(colors.WarningBackground)
                    .PaddingX(Px(6))
                    .PaddingY(Px(1))
                    .Radius(Px(4))
                    .LineClamp(1)
                    .Shrink(0),
                SegmentStyle.Badge => ui.Badge(ui.Text(segment.Text).FontSize(Px(theme.Typography.Caption)).TextColor(colors.Info))
                    .Background(colors.InfoBackground)
                    .PaddingX(Px(6))
                    .PaddingY(Px(1))
                    .Radius(Px(4))
                    .LineClamp(1)
                    .Shrink(0),
                SegmentStyle.Targets => ui.Text(segment.Text).TextColor(Colors.Hex("#BC8CFF")).LineClamp(1).Shrink(0),
                SegmentStyle.Duration => ui.Text(segment.Text)
                    .FontSize(Px(theme.Typography.Detail))
                    .TextColor(colors.TextMuted)
                    .LineClamp(1)
                    .Shrink(0),
                _ => ui.Text(segment.Text),
            };
        }

        return ui.HStack(parts);
    }
}
