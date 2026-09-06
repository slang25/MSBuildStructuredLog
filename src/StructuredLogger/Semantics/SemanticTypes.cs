using System.Collections.Generic;

namespace Microsoft.Build.Logging.StructuredLogger;

public enum SemanticSymbolKind
{
    Property,
    Item,
    Target
}

/// <summary>A place a symbol is defined, assigned or executed.</summary>
public sealed class SemanticLocation
{
    public string FilePath { get; set; }

    /// <summary>1-based; 0 when only the file is known.</summary>
    public int Line { get; set; }

    /// <summary>Short human label, e.g. "Directory.Build.props:12".</summary>
    public string Label { get; set; }

    /// <summary>Extra context for the row, e.g. the value assigned here.</summary>
    public string Detail { get; set; }

    /// <summary>Node to reveal in the build tree, when this location has one.</summary>
    public BaseNode Node { get; set; }
}

/// <summary>One label/value row in a quick-info popover.</summary>
public sealed class SemanticFact
{
    public SemanticFact()
    {
    }

    public SemanticFact(string label, string value)
    {
        Label = label;
        Value = value;
    }

    public string Label { get; set; }
    public string Value { get; set; }
}

/// <summary>The answer to "what is <c>Name</c> in this evaluation?".</summary>
public sealed class SemanticSymbol
{
    public SemanticSymbolKind Kind { get; set; }
    public string Name { get; set; }

    /// <summary>False when the evaluation has no such property/item/target.</summary>
    public bool Found { get; set; }

    /// <summary>Effective value (property) or a summary line (item/target).</summary>
    public string Value { get; set; }

    /// <summary>
    /// Caveat to show under the value — e.g. that assignment locations are
    /// unavailable because the build ran without property tracking.
    /// </summary>
    public string Note { get; set; }

    public List<SemanticLocation> Definitions { get; } = new List<SemanticLocation>();

    /// <summary>Where a target actually ran; empty for properties and items.</summary>
    public List<SemanticLocation> Executions { get; } = new List<SemanticLocation>();

    public List<SemanticFact> Facts { get; } = new List<SemanticFact>();
}

/// <summary>
/// One evaluation a source file participated in. The same
/// Directory.Build.props is imported by many projects and means something
/// different in each, so every semantic answer is scoped to one of these.
/// </summary>
public sealed class SemanticEvaluationContext
{
    public ProjectEvaluation Evaluation { get; set; }
    public string ProjectFile { get; set; }

    /// <summary>File name plus dimensions, e.g. "Foo.csproj (net8.0, Debug)".</summary>
    public string Label { get; set; }

    /// <summary>True when the file being viewed *is* this evaluation's project file.</summary>
    public bool IsProjectFile { get; set; }
}

/// <summary>An <c>&lt;Import&gt;</c> element and the file it resolved to.</summary>
public sealed class SemanticImport
{
    /// <summary>1-based location of the Import element in the containing file.</summary>
    public int Line { get; set; }

    public int Column { get; set; }
    public string ImportedFilePath { get; set; }
    public BaseNode Node { get; set; }
}

/// <summary>
/// An <c>&lt;Import&gt;</c> element MSBuild evaluated but did not follow.
/// </summary>
public sealed class SemanticSkippedImport
{
    /// <summary>1-based location of the Import element in the containing file.</summary>
    public int Line { get; set; }

    public int Column { get; set; }

    /// <summary>The Project attribute as logged — usually still unexpanded.</summary>
    public string FileSpec { get; set; }

    /// <summary>Prose reason, e.g. "Not imported due to no matching files".</summary>
    public string Reason { get; set; }

    /// <summary>The Condition attribute, when that is why it was skipped.</summary>
    public string Condition { get; set; }

    /// <summary>What <see cref="Condition"/> expanded to. Set with it.</summary>
    public string EvaluatedCondition { get; set; }

    public BaseNode Node { get; set; }
}

/// <summary>A <c>&lt;Target Name="..."&gt;</c> element found in a file.</summary>
public sealed class SemanticTargetDefinition
{
    public string Name { get; set; }
    public string FilePath { get; set; }

    /// <summary>1-based.</summary>
    public int Line { get; set; }
}

/// <summary>
/// Everything file-scoped and cheap enough to hand over when a file is
/// opened: which evaluations it belongs to, and the import edges and target
/// definitions it contains.
/// </summary>
public sealed class SemanticFile
{
    public string FilePath { get; set; }
    public SemanticEvaluationContext Context { get; set; }

    /// <summary>Contexts offered in the picker (may be truncated).</summary>
    public List<SemanticEvaluationContext> Contexts { get; } = new List<SemanticEvaluationContext>();

    /// <summary>Total contexts before truncation.</summary>
    public int ContextsTotal { get; set; }

    public List<SemanticImport> Imports { get; } = new List<SemanticImport>();

    /// <summary>Imports in this file that the build evaluated and declined.</summary>
    public List<SemanticSkippedImport> SkippedImports { get; } = new List<SemanticSkippedImport>();

    public List<SemanticTargetDefinition> Targets { get; } = new List<SemanticTargetDefinition>();
}
