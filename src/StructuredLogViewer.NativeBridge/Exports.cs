using System;
using System.Globalization;
using System.IO;
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
                page.Children.Add(NodeFormatter.CreateSummary(session, children[i]));
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

            var chain = new System.Collections.Generic.List<NodeSummaryDto>();
            var current = node.Parent;
            while (current != null)
            {
                chain.Add(NodeFormatter.CreateSummary(session, current));
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
