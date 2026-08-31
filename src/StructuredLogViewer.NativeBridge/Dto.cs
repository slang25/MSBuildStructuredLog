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
    /// <summary>Root first, immediate parent last.</summary>
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

public sealed class ErrorDto
{
    public string Code { get; set; }
    public string Message { get; set; }
}
