using Gpui;
using StructuredLogViewer.NativeBridge;

namespace StructuredLogViewer.Gpui;

/// <summary>
/// Row appearance per node kind: a glyph and a tint, mirroring the Mac
/// viewer's NodeStyling (which in turn mirrors the Avalonia icon set:
/// target=purple, task=blue, item=mint, folder=gold, import=brown, ...).
/// GPUI has no SF Symbols, so glyphs are plain Unicode text.
/// </summary>
public static class NodeStyle
{
    public readonly record struct Style(string Glyph, Color Color);

    private static readonly Color Green = Colors.Hex("#3FB950");
    private static readonly Color Red = Colors.Hex("#F85149");
    private static readonly Color Purple = Colors.Hex("#BC8CFF");
    private static readonly Color Blue = Colors.Hex("#58A6FF");
    private static readonly Color Teal = Colors.Hex("#39C5CF");
    private static readonly Color Mint = Colors.Hex("#56D4B1");
    private static readonly Color Yellow = Colors.Hex("#E3B341");
    private static readonly Color Orange = Colors.Hex("#F0883E");
    private static readonly Color Brown = Colors.Hex("#C08C5A");
    private static readonly Color Indigo = Colors.Hex("#8B8CFF");
    private static readonly Color Muted = Colors.Hex("#8B949E");
    private static readonly Color Navy = Colors.Hex("#4C8DC8");

    public static Style For(NodeSummaryDto node) =>
        node.Kind switch
        {
            "Build" => new Style("●", node.State == "failed" ? Red : Green),
            "Project" => new Style("▣", ProjectTint(node)),
            "ProjectEvaluation" => new Style("▢", ProjectTint(node)),
            "Target" => new Style("◎", node.State == "failed" ? Red : Purple),
            "EntryTarget" => new Style("◉", Purple),
            "Task" => new Style("⚙", Blue),
            "AddItem" => new Style("⊞", Teal),
            "RemoveItem" => new Style("⊟", Red),
            "Item" or "TaskParameterItem" => new Style("▤", Mint),
            "Metadata" => new Style("◇", Teal),
            "Property" or "TaskParameterProperty" => new Style("P", Blue),
            "Parameter" => new Style("→", Blue),
            "Folder" => new Style("▪", Yellow),
            "Message" or "TimedMessage" => new Style("≡", Muted),
            "CriticalBuildMessage" => new Style("!", Orange),
            "Error" => new Style("✕", Red),
            "Warning" => new Style("▲", Yellow),
            "Note" => new Style("✎", Muted),
            "Import" => new Style("↓", Brown),
            "NoImport" => new Style("↓", Red),
            "Package" => new Style("◆", Navy),
            "FileCopy" => new Style("⇄", Blue),
            "SourceFile" => new Style("▭", Blue),
            "SourceFileLine" => new Style("¶", Muted),
            "EvaluationProfileEntry" or "TimedNode" => new Style("◷", Muted),
            "MSBuildServerNode" => new Style("▦", Indigo),
            _ => new Style("▫", Muted),
        };

    /// <summary>Failed/skipped targets and tasks take a state accent over their tint.</summary>
    public static Color? StateAccent(NodeSummaryDto node) =>
        node.State switch
        {
            "failed" => Red,
            "skipped" => Muted,
            _ => null,
        };

    private static Color ProjectTint(NodeSummaryDto node)
    {
        string ext = node.Props?.GetValueOrDefault("extension")?.ToLowerInvariant() ?? string.Empty;
        return ext switch
        {
            ".csproj" => Colors.Hex("#5CB85C"),
            ".vbproj" => Colors.Hex("#3E8ED0"),
            ".fsproj" => Colors.Hex("#B07CC6"),
            ".vcxproj" or ".cppproj" => Colors.Hex("#C77DD9"),
            ".sln" or ".slnx" or ".slnf" => Colors.Hex("#9D5CB8"),
            ".esproj" => Yellow,
            _ => Muted,
        };
    }
}

/// <summary>One styled run of a tree row — the C# twin of the Mac viewer's NodeRowSegment.</summary>
public enum SegmentStyle
{
    /// <summary>The type name a row leads with, tinted like its glyph ("Import", "NoImport").</summary>
    KindLabel,

    /// <summary>The row's own text.</summary>
    Primary,

    /// <summary>Supporting detail that shouldn't compete, e.g. " at (49;3)".</summary>
    Secondary,

    /// <summary>A short fact on a highlight fill — the reason an import was skipped.</summary>
    Chip,

    /// <summary>A project's target framework(s) on a tinted fill.</summary>
    Badge,

    /// <summary>The initial targets a project was built with, tinted like a target.</summary>
    Targets,

    /// <summary>Elapsed time, monospaced.</summary>
    Duration,
}

public readonly record struct RowSegment(string Text, SegmentStyle Style);

/// <summary>
/// Composes a row's runs from a node summary, mirroring the WPF and Avalonia
/// per-type templates (and the Mac viewer's NodeRowText). Only kinds that
/// need more than their title appear here; everything else is one run.
/// </summary>
public static class NodeRowText
{
    public static List<RowSegment> Segments(NodeSummaryDto node)
    {
        var segments = new List<RowSegment>(4);
        switch (node.Kind)
        {
            case "Import":
                segments.Add(new("Import", SegmentStyle.KindLabel));
                segments.Add(new(node.Title, SegmentStyle.Primary));
                AppendLocation(node, segments);
                break;

            case "NoImport":
                segments.Add(new("NoImport", SegmentStyle.KindLabel));
                segments.Add(new(node.Title, SegmentStyle.Primary));
                AppendLocation(node, segments);
                if (node.Props?.GetValueOrDefault("reason") is { Length: > 0 } reason)
                {
                    segments.Add(new(reason, SegmentStyle.Chip));
                }

                break;

            case "Project":
            case "ProjectEvaluation":
                AppendProject(node, segments);
                break;

            default:
                segments.Add(new(node.Title, SegmentStyle.Primary));
                break;
        }

        if (node.DurationMs is > 0 and var duration)
        {
            segments.Add(new(ViewerSession.FormatDuration(duration), SegmentStyle.Duration));
        }

        return segments;
    }

    private static void AppendProject(NodeSummaryDto node, List<RowSegment> segments)
    {
        if (string.IsNullOrEmpty(node.Name))
        {
            segments.Add(new(node.Title, SegmentStyle.Primary));
            return;
        }

        segments.Add(new(node.Name, SegmentStyle.Primary));

        if (node.Props?.GetValueOrDefault("adornment") is { Length: > 0 } adornment)
        {
            segments.Add(new(adornment, SegmentStyle.Badge));
        }

        if (node.Props?.GetValueOrDefault("targetsText") is { Length: > 0 } targets)
        {
            segments.Add(new(targets, SegmentStyle.Targets));
        }

        if (node.Props?.GetValueOrDefault("evaluationText") is { Length: > 0 } evaluation)
        {
            segments.Add(new(evaluation, SegmentStyle.Secondary));
        }
    }

    private static void AppendLocation(NodeSummaryDto node, List<RowSegment> segments)
    {
        if (node.Props is { } props
            && props.TryGetValue("line", out var line)
            && props.TryGetValue("column", out var column))
        {
            segments.Add(new($"at ({line};{column})", SegmentStyle.Secondary));
        }
    }
}
