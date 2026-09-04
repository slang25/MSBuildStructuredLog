using System.Collections.Generic;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// JSON payloads crossing the C ABI. All serialized camelCase via
/// <see cref="BridgeJsonContext"/>; null members are omitted.
/// </summary>
public sealed class NodeSummaryDto
{
    public string Id { get; set; }

    /// <summary>CLR type name (Project, Target, Task, Error, ...).</summary>
    public string Kind { get; set; }

    /// <summary>One-line display text, matching the viewer's tree row.</summary>
    public string Title { get; set; }

    public string Name { get; set; }
    public string Value { get; set; }
    public bool HasChildren { get; set; }
    public int ChildCount { get; set; }
    public bool IsLowRelevance { get; set; }

    /// <summary>succeeded | failed | skipped | none</summary>
    public string State { get; set; }

    public double? DurationMs { get; set; }
    public bool HasSource { get; set; }
    public bool CanPreprocess { get; set; }

    /// <summary>
    /// Index of this node in its parent's natural child order, when cheaply
    /// known (children pages and ancestor chains); null otherwise.
    /// </summary>
    public int? ChildIndex { get; set; }

    /// <summary>Small kind-specific extras (fromAssembly, line, version, ...).</summary>
    public Dictionary<string, string> Props { get; set; }
}

public sealed class NodeDetailsDto
{
    public NodeSummaryDto Node { get; set; }
    public string ParentId { get; set; }
    public string StartTime { get; set; }
    public string EndTime { get; set; }
    public string FullText { get; set; }
    public string SourceFile { get; set; }
    public int? SourceLine { get; set; }
}

public sealed class ChildrenPageDto
{
    public string ParentId { get; set; }
    public int Total { get; set; }
    public int Offset { get; set; }
    public int Count { get; set; }
    public int SortMode { get; set; }
    public List<NodeSummaryDto> Children { get; set; }
}

public sealed class AncestorsDto
{
    /// <summary>
    /// Root first, the requested node itself last. Every entry carries
    /// childIndex (its position in its parent) except the root.
    /// </summary>
    public List<NodeSummaryDto> Chain { get; set; }
}

public sealed class HighlightDto
{
    public string Text { get; set; }
    public bool IsHighlight { get; set; }

    /// <summary>Optional style hint, e.g. "time".</summary>
    public string Style { get; set; }
}

public sealed class SearchTreeNodeDto
{
    /// <summary>The real node behind this row; null for synthetic folders/notes.</summary>
    public NodeSummaryDto Node { get; set; }

    /// <summary>Display text for synthetic rows (and fallback for real ones).</summary>
    public string Text { get; set; }

    public List<HighlightDto> Highlights { get; set; }
    public List<SearchTreeNodeDto> Children { get; set; }
}

public sealed class SearchResponseDto
{
    public string Query { get; set; }
    public int ResultCount { get; set; }

    /// <summary>True when the result count hit the requested cap.</summary>
    public bool Overflow { get; set; }

    public double ElapsedMs { get; set; }
    public List<SearchTreeNodeDto> Roots { get; set; }
}

public sealed class SourceLocationDto
{
    public string FilePath { get; set; }
    public int? Line { get; set; }

    /// <summary>Full text of the file from the embedded archive; null if unavailable.</summary>
    public string Text { get; set; }
}

public sealed class FileEntryDto
{
    public string Path { get; set; }
    public int Lines { get; set; }
    public int Length { get; set; }
}

public sealed class FileListDto
{
    public int Total { get; set; }
    public List<FileEntryDto> Files { get; set; }
}

public sealed class FileMatchSpanDto
{
    public int Start { get; set; }
    public int Length { get; set; }
}

public sealed class FileMatchDto
{
    /// <summary>1-based line number.</summary>
    public int Line { get; set; }

    public string Text { get; set; }
    public List<FileMatchSpanDto> Spans { get; set; }
}

public sealed class FileMatchesDto
{
    public string Path { get; set; }
    public List<FileMatchDto> Matches { get; set; }
}

public sealed class FileSearchResponseDto
{
    public string Query { get; set; }
    public int TotalMatches { get; set; }
    public bool Overflow { get; set; }
    public List<FileMatchesDto> Files { get; set; }
}

public sealed class BuildInfoDto
{
    public string RootId { get; set; }
    public bool Succeeded { get; set; }
    public int ErrorCount { get; set; }
    public int WarningCount { get; set; }
    public int NodeCount { get; set; }
    public bool HasSourceArchive { get; set; }
    public string MSBuildVersion { get; set; }
    public string FilePath { get; set; }
    public long FileSize { get; set; }
    public double DurationMs { get; set; }
    public string StartTime { get; set; }
    public string EndTime { get; set; }
}

public sealed class StatsRecordDto
{
    public string Type { get; set; }
    public long TotalLength { get; set; }
    public int Count { get; set; }
    public int Largest { get; set; }
    public List<StatsRecordDto> Children { get; set; }
}

public sealed class StatsDto
{
    public long FileSize { get; set; }
    public long UncompressedStreamSize { get; set; }
    public long RecordCount { get; set; }
    public int FileFormatVersion { get; set; }
    public int StringCount { get; set; }
    public long StringTotalSize { get; set; }
    public int StringLargest { get; set; }
    public int NameValueListCount { get; set; }
    public long NameValueListTotalSize { get; set; }
    public int NameValueListLargest { get; set; }
    public int BlobCount { get; set; }
    public long BlobTotalSize { get; set; }
    public int BlobLargest { get; set; }
    public StatsRecordDto Records { get; set; }
}

public sealed class TimelineBlockDto
{
    public string Id { get; set; }
    public string Kind { get; set; }
    public string Text { get; set; }

    /// <summary>Milliseconds relative to the timeline start.</summary>
    public double Start { get; set; }

    public double End { get; set; }

    /// <summary>Nesting depth within the lane (0 = outermost).</summary>
    public int Indent { get; set; }

    public bool HasError { get; set; }
}

public sealed class TimelineLaneDto
{
    /// <summary>MSBuild worker node id (0 = evaluation/main).</summary>
    public int NodeId { get; set; }

    /// <summary>Sorted by start time.</summary>
    public List<TimelineBlockDto> Blocks { get; set; }

    public int MaxIndent { get; set; }
}

public sealed class TimelineDto
{
    /// <summary>Timeline origin (earliest block start), ISO-8601.</summary>
    public string StartTime { get; set; }

    /// <summary>Total span in milliseconds.</summary>
    public double DurationMs { get; set; }

    /// <summary>Sorted by node id.</summary>
    public List<TimelineLaneDto> Lanes { get; set; }
}

public sealed class ProjectGraphVertexDto
{
    /// <summary>Full project path (the graph key).</summary>
    public string Value { get; set; }

    /// <summary>Short display name.</summary>
    public string Title { get; set; }

    /// <summary>Longest path to a sink (0 = leaf dependency).</summary>
    public int Height { get; set; }

    /// <summary>Longest path from a source (0 = top-level project).</summary>
    public int Depth { get; set; }

    /// <summary>Direct references, as indices into the vertices array.</summary>
    public List<int> Outgoing { get; set; }
}

public sealed class ProjectGraphDto
{
    public List<ProjectGraphVertexDto> Vertices { get; set; }
}

public sealed class ErrorDto
{
    public string Code { get; set; }
    public string Message { get; set; }
}

// ----- semantics (Cmd-click / quick info over MSBuild source) -----

/// <summary>
/// One evaluation a source file participated in. Semantic answers are
/// meaningless without one: the same .props file is imported by many
/// projects and resolves differently in each.
/// </summary>
public sealed class SemanticContextDto
{
    /// <summary>Node id of the ProjectEvaluation; also revealable in the tree.</summary>
    public string EvaluationId { get; set; }

    public string ProjectFile { get; set; }

    /// <summary>File name plus dimensions, e.g. "Foo.csproj (net8.0, Debug)".</summary>
    public string Label { get; set; }

    /// <summary>True when the viewed file *is* this evaluation's project file.</summary>
    public bool IsProjectFile { get; set; }
}

public sealed class SemanticImportDto
{
    /// <summary>1-based location of the Import element in the viewed file.</summary>
    public int Line { get; set; }

    public int Column { get; set; }
    public string ImportedPath { get; set; }

    /// <summary>False when the imported file can't be opened (not archived).</summary>
    public bool Available { get; set; }
}

/// <summary>
/// An Import element the build evaluated and declined to follow.
/// </summary>
public sealed class SemanticSkippedImportDto
{
    /// <summary>1-based location of the Import element in the viewed file.</summary>
    public int Line { get; set; }

    public int Column { get; set; }

    /// <summary>The Project attribute as logged — usually still unexpanded.</summary>
    public string FileSpec { get; set; }

    /// <summary>Prose reason, e.g. "Not imported due to no matching files".</summary>
    public string Reason { get; set; }

    /// <summary>The Condition attribute, when that is why it was skipped.</summary>
    public string Condition { get; set; }

    /// <summary>What Condition expanded to, e.g. "'' != ''". Set with it.</summary>
    public string EvaluatedCondition { get; set; }
}

public sealed class SemanticTargetDefinitionDto
{
    public string Name { get; set; }

    /// <summary>1-based.</summary>
    public int Line { get; set; }
}

public sealed class SemanticFileDto
{
    public string Path { get; set; }

    /// <summary>The context these imports/targets were resolved under.</summary>
    public string EvaluationId { get; set; }

    public List<SemanticContextDto> Contexts { get; set; }

    /// <summary>Contexts before truncation; more than Contexts.Count means the list was capped.</summary>
    public int ContextsTotal { get; set; }

    public List<SemanticImportDto> Imports { get; set; }

    /// <summary>Imports in this file the build evaluated and declined.</summary>
    public List<SemanticSkippedImportDto> SkippedImports { get; set; }

    public List<SemanticTargetDefinitionDto> Targets { get; set; }
}

public sealed class SemanticLocationDto
{
    public string Path { get; set; }
    public int Line { get; set; }
    public string Label { get; set; }
    public string Detail { get; set; }

    /// <summary>Node to reveal in the build tree, when this location has one.</summary>
    public string NodeId { get; set; }

    /// <summary>False when Path can't be opened (not archived, not on disk).</summary>
    public bool Available { get; set; }
}

public sealed class SemanticFactDto
{
    public string Label { get; set; }
    public string Value { get; set; }
}

public sealed class SemanticSymbolDto
{
    /// <summary>property | item | target</summary>
    public string Kind { get; set; }

    public string Name { get; set; }
    public bool Found { get; set; }
    public string Value { get; set; }

    /// <summary>Caveat to show under the value (e.g. property tracking was off).</summary>
    public string Note { get; set; }

    public List<SemanticLocationDto> Definitions { get; set; }

    /// <summary>Where a target actually ran; empty for properties and items.</summary>
    public List<SemanticLocationDto> Executions { get; set; }

    public List<SemanticFactDto> Facts { get; set; }
}
