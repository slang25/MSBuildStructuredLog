using System;
using System.Collections.Generic;
using Microsoft.Build.Logging.StructuredLogger;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Maps <see cref="MSBuildSemanticModel"/> results to the wire DTOs, and
/// resolves evaluation contexts to/from bridge node ids so the Swift side
/// can address an evaluation with the same id it uses everywhere else.
/// </summary>
internal static class SemanticFormatter
{
    /// <summary>Values longer than this are elided in quick info.</summary>
    private const int MaxValueChars = 2000;

    public static SemanticFileDto CreateFile(BridgeSession session, string path, string evaluationId)
    {
        ProjectEvaluation evaluation = null;
        if (!string.IsNullOrEmpty(evaluationId))
        {
            evaluation = session.ResolveNode(evaluationId) as ProjectEvaluation
                ?? throw new InvalidOperationException($"Node '{evaluationId}' is not a ProjectEvaluation.");
        }

        var semantics = session.SemanticModel.GetFileSemantics(path, evaluation);
        var resolver = session.SourceFileResolver;

        var dto = new SemanticFileDto
        {
            Path = semantics.FilePath,
            EvaluationId = semantics.Context is { } context ? BinlogMcp.NodeId.Get(context.Evaluation) : null,
            ContextsTotal = semantics.ContextsTotal,
            Contexts = new List<SemanticContextDto>(semantics.Contexts.Count),
            Imports = new List<SemanticImportDto>(semantics.Imports.Count),
            SkippedImports = new List<SemanticSkippedImportDto>(semantics.SkippedImports.Count),
            Targets = new List<SemanticTargetDefinitionDto>(semantics.Targets.Count)
        };

        foreach (var candidate in semantics.Contexts)
        {
            dto.Contexts.Add(new SemanticContextDto
            {
                EvaluationId = BinlogMcp.NodeId.Get(candidate.Evaluation),
                ProjectFile = candidate.ProjectFile,
                Label = candidate.Label,
                IsProjectFile = candidate.IsProjectFile
            });
        }

        foreach (var import in semantics.Imports)
        {
            dto.Imports.Add(new SemanticImportDto
            {
                Line = import.Line,
                Column = import.Column,
                ImportedPath = import.ImportedFilePath,
                Available = resolver.HasFile(import.ImportedFilePath)
            });
        }

        foreach (var skipped in semantics.SkippedImports)
        {
            dto.SkippedImports.Add(new SemanticSkippedImportDto
            {
                Line = skipped.Line,
                Column = skipped.Column,
                FileSpec = skipped.FileSpec,
                Reason = skipped.Reason,
                Condition = Shorten(skipped.Condition),
                EvaluatedCondition = Shorten(skipped.EvaluatedCondition)
            });
        }

        foreach (var target in semantics.Targets)
        {
            dto.Targets.Add(new SemanticTargetDefinitionDto
            {
                Name = target.Name,
                Line = target.Line
            });
        }

        return dto;
    }

    public static SemanticSymbolDto Resolve(
        BridgeSession session,
        string evaluationId,
        string kind,
        string name)
    {
        if (session.ResolveNode(evaluationId) is not ProjectEvaluation evaluation)
        {
            throw new InvalidOperationException($"Node '{evaluationId}' is not a ProjectEvaluation.");
        }

        var symbol = session.SemanticModel.Resolve(evaluation, ParseKind(kind), name);

        return new SemanticSymbolDto
        {
            Kind = kind,
            Name = symbol.Name,
            Found = symbol.Found,
            Value = Shorten(symbol.Value),
            Note = symbol.Note,
            Definitions = CreateLocations(session, symbol.Definitions),
            Executions = CreateLocations(session, symbol.Executions),
            Facts = CreateFacts(symbol.Facts)
        };
    }

    private static SemanticSymbolKind ParseKind(string kind)
    {
        switch (kind)
        {
            case "property": return SemanticSymbolKind.Property;
            case "item": return SemanticSymbolKind.Item;
            case "target": return SemanticSymbolKind.Target;
            default:
                throw new ArgumentException(
                    $"Unknown symbol kind '{kind}'. Expected property, item or target.", nameof(kind));
        }
    }

    private static List<SemanticLocationDto> CreateLocations(
        BridgeSession session,
        List<SemanticLocation> locations)
    {
        if (locations.Count == 0)
        {
            return null;
        }

        var result = new List<SemanticLocationDto>(locations.Count);
        foreach (var location in locations)
        {
            result.Add(new SemanticLocationDto
            {
                Path = location.FilePath,
                Line = location.Line,
                Label = location.Label,
                Detail = Shorten(location.Detail),
                NodeId = location.Node != null ? BinlogMcp.NodeId.Get(location.Node) : null,
                Available = location.FilePath != null && session.SourceFileResolver.HasFile(location.FilePath)
            });
        }

        return result;
    }

    private static List<SemanticFactDto> CreateFacts(List<SemanticFact> facts)
    {
        if (facts.Count == 0)
        {
            return null;
        }

        var result = new List<SemanticFactDto>(facts.Count);
        foreach (var fact in facts)
        {
            result.Add(new SemanticFactDto { Label = fact.Label, Value = Shorten(fact.Value) });
        }

        return result;
    }

    private static string Shorten(string value) =>
        value == null ? null : TextUtilities.ShortenValue(value, "...", MaxValueChars);
}
