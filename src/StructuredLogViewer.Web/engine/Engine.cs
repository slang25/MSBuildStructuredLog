using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Runtime.InteropServices.JavaScript;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Threading;
using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer.NativeBridge;

namespace StructuredLogViewer.WebEngine;

/// <summary>
/// The browser-wasm engine: one JSExport dispatcher over the NativeBridge classes.
/// Every method returns the same JSON payload the corresponding <c>mslog_*</c> export in
/// <c>Exports.cs</c> produces (see include/mslog.h), serialized with the bridge's
/// source-generated <see cref="BridgeJsonContext"/>. Nothing ever throws across the
/// export; failures come back as <c>{"error":{"code","message"}}</c>.
/// </summary>
public static partial class Engine
{
    private static BridgeSession session;

    /// <summary>Raised on the JS side as postMessage({event:"progress", ratio}).</summary>
    [JSImport("progress", "engine-worker")]
    internal static partial void Progress(double ratio);

    [JSExport]
    public static string Call(string method, string argsJson)
    {
        try
        {
            using var doc = JsonDocument.Parse(string.IsNullOrWhiteSpace(argsJson) ? "{}" : argsJson);
            var args = doc.RootElement;
            return Dispatch(method ?? string.Empty, args);
        }
        catch (BadNodeIdException ex)
        {
            return Error("BadNodeId", ex.Message);
        }
        catch (NoSessionException ex)
        {
            return Error("NoSession", ex.Message);
        }
        catch (Exception ex)
        {
            return Error(ex.GetType().Name, ex.Message);
        }
    }

    private static string Dispatch(string method, JsonElement args)
    {
        switch (method)
        {
            case "open":
                return Open(GetString(args, "path") ?? throw new ArgumentException("'path' is required."));

            case "close":
                Close();
                return "{}";

            case "build_info":
                return BuildInfo();

            case "node_get":
            {
                var s = Session;
                var node = s.ResolveNode(RequireString(args, "id"));
                var details = NodeFormatter.CreateDetails(s, node);
                return JsonSerializer.Serialize(details, BridgeJsonContext.Default.NodeDetailsDto);
            }

            case "node_children":
                return NodeChildren(
                    RequireString(args, "id"),
                    GetInt(args, "offset", 0),
                    GetInt(args, "count", 0),
                    GetInt(args, "sortMode", 0));

            case "node_ancestors":
                return NodeAncestors(RequireString(args, "id"));

            case "node_source":
                return NodeSource(RequireString(args, "id"));

            case "node_preprocess":
                return Text(NodePreprocess(RequireString(args, "id")));

            case "node_subtree_text":
            {
                var node = Session.ResolveNode(RequireString(args, "id"));
                return Text(SearchExecution.GetSubtreeText(node) ?? string.Empty);
            }

            case "target_parent":
                return TargetParent(RequireString(args, "id"));

            case "file_read":
                return Text(FileSearch.ReadFile(Session, RequireString(args, "path")));

            case "files_list":
                return JsonSerializer.Serialize(FileSearch.ListFiles(Session), BridgeJsonContext.Default.FileListDto);

            case "files_search":
            {
                var response = FileSearch.SearchFiles(
                    Session,
                    GetString(args, "term") ?? GetString(args, "query"),
                    GetInt(args, "maxResults", 0),
                    CancellationToken.None);
                return JsonSerializer.Serialize(response, BridgeJsonContext.Default.FileSearchResponseDto);
            }

            case "search":
            {
                var response = SearchExecution.Search(
                    Session,
                    GetString(args, "query") ?? string.Empty,
                    GetInt(args, "maxResults", 0),
                    CancellationToken.None);
                return JsonSerializer.Serialize(response, BridgeJsonContext.Default.SearchResponseDto);
            }

            case "search_properties_and_items":
            {
                var response = SearchExecution.SearchPropertiesAndItems(
                    Session,
                    RequireString(args, "contextNodeId"),
                    GetString(args, "query") ?? string.Empty,
                    GetInt(args, "maxResults", 0),
                    CancellationToken.None);
                return JsonSerializer.Serialize(response, BridgeJsonContext.Default.SearchResponseDto);
            }

            case "semantic_file":
            {
                var dto = SemanticFormatter.CreateFile(
                    Session,
                    RequireString(args, "path"),
                    GetString(args, "evaluationId"));
                return JsonSerializer.Serialize(dto, BridgeJsonContext.Default.SemanticFileDto);
            }

            case "semantic_resolve":
            {
                var dto = SemanticFormatter.Resolve(
                    Session,
                    RequireString(args, "evaluationId"),
                    RequireString(args, "kind"),
                    RequireString(args, "name"));
                return JsonSerializer.Serialize(dto, BridgeJsonContext.Default.SemanticSymbolDto);
            }

            case "timeline":
            {
                var timeline = TimelineFormatter.Build(Session, CancellationToken.None);
                return JsonSerializer.Serialize(timeline, BridgeJsonContext.Default.TimelineDto);
            }

            case "project_graph":
                return ProjectGraph();

            case "build_stats":
            {
                var stats = StatsFormatter.Calculate(Session, CancellationToken.None);
                return JsonSerializer.Serialize(stats, BridgeJsonContext.Default.StatsDto);
            }

            default:
                throw new ArgumentException($"Unknown method '{method}'.");
        }
    }

    // ----- lifecycle -----

    private static bool hostConfigured;

    /// <summary>
    /// Host hooks the engine expects a viewer to inject. The defaults hash the
    /// preprocessed text with MD5, which browser-wasm does not implement
    /// (CryptographicException: Cryptography_UnknownHashAlgorithm, MD5), and
    /// write under Path.GetTempPath(); use a plain FNV-1a hash and the
    /// emscripten in-memory /tmp instead.
    /// </summary>
    private static void ConfigureHost()
    {
        if (hostConfigured)
        {
            return;
        }

        hostConfigured = true;
        PreprocessedFileManager.GetPreprocessedFilePath = GetPreprocessedFilePath;
        PreprocessedFileManager.WriteContentToTempFileAndGetPath = (content, extension) =>
        {
            string filePath = GetPreprocessedFilePath(content, extension);
            System.IO.Directory.CreateDirectory(System.IO.Path.GetDirectoryName(filePath));
            if (!System.IO.File.Exists(filePath))
            {
                System.IO.File.WriteAllText(filePath, content);
            }

            return filePath;
        };
    }

    private static string GetPreprocessedFilePath(string content, string extension)
    {
        ulong hash = 14695981039346656037UL;
        foreach (char c in content ?? string.Empty)
        {
            hash = (hash ^ c) * 1099511628211UL;
        }

        return System.IO.Path.Combine("/tmp/MSBuildStructuredLog", hash.ToString("x16") + extension);
    }

    private static string Open(string path)
    {
        Close();
        ConfigureHost();

        var progress = new Progress();
        double last = -1;
        progress.Updated += update =>
        {
            // The reader reports per buffer; throttle to every 0.5% (or completion).
            if (update.Ratio >= 1 || update.Ratio - last >= 0.005)
            {
                last = update.Ratio;
                try
                {
                    Progress(update.Ratio);
                }
                catch
                {
                    // Progress is advisory; never let a JS-side failure abort the load.
                }
            }
        };

        session = BridgeSession.Load(path, progress, CancellationToken.None);
        return BuildInfo();
    }

    private static void Close()
    {
        if (session == null)
        {
            return;
        }

        session = null;
        GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
    }

    private static BridgeSession Session => session ?? throw new NoSessionException();

    // ----- payload builders mirroring Exports.cs -----

    private static string BuildInfo()
    {
        var s = Session;
        var build = s.Build;

        var info = new BuildInfoDto
        {
            RootId = BinlogMcp.NodeId.Get(build),
            Succeeded = build.Succeeded,
            ErrorCount = s.ErrorCount,
            WarningCount = s.WarningCount,
            NodeCount = build.SearchIndex?.NodeCount ?? 0,
            HasSourceArchive = build.SourceFiles is { Count: > 0 },
            MSBuildVersion = build.MSBuildVersion,
            FilePath = s.Path,
            FileSize = s.FileSize,
            DurationMs = build.Duration.TotalMilliseconds,
            StartTime = build.StartTime.ToString("o", CultureInfo.InvariantCulture),
            EndTime = build.EndTime.ToString("o", CultureInfo.InvariantCulture)
        };

        return JsonSerializer.Serialize(info, BridgeJsonContext.Default.BuildInfoDto);
    }

    private static string NodeChildren(string id, int offset, int count, int sortMode)
    {
        var s = Session;
        var node = s.ResolveNode(id);

        int skip = Math.Max(offset, 0);
        int take = Math.Clamp(count <= 0 ? 512 : count, 1, 5000);

        var children = s.GetSortedChildren(node, sortMode);
        int total = children.Count;

        var page = new ChildrenPageDto
        {
            ParentId = id,
            Total = total,
            Offset = skip,
            SortMode = sortMode,
            Children = new List<NodeSummaryDto>()
        };

        int end = Math.Min(total, skip + take);
        for (int i = skip; i < end; i++)
        {
            var summary = NodeFormatter.CreateSummary(s, children[i]);
            if (sortMode == 0)
            {
                summary.ChildIndex = i;
            }

            page.Children.Add(summary);
        }

        page.Count = page.Children.Count;
        return JsonSerializer.Serialize(page, BridgeJsonContext.Default.ChildrenPageDto);
    }

    private static string NodeAncestors(string id)
    {
        var s = Session;
        var node = s.ResolveNode(id);

        var chain = new List<NodeSummaryDto>();
        BaseNode current = node;
        while (current != null)
        {
            var summary = NodeFormatter.CreateSummary(s, current);
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
        return JsonSerializer.Serialize(new AncestorsDto { Chain = chain }, BridgeJsonContext.Default.AncestorsDto);
    }

    private static string NodeSource(string id)
    {
        var s = Session;
        var node = s.ResolveNode(id);

        if (!NodeFormatter.TryGetSourceLocation(node, out string filePath, out int? line))
        {
            throw new InvalidOperationException("Node has no source location.");
        }

        var location = new SourceLocationDto
        {
            FilePath = filePath,
            Line = line,
            Text = s.SourceFileResolver.GetSourceFileText(filePath)?.Text
        };

        return JsonSerializer.Serialize(location, BridgeJsonContext.Default.SourceLocationDto);
    }

    private static string NodePreprocess(string id)
    {
        var s = Session;
        var node = s.ResolveNode(id);

        if (node is not IPreprocessable preprocessable)
        {
            throw new InvalidOperationException(
                $"Node is a {node.GetType().Name}; only projects, evaluations and imports can be preprocessed.");
        }

        string text = s.PreprocessedFileManager.GetPreprocessedText(
            preprocessable.RootFilePath,
            PreprocessedFileManager.GetNodeEvaluationKey(node));

        if (string.IsNullOrEmpty(text))
        {
            throw new InvalidOperationException(
                $"No preprocessed text available for '{preprocessable.RootFilePath}'.");
        }

        return text;
    }

    private static string TargetParent(string id)
    {
        var s = Session;
        var node = s.ResolveNode(id);

        if (node is not Target target)
        {
            throw new InvalidOperationException(
                $"Node is a {node.GetType().Name}; only targets have a parent-target link.");
        }

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

        return JsonSerializer.Serialize(NodeFormatter.CreateSummary(s, destination), BridgeJsonContext.Default.NodeSummaryDto);
    }

    private static string ProjectGraph()
    {
        var graph = Session.Build.ProjectReferenceGraph?.Graph;
        var dto = new ProjectGraphDto { Vertices = new List<ProjectGraphVertexDto>() };

        if (graph != null && !graph.IsEmpty)
        {
            var vertices = graph.Vertices
                .OrderBy(v => v.Title ?? v.Value, StringComparer.OrdinalIgnoreCase)
                .ToArray();
            var indexOf = new Dictionary<Vertex, int>(vertices.Length);
            for (int i = 0; i < vertices.Length; i++)
            {
                indexOf[vertices[i]] = i;
            }

            foreach (var vertex in vertices)
            {
                var vertexDto = new ProjectGraphVertexDto
                {
                    Value = vertex.Value,
                    Title = (vertex.Title ?? vertex.Value).Trim('"'),
                    Height = vertex.Height,
                    Depth = vertex.Depth
                };

                if (vertex.OutDegree > 0)
                {
                    vertexDto.Outgoing = new List<int>(vertex.OutDegree);
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

        return JsonSerializer.Serialize(dto, BridgeJsonContext.Default.ProjectGraphDto);
    }

    // ----- JSON plumbing (source-generated; reflection serialization is trimmed away under wasm) -----

    private static string Text(string text) =>
        JsonSerializer.Serialize(new TextDto { Text = text ?? string.Empty }, EngineJsonContext.Default.TextDto);

    private static string Error(string code, string message) =>
        JsonSerializer.Serialize(
            new ErrorEnvelopeDto { Error = new ErrorDto { Code = code, Message = message } },
            EngineJsonContext.Default.ErrorEnvelopeDto);

    private static string GetString(JsonElement args, string name)
    {
        if (args.ValueKind != JsonValueKind.Object || !args.TryGetProperty(name, out var value))
        {
            return null;
        }

        return value.ValueKind switch
        {
            JsonValueKind.String => value.GetString(),
            JsonValueKind.Null or JsonValueKind.Undefined => null,
            _ => value.GetRawText()
        };
    }

    private static string RequireString(JsonElement args, string name) =>
        GetString(args, name) ?? throw new ArgumentException($"'{name}' is required.");

    private static int GetInt(JsonElement args, string name, int fallback)
    {
        if (args.ValueKind != JsonValueKind.Object || !args.TryGetProperty(name, out var value))
        {
            return fallback;
        }

        return value.ValueKind switch
        {
            JsonValueKind.Number => value.TryGetInt32(out int i) ? i : (int)value.GetDouble(),
            JsonValueKind.String => int.TryParse(value.GetString(), NumberStyles.Integer, CultureInfo.InvariantCulture, out int parsed) ? parsed : fallback,
            _ => fallback
        };
    }

    /// <summary>Required by OutputType=Exe; never run — the worker only calls the JSExport.</summary>
    public static void Main()
    {
    }
}

internal sealed class NoSessionException : Exception
{
    public NoSessionException()
        : base("No binlog is open; call 'open' first.")
    {
    }
}

public sealed class TextDto
{
    public string Text { get; set; }
}

public sealed class ErrorEnvelopeDto
{
    public ErrorDto Error { get; set; }
}

[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(TextDto))]
[JsonSerializable(typeof(ErrorEnvelopeDto))]
internal partial class EngineJsonContext : JsonSerializerContext
{
}
