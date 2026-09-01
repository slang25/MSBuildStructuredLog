using System;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text.Json;
using Microsoft.Build.Logging.StructuredLogger;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// All C ABI entry points. Marshaling and the exception guard only —
/// behavior lives in <see cref="BridgeSession"/> and friends. An unhandled
/// exception escaping an <c>[UnmanagedCallersOnly]</c> method under
/// NativeAOT fail-fasts the process, so every export body is wrapped in
/// try/catch routed through <see cref="Fail"/>.
///
/// Status codes: 0 ok, 1 error (+errorJson), 2 cancelled, 3 bad handle,
/// 4 bad node id. See include/mslog.h for the authoritative contract.
/// </summary>
public static unsafe class Exports
{
    private const string Version = "0.1.0";

    private const int StatusOk = 0;
    private const int StatusError = 1;
    private const int StatusCancelled = 2;
    private const int StatusBadHandle = 3;
    private const int StatusBadNodeId = 4;

    // ----- lifecycle-free helpers -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_version")]
    public static IntPtr MslogVersion()
    {
        try
        {
            return NativeStrings.ToNative(Version);
        }
        catch
        {
            return IntPtr.Zero;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_string_free")]
    public static void MslogStringFree(IntPtr str)
    {
        NativeStrings.Free(str);
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_cancel")]
    public static void MslogCancel(long opId)
    {
        try
        {
            OperationRegistry.Cancel(opId);
        }
        catch
        {
            // Cancellation is best-effort; never propagate across the ABI.
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_search_help")]
    public static IntPtr MslogSearchHelp()
    {
        try
        {
            using var stream = typeof(Exports).Assembly.GetManifestResourceStream("SearchSyntax.md");
            if (stream == null)
            {
                return IntPtr.Zero;
            }

            using var reader = new StreamReader(stream);
            return NativeStrings.ToNative(reader.ReadToEnd());
        }
        catch
        {
            return IntPtr.Zero;
        }
    }

    // ----- build lifecycle -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_build_open")]
    public static int MslogBuildOpen(
        IntPtr path,
        long opId,
        IntPtr progressCallback,
        IntPtr progressContext,
        long* outHandle,
        IntPtr* errorJson)
    {
        Clear(errorJson);
        try
        {
            if (outHandle == null)
            {
                throw new ArgumentNullException(nameof(outHandle));
            }

            *outHandle = 0;
            string filePath = NativeStrings.FromNative(path)
                ?? throw new ArgumentNullException(nameof(path));

            using var operation = OperationRegistry.Begin(opId);

            Progress progress = null;
            if (progressCallback != IntPtr.Zero)
            {
                progress = new Progress();
                progress.Updated += update => InvokeProgress(progressCallback, progressContext, update.Ratio);
            }

            var session = BridgeSession.Load(filePath, progress, operation.Token);
            *outHandle = SessionTable.Add(session);
            return StatusOk;
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_build_close")]
    public static int MslogBuildClose(long handle, IntPtr* errorJson)
    {
        Clear(errorJson);
        try
        {
            SessionTable.Close(handle);
            return StatusOk;
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_build_info")]
    public static int MslogBuildInfo(long handle, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var session = lease.Session;
            var build = session.Build;

            var info = new BuildInfoDto
            {
                RootId = BinlogMcp.NodeId.Get(build),
                Succeeded = build.Succeeded,
                ErrorCount = session.ErrorCount,
                WarningCount = session.WarningCount,
                NodeCount = build.SearchIndex?.NodeCount ?? 0,
                HasSourceArchive = build.SourceFiles is { Count: > 0 },
                MSBuildVersion = build.MSBuildVersion,
                FilePath = session.Path,
                FileSize = session.FileSize,
                DurationMs = build.Duration.TotalMilliseconds,
                StartTime = build.StartTime.ToString("o", CultureInfo.InvariantCulture),
                EndTime = build.EndTime.ToString("o", CultureInfo.InvariantCulture)
            };

            return Ok(outJson, JsonSerializer.Serialize(info, BridgeJsonContext.Default.BuildInfoDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- nodes -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_node_get")]
    public static int MslogNodeGet(long handle, IntPtr nodeId, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var node = lease.Session.ResolveNode(NativeStrings.FromNative(nodeId));
            var details = NodeFormatter.CreateDetails(lease.Session, node);
            return Ok(outJson, JsonSerializer.Serialize(details, BridgeJsonContext.Default.NodeDetailsDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_node_children")]
    public static int MslogNodeChildren(
        long handle,
        IntPtr nodeId,
        int offset,
        int count,
        int sortMode,
        IntPtr* outJson,
        IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var session = lease.Session;
            string id = NativeStrings.FromNative(nodeId);
            var node = session.ResolveNode(id);

            int skip = Math.Max(offset, 0);
            int take = Math.Clamp(count <= 0 ? 512 : count, 1, 5000);

            var children = session.GetSortedChildren(node, sortMode);
            int total = children.Count;

            var page = new ChildrenPageDto
            {
                ParentId = id,
                Total = total,
                Offset = skip,
                SortMode = sortMode,
                Children = new System.Collections.Generic.List<NodeSummaryDto>()
            };

            int end = Math.Min(total, skip + take);
            for (int i = skip; i < end; i++)
            {
                var summary = NodeFormatter.CreateSummary(session, children[i]);
                if (sortMode == 0)
                {
                    summary.ChildIndex = i;
                }

                page.Children.Add(summary);
            }

            page.Count = page.Children.Count;
            return Ok(outJson, JsonSerializer.Serialize(page, BridgeJsonContext.Default.ChildrenPageDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_node_ancestors")]
    public static int MslogNodeAncestors(long handle, IntPtr nodeId, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var session = lease.Session;
            var node = session.ResolveNode(NativeStrings.FromNative(nodeId));

            // Chain includes the node itself; each entry (except the root)
            // carries its index within its parent so a UI can expand
            // top-down and jump straight to the right row.
            var chain = new System.Collections.Generic.List<NodeSummaryDto>();
            BaseNode current = node;
            while (current != null)
            {
                var summary = NodeFormatter.CreateSummary(session, current);
                if (current.Parent is TreeNode parent)
                {
                    int index = parent.Children.IndexOf(current);
                    if (index >= 0)
                    {
                        summary.ChildIndex = index;
                    }
                }

                chain.Add(summary);
                current = current.Parent;
            }

            chain.Reverse();
            var dto = new AncestorsDto { Chain = chain };
            return Ok(outJson, JsonSerializer.Serialize(dto, BridgeJsonContext.Default.AncestorsDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_node_subtree_text")]
    public static int MslogNodeSubtreeText(long handle, IntPtr nodeId, IntPtr* outText, IntPtr* errorJson)
    {
        Clear(outText);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var node = lease.Session.ResolveNode(NativeStrings.FromNative(nodeId));
            return Ok(outText, SearchExecution.GetSubtreeText(node) ?? string.Empty);
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_node_source")]
    public static int MslogNodeSource(long handle, IntPtr nodeId, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var session = lease.Session;
            var node = session.ResolveNode(NativeStrings.FromNative(nodeId));

            if (!NodeFormatter.TryGetSourceLocation(node, out string filePath, out int? line))
            {
                throw new InvalidOperationException("Node has no source location.");
            }

            var location = new SourceLocationDto
            {
                FilePath = filePath,
                Line = line,
                Text = session.SourceFileResolver.GetSourceFileText(filePath)?.Text
            };

            return Ok(outJson, JsonSerializer.Serialize(location, BridgeJsonContext.Default.SourceLocationDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_node_preprocess")]
    public static int MslogNodePreprocess(long handle, IntPtr nodeId, IntPtr* outText, IntPtr* errorJson)
    {
        Clear(outText);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var session = lease.Session;
            var node = session.ResolveNode(NativeStrings.FromNative(nodeId));

            if (node is not IPreprocessable preprocessable)
            {
                throw new InvalidOperationException(
                    $"Node is a {node.GetType().Name}; only projects, evaluations and imports can be preprocessed.");
            }

            var manager = session.PreprocessedFileManager;
            string text = manager.GetPreprocessedText(
                preprocessable.RootFilePath,
                PreprocessedFileManager.GetNodeEvaluationKey(node));

            if (string.IsNullOrEmpty(text))
            {
                throw new InvalidOperationException(
                    $"No preprocessed text available for '{preprocessable.RootFilePath}'.");
            }

            return Ok(outText, text);
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_target_parent")]
    public static int MslogTargetParent(long handle, IntPtr nodeId, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var session = lease.Session;
            var node = session.ResolveNode(NativeStrings.FromNative(nodeId));

            if (node is not Target target)
            {
                throw new InvalidOperationException(
                    $"Node is a {node.GetType().Name}; only targets have a parent-target link.");
            }

            // Mirrors the viewers' NodeHyperlinkControl: prefer the node
            // where a re-entrant target originally built, else find the
            // named parent target within the same project.
            BaseNode destination = target.OriginalNode;
            if (destination == null &&
                target.ParentTarget is string parentName &&
                target.Project is Project project)
            {
                destination = project.FindFirstDescendant<Target>(
                    t => t.Name == parentName && t.Project == project);
            }

            if (destination == null)
            {
                throw new InvalidOperationException(
                    $"Target '{target.ParentTarget}' was not found in this project — it may have built in another project instance.");
            }

            var summary = NodeFormatter.CreateSummary(session, destination);
            return Ok(outJson, JsonSerializer.Serialize(summary, BridgeJsonContext.Default.NodeSummaryDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- semantics -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_semantic_file")]
    public static int MslogSemanticFile(
        long handle,
        IntPtr path,
        IntPtr evaluationId,
        IntPtr* outJson,
        IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var dto = SemanticFormatter.CreateFile(
                lease.Session,
                NativeStrings.FromNative(path),
                NativeStrings.FromNative(evaluationId));

            return Ok(outJson, JsonSerializer.Serialize(dto, BridgeJsonContext.Default.SemanticFileDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_semantic_resolve")]
    public static int MslogSemanticResolve(
        long handle,
        IntPtr evaluationId,
        IntPtr kind,
        IntPtr name,
        IntPtr* outJson,
        IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var dto = SemanticFormatter.Resolve(
                lease.Session,
                NativeStrings.FromNative(evaluationId),
                NativeStrings.FromNative(kind),
                NativeStrings.FromNative(name));

            return Ok(outJson, JsonSerializer.Serialize(dto, BridgeJsonContext.Default.SemanticSymbolDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- search -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_search")]
    public static int MslogSearch(
        long handle,
        IntPtr query,
        int maxResults,
        long opId,
        IntPtr* outJson,
        IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            using var operation = OperationRegistry.Begin(opId);

            var response = SearchExecution.Search(
                lease.Session,
                NativeStrings.FromNative(query) ?? string.Empty,
                maxResults,
                operation.Token);

            return Ok(outJson, JsonSerializer.Serialize(response, BridgeJsonContext.Default.SearchResponseDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_search_properties_and_items")]
    public static int MslogSearchPropertiesAndItems(
        long handle,
        IntPtr contextNodeId,
        IntPtr query,
        int maxResults,
        long opId,
        IntPtr* outJson,
        IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            using var operation = OperationRegistry.Begin(opId);

            var response = SearchExecution.SearchPropertiesAndItems(
                lease.Session,
                NativeStrings.FromNative(contextNodeId),
                NativeStrings.FromNative(query) ?? string.Empty,
                maxResults,
                operation.Token);

            return Ok(outJson, JsonSerializer.Serialize(response, BridgeJsonContext.Default.SearchResponseDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- embedded files -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_files_list")]
    public static int MslogFilesList(long handle, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var list = FileSearch.ListFiles(lease.Session);
            return Ok(outJson, JsonSerializer.Serialize(list, BridgeJsonContext.Default.FileListDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_file_read")]
    public static int MslogFileRead(long handle, IntPtr path, IntPtr* outText, IntPtr* errorJson)
    {
        Clear(outText);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            string text = FileSearch.ReadFile(lease.Session, NativeStrings.FromNative(path));
            return Ok(outText, text);
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "mslog_files_search")]
    public static int MslogFilesSearch(
        long handle,
        IntPtr term,
        int maxResults,
        long opId,
        IntPtr* outJson,
        IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            using var operation = OperationRegistry.Begin(opId);

            var response = FileSearch.SearchFiles(
                lease.Session,
                NativeStrings.FromNative(term),
                maxResults,
                operation.Token);

            return Ok(outJson, JsonSerializer.Serialize(response, BridgeJsonContext.Default.FileSearchResponseDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- stats -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_build_stats")]
    public static int MslogBuildStats(long handle, long opId, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            using var operation = OperationRegistry.Begin(opId);

            var stats = StatsFormatter.Calculate(lease.Session, operation.Token);
            return Ok(outJson, JsonSerializer.Serialize(stats, BridgeJsonContext.Default.StatsDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- timeline -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_timeline")]
    public static int MslogTimeline(long handle, long opId, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            using var operation = OperationRegistry.Begin(opId);

            var timeline = TimelineFormatter.Build(lease.Session, operation.Token);
            return Ok(outJson, JsonSerializer.Serialize(timeline, BridgeJsonContext.Default.TimelineDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- project reference graph -----

    [UnmanagedCallersOnly(EntryPoint = "mslog_project_graph")]
    public static int MslogProjectGraph(long handle, IntPtr* outJson, IntPtr* errorJson)
    {
        Clear(outJson);
        Clear(errorJson);
        try
        {
            using var lease = SessionTable.Rent(handle);
            var graph = lease.Session.Build.ProjectReferenceGraph?.Graph;

            var dto = new ProjectGraphDto
            {
                Vertices = new System.Collections.Generic.List<ProjectGraphVertexDto>()
            };

            if (graph != null && !graph.IsEmpty)
            {
                // First pass: stable order + index lookup, then edges.
                var vertices = graph.Vertices
                    .OrderBy(v => v.Title ?? v.Value, StringComparer.OrdinalIgnoreCase)
                    .ToArray();
                var indexOf = new System.Collections.Generic.Dictionary<Microsoft.Build.Logging.StructuredLogger.Vertex, int>(vertices.Length);
                for (int i = 0; i < vertices.Length; i++)
                {
                    indexOf[vertices[i]] = i;
                }

                foreach (var vertex in vertices)
                {
                    // Titles come pre-quoted for the search DSL
                    // (project("X")); display wants them bare.
                    var vertexDto = new ProjectGraphVertexDto
                    {
                        Value = vertex.Value,
                        Title = (vertex.Title ?? vertex.Value).Trim('"'),
                        Height = vertex.Height,
                        Depth = vertex.Depth
                    };

                    if (vertex.OutDegree > 0)
                    {
                        vertexDto.Outgoing = new System.Collections.Generic.List<int>(vertex.OutDegree);
                        foreach (var target in vertex.Outgoing)
                        {
                            if (indexOf.TryGetValue(target, out int index))
                            {
                                vertexDto.Outgoing.Add(index);
                            }
                        }
                    }

                    dto.Vertices.Add(vertexDto);
                }
            }

            return Ok(outJson, JsonSerializer.Serialize(dto, BridgeJsonContext.Default.ProjectGraphDto));
        }
        catch (Exception ex)
        {
            return Fail(ex, errorJson);
        }
    }

    // ----- plumbing -----

    private static void InvokeProgress(IntPtr callback, IntPtr context, double ratio)
    {
        if (callback == IntPtr.Zero)
        {
            return;
        }

        ((delegate* unmanaged<IntPtr, double, void>)callback)(context, ratio);
    }

    private static void Clear(IntPtr* slot)
    {
        if (slot != null)
        {
            *slot = IntPtr.Zero;
        }
    }

    private static int Ok(IntPtr* slot, string value)
    {
        if (slot != null)
        {
            *slot = NativeStrings.ToNative(value);
        }

        return StatusOk;
    }

    private static int Fail(Exception ex, IntPtr* errorJson)
    {
        if (ex is OperationCanceledException)
        {
            return StatusCancelled;
        }

        if (ex is BadHandleException)
        {
            return StatusBadHandle;
        }

        int status = ex is BadNodeIdException ? StatusBadNodeId : StatusError;
        if (errorJson != null)
        {
            try
            {
                *errorJson = NativeStrings.ToNative(
                    BridgeJsonContext.SerializeError(ex.GetType().Name, ex.Message));
            }
            catch
            {
                // Even error reporting must never throw across the ABI.
            }
        }

        return status;
    }
}
