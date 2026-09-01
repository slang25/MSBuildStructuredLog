using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;

namespace Microsoft.Build.Logging.StructuredLogger;

/// <summary>
/// Answers "go to definition" and "quick info" questions about MSBuild
/// source files using only what the build already recorded — no MSBuild
/// evaluation, no XML parse.
/// <para>
/// Every answer is scoped to a <see cref="ProjectEvaluation"/>: the same
/// Directory.Build.props is imported by many projects, and
/// <c>$(OutputPath)</c> has a different value in each. Callers pick a
/// context via <see cref="GetFileSemantics"/> and pass its evaluation back
/// in to <see cref="Resolve"/>.
/// </para>
/// </summary>
public sealed class MSBuildSemanticModel
{
    /// <summary>How many item values a quick-info popover samples.</summary>
    private const int MaxItemSamples = 12;

    /// <summary>How many assignment steps a property's history shows.</summary>
    private const int MaxAssignmentSteps = 24;

    // <Target ... Name="Build" ...>. Deliberately lexical: it also matches
    // targets inside comments, which is the cheap trade for never needing a
    // real XML parse of possibly-malformed preprocessed content.
    private static readonly Regex TargetDefinitionRegex = new Regex(
        @"<Target\s[^>]*?\bName\s*=\s*(?:""(?<name>[^""]*)""|'(?<name>[^']*)')",
        RegexOptions.Compiled | RegexOptions.Singleline);

    private readonly Build build;
    private readonly SourceFileResolver sourceFileResolver;
    private readonly object sync = new object();

    private Dictionary<string, List<ProjectEvaluation>> evaluationsByFile;
    private Dictionary<int, List<Project>> projectsByEvaluationId;

    private readonly Dictionary<string, IReadOnlyList<SemanticTargetDefinition>> targetsByFile =
        new Dictionary<string, IReadOnlyList<SemanticTargetDefinition>>(StringComparer.OrdinalIgnoreCase);

    private readonly Dictionary<int, Dictionary<string, List<SemanticTargetDefinition>>> targetsByEvaluation =
        new Dictionary<int, Dictionary<string, List<SemanticTargetDefinition>>>();

    public MSBuildSemanticModel(Build build, SourceFileResolver sourceFileResolver)
    {
        this.build = build ?? throw new ArgumentNullException(nameof(build));
        this.sourceFileResolver = sourceFileResolver ?? throw new ArgumentNullException(nameof(sourceFileResolver));
    }

    /// <summary>
    /// File-scoped facts for <paramref name="filePath"/>: the evaluations it
    /// participated in, plus the import edges and target definitions it
    /// contains under <paramref name="evaluation"/>. Pass a null evaluation
    /// to take the default (the project itself, else the first evaluation
    /// that imported the file).
    /// </summary>
    /// <param name="maxContexts">Caps the returned context list; a widely
    /// imported .props file belongs to every evaluation in the build.</param>
    public SemanticFile GetFileSemantics(string filePath, ProjectEvaluation evaluation, int maxContexts = 200)
    {
        var result = new SemanticFile { FilePath = filePath };
        if (string.IsNullOrEmpty(filePath))
        {
            return result;
        }

        var candidates = GetEvaluationsForFile(filePath);
        result.ContextsTotal = candidates.Count;

        foreach (var candidate in candidates.Take(Math.Max(1, maxContexts)))
        {
            result.Contexts.Add(CreateContext(candidate, filePath));
        }

        DisambiguateLabels(result.Contexts);

        var selected = evaluation != null && candidates.Contains(evaluation)
            ? evaluation
            : candidates.FirstOrDefault();

        // An evaluation the caller asked for but that never saw this file is
        // still honoured — better to answer in the requested context than to
        // silently switch it out from under them.
        selected ??= evaluation;

        if (selected == null)
        {
            return result;
        }

        result.Context = result.Contexts.FirstOrDefault(c => c.Evaluation == selected)
            ?? CreateContext(selected, filePath);

        foreach (var import in selected.GetAllImportsTransitive())
        {
            if (import.ImportedProjectFilePath == null ||
                !string.Equals(import.ProjectFilePath, filePath, StringComparison.OrdinalIgnoreCase))
            {
                continue;
            }

            result.Imports.Add(new SemanticImport
            {
                Line = import.Line,
                Column = import.Column,
                ImportedFilePath = import.ImportedProjectFilePath,
                Node = import
            });
        }

        result.Targets.AddRange(GetTargetsInFile(filePath));
        return result;
    }

    /// <summary>
    /// Resolves one symbol within an evaluation. Never throws for an unknown
    /// name — the result carries <see cref="SemanticSymbol.Found"/> false so
    /// the UI can say "no such property here" rather than nothing.
    /// </summary>
    public SemanticSymbol Resolve(ProjectEvaluation evaluation, SemanticSymbolKind kind, string name)
    {
        var symbol = new SemanticSymbol { Kind = kind, Name = name };
        if (evaluation == null || string.IsNullOrEmpty(name))
        {
            return symbol;
        }

        switch (kind)
        {
            case SemanticSymbolKind.Property:
                ResolveProperty(evaluation, name, symbol);
                break;
            case SemanticSymbolKind.Item:
                ResolveItem(evaluation, name, symbol);
                break;
            case SemanticSymbolKind.Target:
                ResolveTarget(evaluation, name, symbol);
                break;
        }

        return symbol;
    }

    public ProjectEvaluation FindEvaluation(int id) => build.FindEvaluation(id);

    // ----- properties -----

    private void ResolveProperty(ProjectEvaluation evaluation, string name, SemanticSymbol symbol)
    {
        var propertiesFolder = evaluation.FindChild<Folder>(Strings.Properties);
        var property = propertiesFolder?.FindChild<Property>(name);
        if (property != null)
        {
            symbol.Found = true;
            symbol.Value = property.Value;
        }

        // "Property initialized" / "Property reassignment" folders only
        // exist when the build logged property tracking; without it we can
        // still show the final value, just not where it came from.
        var initial = FindAssignments(evaluation.PropertyAssignmentFolder, name);
        var reassignments = FindAssignments(
            evaluation.FindChild<TimedNode>(Strings.PropertyReassignmentFolder), name);

        int step = 0;
        foreach (var assignment in initial)
        {
            symbol.Found = true;
            AddAssignment(symbol, assignment, "Initial value");
            step++;
        }

        for (int i = 0; i < reassignments.Count; i++)
        {
            var assignment = reassignments[i];
            symbol.Found = true;

            // The first reassignment is the only place the pre-tracking
            // value is recorded, so surface it when nothing initialized it.
            if (step == 0 && assignment is PropertyReassignmentMessage first &&
                !string.IsNullOrEmpty(first.PreviousValue))
            {
                symbol.Facts.Add(new SemanticFact("Initial value", NormalizeValue(first.PreviousValue)));
            }

            if (step >= MaxAssignmentSteps)
            {
                symbol.Facts.Add(new SemanticFact("…", $"{reassignments.Count - i} more reassignments"));
                break;
            }

            step++;
            AddAssignment(symbol, assignment, $"Set {step}");
        }

        // Last write wins in MSBuild, so "go to definition" means the final
        // assignment. Earlier ones stay available, just further down the list.
        symbol.Definitions.Reverse();
        if (symbol.Definitions.Count > 1)
        {
            symbol.Definitions[0].Label += " (wins)";
        }

        if (symbol.Found && symbol.Definitions.Count == 0)
        {
            symbol.Note = "No assignment locations were recorded — rebuild with property tracking " +
                "(MSBUILDLOGPROPERTYTRACKING) to see where this value came from.";
        }
    }

    private static List<PropertyAssignmentMessage> FindAssignments(TimedNode folder, string name)
    {
        var result = new List<PropertyAssignmentMessage>();
        var perProperty = folder?.FindChild<Folder>(name);
        if (perProperty == null)
        {
            return result;
        }

        foreach (var child in perProperty.Children)
        {
            if (child is PropertyAssignmentMessage assignment)
            {
                result.Add(assignment);
            }
        }

        return result;
    }

    private static void AddAssignment(SemanticSymbol symbol, PropertyAssignmentMessage assignment, string label)
    {
        string value = NormalizeValue(assignment.NewValue);
        symbol.Facts.Add(new SemanticFact(label, value));

        if (string.IsNullOrEmpty(assignment.FilePath))
        {
            return;
        }

        symbol.Definitions.Add(new SemanticLocation
        {
            FilePath = assignment.FilePath,
            Line = assignment.Line,
            Label = FormatLocation(assignment.FilePath, assignment.Line),
            Detail = value,
            Node = assignment
        });
    }

    // ----- items -----

    private void ResolveItem(ProjectEvaluation evaluation, string name, SemanticSymbol symbol)
    {
        var itemsFolder = evaluation.FindChild<Folder>(Strings.Items);
        var itemGroup = itemsFolder?.FindChild<AddItem>(name);
        if (itemGroup == null)
        {
            return;
        }

        symbol.Found = true;

        var values = itemGroup.Children.OfType<Item>().Select(i => i.Text).ToList();
        symbol.Value = values.Count == 1 ? values[0] : $"{values.Count} item(s)";

        foreach (var value in values.Take(MaxItemSamples))
        {
            symbol.Facts.Add(new SemanticFact(null, value));
        }

        if (values.Count > MaxItemSamples)
        {
            symbol.Facts.Add(new SemanticFact("…", $"{values.Count - MaxItemSamples} more"));
        }

        // Evaluation-time items carry no source location, so the only useful
        // jump is into the tree where the whole item list is browsable.
        symbol.Definitions.Add(new SemanticLocation
        {
            Label = $"@({name}) in {Path.GetFileName(evaluation.ProjectFile)}",
            Detail = $"{values.Count} item(s)",
            Node = itemGroup
        });
    }

    // ----- targets -----

    private void ResolveTarget(ProjectEvaluation evaluation, string name, SemanticSymbol symbol)
    {
        var index = GetTargetIndex(evaluation);
        if (index.TryGetValue(name, out var definitions))
        {
            symbol.Found = true;
            foreach (var definition in definitions)
            {
                symbol.Definitions.Add(new SemanticLocation
                {
                    FilePath = definition.FilePath,
                    Line = definition.Line,
                    Label = FormatLocation(definition.FilePath, definition.Line)
                });
            }

            if (definitions.Count > 1)
            {
                symbol.Note = $"Defined in {definitions.Count} files; MSBuild uses the one evaluated last.";
            }
        }

        foreach (var project in GetProjects(evaluation))
        {
            foreach (var target in project.Children.OfType<Target>())
            {
                if (!string.Equals(target.Name, name, StringComparison.OrdinalIgnoreCase))
                {
                    continue;
                }

                symbol.Found = true;

                // Deliberately no FilePath: an execution's destination is the
                // build tree, not the source line the definition already covers.
                symbol.Executions.Add(new SemanticLocation
                {
                    Label = DescribeExecution(target),
                    Detail = DescribeExecutionDetail(target),
                    Node = target
                });
            }
        }

        symbol.Value = symbol.Executions.Count == 0
            ? (symbol.Found ? "never ran in this project" : null)
            : (symbol.Executions.Count == 1 ? "ran once" : $"ran {symbol.Executions.Count} times");
    }

    private static string DescribeExecution(Target target)
    {
        var text = new StringBuilder(target.Name ?? string.Empty);
        if (target.Skipped)
        {
            text.Append(" (skipped)");
        }
        else if (target.Duration != default)
        {
            text.Append(" (").Append(TextUtilities.DisplayDuration(target.Duration)).Append(')');
        }

        return text.ToString();
    }

    private static string DescribeExecutionDetail(Target target)
    {
        string project = target.Project?.ProjectFile;
        return string.IsNullOrEmpty(project) ? target.DependsOnTargets : Path.GetFileName(project);
    }

    private Dictionary<string, List<SemanticTargetDefinition>> GetTargetIndex(ProjectEvaluation evaluation)
    {
        lock (sync)
        {
            if (targetsByEvaluation.TryGetValue(evaluation.Id, out var cached))
            {
                return cached;
            }
        }

        var index = new Dictionary<string, List<SemanticTargetDefinition>>(StringComparer.OrdinalIgnoreCase);

        void Add(string filePath)
        {
            foreach (var definition in GetTargetsInFile(filePath))
            {
                if (!index.TryGetValue(definition.Name, out var list))
                {
                    list = new List<SemanticTargetDefinition>();
                    index[definition.Name] = list;
                }

                list.Add(definition);
            }
        }

        Add(evaluation.ProjectFile);
        foreach (var import in evaluation.GetAllImportsTransitive())
        {
            Add(import.ImportedProjectFilePath);
        }

        lock (sync)
        {
            targetsByEvaluation[evaluation.Id] = index;
        }

        return index;
    }

    /// <summary>
    /// All <c>&lt;Target Name="..."&gt;</c> elements in one file. Cached per
    /// file because the same .targets is in hundreds of import closures.
    /// </summary>
    private IReadOnlyList<SemanticTargetDefinition> GetTargetsInFile(string filePath)
    {
        if (string.IsNullOrEmpty(filePath))
        {
            return Array.Empty<SemanticTargetDefinition>();
        }

        lock (sync)
        {
            if (targetsByFile.TryGetValue(filePath, out var cached))
            {
                return cached;
            }
        }

        IReadOnlyList<SemanticTargetDefinition> result = Array.Empty<SemanticTargetDefinition>();
        var sourceText = sourceFileResolver.GetSourceFileText(filePath);
        if (sourceText != null)
        {
            var definitions = new List<SemanticTargetDefinition>();
            var lines = sourceText.Lines;
            foreach (Match match in TargetDefinitionRegex.Matches(sourceText.Text))
            {
                definitions.Add(new SemanticTargetDefinition
                {
                    Name = match.Groups["name"].Value,
                    FilePath = filePath,
                    Line = GetLineNumber(lines, match.Index)
                });
            }

            result = definitions;
        }

        lock (sync)
        {
            targetsByFile[filePath] = result;
        }

        return result;
    }

    // ----- evaluation index -----

    private IReadOnlyList<ProjectEvaluation> GetEvaluationsForFile(string filePath)
    {
        EnsureEvaluationIndex();
        if (evaluationsByFile.TryGetValue(filePath, out var evaluations))
        {
            return evaluations;
        }

        return Array.Empty<ProjectEvaluation>();
    }

    private void EnsureEvaluationIndex()
    {
        lock (sync)
        {
            if (evaluationsByFile != null)
            {
                return;
            }

            var index = new Dictionary<string, List<ProjectEvaluation>>(StringComparer.OrdinalIgnoreCase);

            void Add(string filePath, ProjectEvaluation evaluation)
            {
                if (string.IsNullOrEmpty(filePath))
                {
                    return;
                }

                if (!index.TryGetValue(filePath, out var list))
                {
                    list = new List<ProjectEvaluation>();
                    index[filePath] = list;
                }

                // The project file is also its own first "import"; keep one
                // entry per evaluation.
                if (list.Count == 0 || list[list.Count - 1] != evaluation)
                {
                    list.Add(evaluation);
                }
            }

            foreach (var evaluation in GetAllEvaluations())
            {
                Add(evaluation.ProjectFile, evaluation);
                foreach (var import in evaluation.GetAllImportsTransitive())
                {
                    Add(import.ImportedProjectFilePath, evaluation);
                }
            }

            // Evaluations whose project file *is* the viewed file come first;
            // otherwise keep build order, so the default context is the
            // earliest evaluation that pulled the file in.
            foreach (var pair in index)
            {
                pair.Value.Sort((left, right) =>
                {
                    bool leftIsProject = string.Equals(left.ProjectFile, pair.Key, StringComparison.OrdinalIgnoreCase);
                    bool rightIsProject = string.Equals(right.ProjectFile, pair.Key, StringComparison.OrdinalIgnoreCase);
                    if (leftIsProject != rightIsProject)
                    {
                        return leftIsProject ? -1 : 1;
                    }

                    return left.Id.CompareTo(right.Id);
                });
            }

            evaluationsByFile = index;
        }
    }

    private IEnumerable<ProjectEvaluation> GetAllEvaluations()
    {
        var folder = build.EvaluationFolder;
        if (folder == null)
        {
            return Array.Empty<ProjectEvaluation>();
        }

        return folder.Children.OfType<ProjectEvaluation>();
    }

    private IReadOnlyList<Project> GetProjects(ProjectEvaluation evaluation)
    {
        lock (sync)
        {
            if (projectsByEvaluationId == null)
            {
                var index = new Dictionary<int, List<Project>>();
                build.VisitAllChildren<Project>(project =>
                {
                    if (!index.TryGetValue(project.EvaluationId, out var list))
                    {
                        list = new List<Project>();
                        index[project.EvaluationId] = list;
                    }

                    list.Add(project);
                });

                projectsByEvaluationId = index;
            }

            if (projectsByEvaluationId.TryGetValue(evaluation.Id, out var projects))
            {
                return projects;
            }
        }

        return Array.Empty<Project>();
    }

    // ----- helpers -----

    private SemanticEvaluationContext CreateContext(ProjectEvaluation evaluation, string filePath)
    {
        return new SemanticEvaluationContext
        {
            Evaluation = evaluation,
            ProjectFile = evaluation.ProjectFile,
            Label = BuildLabel(evaluation),
            IsProjectFile = string.Equals(evaluation.ProjectFile, filePath, StringComparison.OrdinalIgnoreCase)
        };
    }

    private static string BuildLabel(ProjectEvaluation evaluation)
    {
        string name = string.IsNullOrEmpty(evaluation.ProjectFile)
            ? (evaluation.Name ?? "(unknown project)")
            : Path.GetFileName(evaluation.ProjectFile);

        var dimensions = new List<string>(3);
        if (!string.IsNullOrEmpty(evaluation.TargetFramework))
        {
            dimensions.Add(evaluation.TargetFramework);
        }

        if (!string.IsNullOrEmpty(evaluation.Configuration))
        {
            dimensions.Add(evaluation.Configuration);
        }

        if (!string.IsNullOrEmpty(evaluation.Platform))
        {
            dimensions.Add(evaluation.Platform);
        }

        return dimensions.Count == 0 ? name : $"{name} ({string.Join(", ", dimensions)})";
    }

    /// <summary>
    /// A project evaluated several times over the same dimensions (restore
    /// pass vs. build pass, say) produces identical labels. Append the global
    /// properties that actually differ, so the picker offers real choices.
    /// </summary>
    private static void DisambiguateLabels(List<SemanticEvaluationContext> contexts)
    {
        foreach (var group in contexts.GroupBy(c => c.Label, StringComparer.Ordinal))
        {
            var collisions = group.ToList();
            if (collisions.Count < 2)
            {
                continue;
            }

            var globals = collisions.ToDictionary(c => c, c => GetGlobalProperties(c.Evaluation));

            var distinguishing = globals.Values
                .SelectMany(g => g.Keys)
                .Distinct(StringComparer.OrdinalIgnoreCase)
                .Where(key => globals.Values
                    .Select(g => g.TryGetValue(key, out var value) ? value : null)
                    .Distinct(StringComparer.Ordinal)
                    .Count() > 1)
                .OrderBy(key => key, StringComparer.OrdinalIgnoreCase)
                .Take(2)
                .ToList();

            foreach (var context in collisions)
            {
                var parts = distinguishing
                    .Select(key => globals[context].TryGetValue(key, out var value)
                        ? $"{key}={value}"
                        : $"no {key}")
                    .ToList();

                context.Label += parts.Count > 0
                    ? $" [{string.Join(", ", parts)}]"
                    : $" #{context.Evaluation.Id}";
            }
        }
    }

    private static Dictionary<string, string> GetGlobalProperties(ProjectEvaluation evaluation)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        var global = evaluation.FindChild<Folder>(Strings.Properties)?.FindChild<Folder>(Strings.Global);
        if (global == null)
        {
            return result;
        }

        foreach (var child in global.Children)
        {
            if (child is Property property && property.Name != null)
            {
                result[property.Name] = property.Value;
            }
        }

        return result;
    }

    private static string FormatLocation(string filePath, int line)
    {
        if (string.IsNullOrEmpty(filePath))
        {
            return null;
        }

        string name = Path.GetFileName(filePath);
        return line > 0 ? $"{name}:{line}" : name;
    }

    private static string NormalizeValue(string value)
    {
        return value == null ? null : value.NormalizePropertyValue();
    }

    private static int GetLineNumber(IReadOnlyList<Span> lines, int position)
    {
        int low = 0;
        int high = lines.Count - 1;
        while (low <= high)
        {
            int mid = low + ((high - low) / 2);
            var span = lines[mid];
            if (position < span.Start)
            {
                high = mid - 1;
            }
            else if (position >= span.End)
            {
                low = mid + 1;
            }
            else
            {
                return mid + 1;
            }
        }

        return Math.Max(1, Math.Min(lines.Count, low + 1));
    }
}
