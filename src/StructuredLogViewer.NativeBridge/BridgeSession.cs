using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer;

namespace StructuredLogViewer.NativeBridge;

/// <summary>Thrown when a node id does not resolve; mapped to status 4.</summary>
internal sealed class BadNodeIdException : Exception
{
    public BadNodeIdException(string message, Exception inner = null)
        : base(message, inner)
    {
    }
}

/// <summary>
/// Per-open-build state. Direct port of <c>BinlogMcp</c>'s
/// <c>LoadedBinlog</c>: the analyzed <see cref="Build"/>, a dense
/// <see cref="TimedNode.Index"/> map, and the lazily-built source-file /
/// preprocess / properties-search pipeline. The tree is frozen after load;
/// everything here is read-only except the lazy caches, which are guarded.
/// </summary>
public sealed class BridgeSession
{
    private TimedNode[] indexMap;
    private SourceFileResolver sourceFileResolver;
    private PreprocessedFileManager preprocessedFileManager;
    private PropertiesAndItemsSearch propertiesAndItemsSearch;
    private PropertyGraph propertyGraph;
    private readonly object pipelineSync = new();

    private int errorCount = -1;
    private int warningCount = -1;

    // SearchIndex.FindNodes and PropertiesAndItemsSearch are not
    // thread-safe; serialize all searches within a session.
    public SemaphoreSlim SearchLock { get; } = new(1, 1);

    // One-entry cache for sorted children pages: repeated paging over the
    // same expanded parent shouldn't re-sort every call.
    private readonly object sortSync = new();
    private (BaseNode Node, int SortMode, IList<BaseNode> Sorted) sortedChildrenCache;

    public string Path { get; init; }
    public Build Build { get; init; }
    public long FileSize { get; init; }

    public static BridgeSession Load(string path, Progress progress, CancellationToken cancellationToken)
    {
        if (!System.IO.File.Exists(path))
        {
            throw new System.IO.FileNotFoundException($"Binlog not found: {path}", path);
        }

        long fileSize = new System.IO.FileInfo(path).Length;

        if (progress != null)
        {
            progress.CancellationToken = cancellationToken;
        }

        cancellationToken.ThrowIfCancellationRequested();
        var build = Serialization.Read(path, progress);
        cancellationToken.ThrowIfCancellationRequested();

        BuildAnalyzer.AnalyzeBuild(build);
        cancellationToken.ThrowIfCancellationRequested();

        build.SearchIndex = new SearchIndex(build);

        // Match the viewer: register the optional search extensions so
        // queries like `$secret` and `$nuget` work out of the box.
        build.SearchExtensions.Add(new SecretsSearch(build));
        build.SearchExtensions.Add(new NuGetSearch(build));

        return new BridgeSession
        {
            Path = path,
            Build = build,
            FileSize = fileSize
        };
    }

    /// <summary>
    /// Dense <see cref="TimedNode.Index"/> → node lookup. Built once on
    /// first use; indices are assigned densely from 0 by BuildAnalyzer.
    /// </summary>
    public TimedNode[] IndexMap
    {
        get
        {
            var local = indexMap;
            if (local != null)
            {
                return local;
            }

            lock (pipelineSync)
            {
                if (indexMap != null)
                {
                    return indexMap;
                }

                var list = new List<TimedNode>();
                Build.VisitAllChildren<TimedNode>(node =>
                {
                    int i = node.Index;
                    while (list.Count <= i)
                    {
                        list.Add(null);
                    }

                    // Nodes added to the tree after BuildAnalyzer ran keep
                    // the default Index 0 and would collide with the Build
                    // root. Both visits are pre-order, so first-wins keeps
                    // the analyzer's assignment.
                    list[i] ??= node;
                });

                indexMap = list.ToArray();
                return indexMap;
            }
        }
    }

    public SourceFileResolver SourceFileResolver
    {
        get
        {
            EnsurePropertiesPipeline();
            return sourceFileResolver;
        }
    }

    public PreprocessedFileManager PreprocessedFileManager
    {
        get
        {
            EnsurePropertiesPipeline();
            return preprocessedFileManager;
        }
    }

    public PropertiesAndItemsSearch PropertiesAndItemsSearch
    {
        get
        {
            EnsurePropertiesPipeline();
            return propertiesAndItemsSearch;
        }
    }

    public int ErrorCount
    {
        get
        {
            EnsureDiagnosticCounts();
            return errorCount;
        }
    }

    public int WarningCount
    {
        get
        {
            EnsureDiagnosticCounts();
            return warningCount;
        }
    }

    public BaseNode ResolveNode(string id)
    {
        try
        {
            return BinlogMcp.NodeId.Resolve(IndexMap, id);
        }
        catch (Exception ex) when (ex is ArgumentException or KeyNotFoundException)
        {
            throw new BadNodeIdException(ex.Message, ex);
        }
    }

    public IList<BaseNode> GetSortedChildren(BaseNode node, int sortMode)
    {
        if (node is not TreeNode tree || !tree.HasChildren)
        {
            return Array.Empty<BaseNode>();
        }

        if (sortMode == 0)
        {
            return tree.Children;
        }

        lock (sortSync)
        {
            var cached = sortedChildrenCache;
            if (ReferenceEquals(cached.Node, node) && cached.SortMode == sortMode)
            {
                return cached.Sorted;
            }

            IList<BaseNode> copy;
            if (sortMode == 1)
            {
                copy = tree.Children
                    .OrderBy(static n => ProxyNode.GetNodeText(n) ?? string.Empty, StringComparer.OrdinalIgnoreCase)
                    .ToList();
            }
            else
            {
                // Longest duration first; non-timed nodes sink to the bottom
                // in their original order (OrderBy is a stable sort).
                copy = tree.Children
                    .OrderByDescending(static n => (n as TimedNode)?.Duration ?? TimeSpan.MinValue)
                    .ToList();
            }

            sortedChildrenCache = (node, sortMode, copy);
            return copy;
        }
    }

    private void EnsureDiagnosticCounts()
    {
        if (errorCount >= 0)
        {
            return;
        }

        lock (pipelineSync)
        {
            if (errorCount >= 0)
            {
                return;
            }

            int errors = 0;
            int warnings = 0;
            Build.VisitAllChildren<BaseNode>(node =>
            {
                if (node is Error)
                {
                    errors++;
                }
                else if (node is Warning)
                {
                    warnings++;
                }
            });

            warningCount = warnings;
            errorCount = errors;
        }
    }

    // Lazily constructed source-file pipeline matching the viewer's wiring:
    // SourceFileResolver → PreprocessedFileManager → PropertyGraph subscribes
    // to PropertiesAndItemsSearch.AugmentResults so that scoped searches over
    // properties also surface the cross-import property graph as a result.
    private void EnsurePropertiesPipeline()
    {
        if (propertyGraph != null)
        {
            return;
        }

        lock (pipelineSync)
        {
            if (propertyGraph != null)
            {
                return;
            }

            var resolver = new SourceFileResolver(Build.SourceFiles ?? Array.Empty<ArchiveFile>());
            var preprocessed = new PreprocessedFileManager(Build, resolver);
            Build.WaitForBackgroundTasks();
            Build.TextProvider = evaluation => preprocessed.GetPreprocessedText(evaluation);

            var search = new PropertiesAndItemsSearch();
            // Constructor subscribes to search.AugmentResults; leave
            // AppendDependencyReferences null since the bridge doesn't
            // render clickable buttons.
            var graph = new PropertyGraph(preprocessed, search);

            sourceFileResolver = resolver;
            preprocessedFileManager = preprocessed;
            propertiesAndItemsSearch = search;
            propertyGraph = graph;
        }
    }
}
