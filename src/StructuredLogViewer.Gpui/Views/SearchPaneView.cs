using Gpui;
using StructuredLogViewer.NativeBridge;
using static Gpui.Units;

namespace StructuredLogViewer.Gpui.Views;

/// <summary>
/// Search Log pane: query field, debounced cancel-previous search through
/// the bridge's <see cref="SearchExecution"/>, and the grouped result tree
/// with highlight spans. Clicking a real result reveals it in the build tree.
/// </summary>
[GpuiView]
internal sealed partial class SearchPaneView : View<ViewerProps>
{
    private const int DebounceMilliseconds = 300;
    private const int MinimumQueryLength = 3;
    private const int MaxResults = 500;
    private const float RowHeight = 22;

    private sealed record ResultRow(ulong Id, string Path, int Depth, SearchTreeNodeDto Node, bool HasChildren);

    private InputController input;
    private ListController list;
    private string query = string.Empty;
    private SearchResponseDto? response;
    private string? error;
    private bool searching;
    private CancellationTokenSource? searchCts;
    private readonly List<ResultRow> rows = new();
    private readonly HashSet<string> collapsed = new(StringComparer.Ordinal);
    private ulong revision;
    private ulong nextRowId = 1;
    private int selected = -1;

    private static bool IsExecutable(string query)
    {
        var trimmed = query.Trim();
        return trimmed.Length >= MinimumQueryLength || trimmed.StartsWith('$');
    }

    private void OnQueryChanged(InputEvent e)
    {
        query = e.Value;
        ScheduleSearch(debounced: true);
    }

    private void OnQuerySubmitted(InputEvent e)
    {
        query = e.Value;
        ScheduleSearch(debounced: false);
    }

    private void ScheduleSearch(bool debounced)
    {
        searchCts?.Cancel();
        var trimmed = query.Trim();
        if (!IsExecutable(trimmed))
        {
            response = null;
            error = null;
            searching = false;
            RebuildRows();
            Invalidate();
            return;
        }

        var cts = searchCts = CancellationTokenSource.CreateLinkedTokenSource(Lifetime);
        var token = cts.Token;
        var bridge = Props.Bridge;
        searching = true;
        error = null;
        Invalidate();

        _ = Task.Run(async () =>
        {
            try
            {
                if (debounced)
                {
                    await Task.Delay(DebounceMilliseconds, token);
                }

                var result = SearchExecution.Search(bridge, trimmed, MaxResults, token);
                token.ThrowIfCancellationRequested();
                Post(() =>
                {
                    response = result;
                    searching = false;
                    collapsed.Clear();
                    RebuildRows();
                });
            }
            catch (OperationCanceledException)
            {
                // Superseded by a newer keystroke.
            }
            catch (Exception ex)
            {
                Post(() =>
                {
                    error = ex.Message;
                    searching = false;
                });
            }
        }, token);
    }

    private void Post(Action action)
    {
        try
        {
            Dispatcher.Post(() =>
            {
                action();
                Invalidate();
            });
        }
        catch (InvalidOperationException)
        {
            // Pane unmounted while the search was running.
        }
    }

    private void RebuildRows()
    {
        rows.Clear();
        selected = -1;
        if (response is { Roots: { } roots })
        {
            for (int i = 0; i < roots.Count; i++)
            {
                Walk(roots[i], i.ToString(), 0);
            }
        }

        revision++;
        if (list.IsBound)
        {
            list.Reset(rows.Count);
        }

        void Walk(SearchTreeNodeDto node, string path, int depth)
        {
            bool hasChildren = node.Children is { Count: > 0 };
            rows.Add(new ResultRow(nextRowId++, path, depth, node, hasChildren));
            if (!hasChildren || collapsed.Contains(path))
            {
                return;
            }

            for (int i = 0; i < node.Children!.Count; i++)
            {
                Walk(node.Children[i], $"{path}/{i}", depth + 1);
            }
        }
    }

    private int IndexOf(ulong rowId)
    {
        for (int i = 0; i < rows.Count; i++)
        {
            if (rows[i].Id == rowId)
            {
                return i;
            }
        }

        return -1;
    }

    private void ToggleRow(ClickEvent e)
    {
        int index = IndexOf(e.Payload);
        if (index < 0)
        {
            return;
        }

        var row = rows[index];
        if (!collapsed.Remove(row.Path))
        {
            collapsed.Add(row.Path);
        }

        RebuildRows();
        Invalidate();
    }

    private void ActivateRow(ClickEvent e)
    {
        int index = IndexOf(e.Payload);
        if (index < 0)
        {
            return;
        }

        var row = rows[index];
        int previous = selected;
        selected = index;
        revision++;
        if (previous >= 0 && previous < rows.Count && previous != index)
        {
            list.RefreshRanges((previous, 1), (index, 1));
        }
        else
        {
            list.Refresh(index, 1);
        }

        if (row.Node.Node is { Id: { } nodeId })
        {
            Props.Session.RequestReveal(nodeId);
        }
        else if (row.HasChildren)
        {
            ToggleRow(e);
        }
    }

    private void RunExample(string example)
    {
        query = example;
        input.SetValue(example);
        ScheduleSearch(debounced: false);
    }

    // ----- rendering -----

    protected override void OnMounted(ref ViewContext context)
    {
        // Land in the search box, as the Mac viewer does on open.
        input = context.CreateInputController("search-query");
        context.Dispatcher.Post(() =>
        {
            input.Focus();
            if (LaunchOptions.InitialSearch is { Length: > 0 } initial)
            {
                LaunchOptions.InitialSearch = null;
                RunExample(initial);
            }
        });
    }

    protected override Element Render(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;

        var field = ui.Input(ref input, new InputOptions(placeholder: "Search ($error, $task csc, …)"))
            .OnChanged(this, (view, e) => view.OnQueryChanged(e))
            .OnSubmitted(this, (view, e) => view.OnQuerySubmitted(e))
            .Width(Percent(100));

        Element body;
        if (searching && response is null)
        {
            body = Centered(ref ui, ui.Text("Searching…"u8).TextColor(colors.TextMuted));
        }
        else if (error is not null)
        {
            body = Centered(ref ui, ui.Text(error).TextColor(colors.Error));
        }
        else if (response is { } current)
        {
            var status = ui.HStack(
                    ui.Text($"{current.ResultCount:N0} result{(current.ResultCount == 1 ? "" : "s")}{(current.Overflow ? " (capped)" : "")} · {current.ElapsedMs:0} ms")
                        .FontSize(Px(theme.Typography.Caption))
                        .TextColor(colors.TextMuted),
                    ui.Spacer(),
                    searching ? ui.Text("updating…"u8).FontSize(Px(theme.Typography.Caption)).TextColor(colors.TextPlaceholder) : ui.Div()
                )
                .ItemsCenter()
                .PaddingX(Px(10))
                .PaddingY(Px(3))
                .Width(Percent(100));

            var results = ui.List(
                    ref list,
                    new ListDataSource(rows.Count, revision),
                    Rows.ResultRowElement,
                    new ListOptions(batchSize: 96, overdraw: 400, estimatedItemHeight: RowHeight, scrollbarGutter: true)
                )
                .Grow()
                .Width(Percent(100));

            body = ui.VStack(status, ui.Divider(), results).Grow().Width(Percent(100));
        }
        else
        {
            body = RenderWatermark(ref ui);
        }

        return ui.VStack(
                ui.VStack(field).Padding(Px(8)).Width(Percent(100)),
                ui.Divider(),
                body
            )
            .Grow()
            .Width(Percent(100))
            .Height(Percent(100))
            .Background(colors.PanelBackground);
    }

    private static Element Centered(ref RenderContext ui, Element child) =>
        ui.VStack(child).Grow().Width(Percent(100)).ItemsCenter().JustifyCenter();

    private Element RenderWatermark(ref RenderContext ui)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;
        (string Query, string Hint)[] examples =
        [
            ("$error", "all errors"),
            ("$warning", "all warnings"),
            ("$task Csc", "Csc task invocations"),
            ("$target Build", "Build targets"),
            ("$project", "every project"),
            ("$property TargetFramework", "a property everywhere it was set"),
            ("Copy under($target Build)", "text under a target"),
            ("$time", "sort by duration"),
        ];

        var items = new Element[examples.Length];
        for (int i = 0; i < examples.Length; i++)
        {
            var (example, hint) = examples[i];
            items[i] = ui.Button($"example-{i}", ui.HStack(
                        ui.Text(example).TextColor(colors.TextAccent).Width(Px(190)),
                        ui.Text(hint).TextColor(colors.TextMuted).FontSize(Px(theme.Typography.Detail))
                    ).Gap(Px(10)).ItemsCenter())
                .OnClick(this, (view, _) => view.RunExample(example))
                .Width(Percent(100))
                .PaddingX(Px(8))
                .PaddingY(Px(4))
                .Background(colors.GhostElementBackground)
                .HoverBackground(colors.GhostElementHover)
                .Radius(Px(4));
        }

        return ui.VStack(
                ui.Text("Search the build log"u8).TextColor(colors.Text),
                ui.Text("Type 3+ characters, or a $-prefixed selector. Results group by project and target, like the other viewers."u8)
                    .FontSize(Px(theme.Typography.Detail))
                    .TextColor(colors.TextMuted),
                ui.Divider(),
                ui.VStack(items).Gap(Px(2)).Width(Percent(100))
            )
            .Gap(Px(10))
            .Padding(Px(14))
            .Width(Percent(100));
    }

    [GpuiListItem]
    private Element ResultRowElement(int index, ref RenderContext ui)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;
        var row = rows[index];
        var node = row.Node;
        bool isSelected = index == selected;

        const float cell = 14;
        var guides = RowChrome.Guides(ref ui, row.Depth, cell, RowHeight, colors.BorderVariant);
        var chevron = RowChrome.Chevron(
            ref ui, this, row.HasChildren, !collapsed.Contains(row.Path), cell, RowHeight, row.Id,
            static (view, e) => view.ToggleRow(e), colors);

        Element glyph;
        if (node.Node is { } summary)
        {
            var style = NodeStyle.For(summary);
            glyph = ui.Text(style.Glyph).Width(Px(16)).Shrink(0).TextAlign(TextAlignment.Center).TextColor(style.Color).FontSize(Px(theme.Typography.Detail));
        }
        else
        {
            glyph = ui.Text("▪"u8).Width(Px(16)).Shrink(0).TextAlign(TextAlignment.Center).TextColor(colors.TextMuted).FontSize(Px(theme.Typography.Detail));
        }

        var text = RenderHighlights(ref ui, node)
            .ItemsCenter()
            .Grow()
            .MinWidth(Px(0))
            .OverflowHidden();

        var body = ui.Button("rrow", ui.HStack(glyph, text).Gap(Px(2)).ItemsCenter().Width(Percent(100)).MinWidth(Px(0)))
            .OnClick(this, (view, e) => view.ActivateRow(e), row.Id)
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
            .FontSize(Px(theme.Typography.Detail))
            .Background(RowChrome.RowBackground(colors, isSelected));
    }

    private static Element<DivTag> RenderHighlights(ref RenderContext ui, SearchTreeNodeDto node)
    {
        var theme = ui.Theme;
        var colors = theme.Colors;

        if (node.Highlights is { Count: > 0 } highlights)
        {
            var parts = new Element[highlights.Count];
            for (int i = 0; i < highlights.Count; i++)
            {
                var span = highlights[i];
                // Only the last span may give way; earlier spans (the
                // highlighted match) keep their width.
                var text = ui.Text(span.Text).LineClamp(1);
                if (i < highlights.Count - 1)
                {
                    text = text.Shrink(0);
                }

                parts[i] = span.Style == "time"
                    ? text.TextColor(colors.TextMuted).FontSize(Px(theme.Typography.Caption))
                    : span.IsHighlight
                        ? text.TextColor(colors.TextAccent).Background(colors.InfoBackground).Radius(Px(2))
                        : text.TextColor(colors.Text);
            }

            return ui.HStack(parts);
        }

        var fallback = node.Node?.Title ?? node.Text ?? string.Empty;
        return ui.HStack(ui.Text(fallback).TextColor(colors.Text).LineClamp(1).MinWidth(Px(0)));
    }
}
