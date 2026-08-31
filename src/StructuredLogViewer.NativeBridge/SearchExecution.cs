using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Runs viewer-syntax searches and serializes the grouped result tree
/// (<see cref="ResultTree.BuildResultTree"/> output) with per-row
/// highlight spans, mirroring the viewer's Search panel.
/// </summary>
internal static class SearchExecution
{
    public const int DefaultMaxResults = 500;
    public const int MaxAllowedResults = 5000;

    public static SearchResponseDto Search(
        BridgeSession session,
        string query,
        int maxResults,
        CancellationToken cancellationToken)
    {
        int cap = Math.Clamp(maxResults <= 0 ? DefaultMaxResults : maxResults, 1, MaxAllowedResults);

        var index = session.Build.SearchIndex
            ?? throw new InvalidOperationException("Build has no SearchIndex.");

        session.SearchLock.Wait(cancellationToken);
        try
        {
            index.MaxResults = cap;
            index.MarkResultsInTree = false;

            var stopwatch = Stopwatch.StartNew();
            var results = index.FindNodes(query, cancellationToken).ToArray();
            stopwatch.Stop();
            cancellationToken.ThrowIfCancellationRequested();

            return BuildResponse(session, query, results, cap, stopwatch.Elapsed);
        }
        finally
        {
            session.SearchLock.Release();
        }
    }

    public static SearchResponseDto SearchPropertiesAndItems(
        BridgeSession session,
        string contextNodeId,
        string query,
        int maxResults,
        CancellationToken cancellationToken)
    {
        int cap = Math.Clamp(maxResults <= 0 ? DefaultMaxResults : maxResults, 1, MaxAllowedResults);

        var contextNode = session.ResolveNode(contextNodeId);
        if (contextNode is not TimedNode timedContext || contextNode is not IProjectOrEvaluation)
        {
            throw new BadNodeIdException(
                $"Node [{contextNodeId}] is a {contextNode.GetType().Name}, but a Project or ProjectEvaluation is required.");
        }

        // Touch the lazy pipeline before taking the lock: constructing it
        // waits for background analyzer tasks and can be slow.
        var search = session.PropertiesAndItemsSearch;

        session.SearchLock.Wait(cancellationToken);
        try
        {
            var stopwatch = Stopwatch.StartNew();
            var results = search.Search(
                timedContext,
                query,
                maxResults: cap,
                markResultsInTree: false,
                cancellationToken).ToArray();
            stopwatch.Stop();
            cancellationToken.ThrowIfCancellationRequested();

            return BuildResponse(session, query, results, cap, stopwatch.Elapsed);
        }
        finally
        {
            session.SearchLock.Release();
        }
    }

    public static string GetSubtreeText(BaseNode node) =>
        Microsoft.Build.Logging.StructuredLogger.StringWriter.GetString(node);

    private static SearchResponseDto BuildResponse(
        BridgeSession session,
        string query,
        IReadOnlyList<SearchResult> results,
        int cap,
        TimeSpan elapsed)
    {
        // addDuration=false suppresses ResultTree's own "N results" Note;
        // the response carries the count already.
        var tree = ResultTree.BuildResultTree((ICollection<SearchResult>)results, addDuration: false);

        var roots = new List<SearchTreeNodeDto>(tree.Children.Count);
        foreach (var child in tree.Children)
        {
            roots.Add(Serialize(session, child));
        }

        return new SearchResponseDto
        {
            Query = query,
            ResultCount = results.Count,
            Overflow = results.Count >= cap,
            ElapsedMs = elapsed.TotalMilliseconds,
            Roots = roots
        };
    }

    private static SearchTreeNodeDto Serialize(BridgeSession session, BaseNode node)
    {
        var dto = new SearchTreeNodeDto();

        if (node is ProxyNode proxy)
        {
            dto.Text = proxy.Text;
            if (proxy.Original is BaseNode original && BinlogMcp.NodeId.Get(original) != null)
            {
                dto.Node = NodeFormatter.CreateSummary(session, original);
            }

            var highlights = new List<HighlightDto>(proxy.Highlights.Count);
            foreach (var highlight in proxy.Highlights)
            {
                if (highlight is HighlightedText highlighted)
                {
                    highlights.Add(new HighlightDto
                    {
                        Text = highlighted.Text,
                        IsHighlight = highlighted.Style != "time",
                        Style = highlighted.Style
                    });
                }
                else if (highlight is string text && text.Length > 0)
                {
                    highlights.Add(new HighlightDto { Text = text });
                }
            }

            dto.Highlights = highlights;
        }
        else
        {
            // Synthetic Note/Message/Folder rows (and extension output).
            // Real nodes that happen to appear directly still get an id.
            if (BinlogMcp.NodeId.Get(node) != null)
            {
                dto.Node = NodeFormatter.CreateSummary(session, node);
            }

            dto.Text = (node.GetFullText() ?? node.Title ?? string.Empty).TrimEnd();
        }

        if (node is TreeNode treeNode && treeNode.HasChildren)
        {
            var children = new List<SearchTreeNodeDto>(treeNode.Children.Count);
            foreach (var child in treeNode.Children)
            {
                children.Add(Serialize(session, child));
            }

            dto.Children = children;
        }

        return dto;
    }
}
