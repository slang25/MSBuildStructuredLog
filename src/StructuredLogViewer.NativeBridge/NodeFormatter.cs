using System;
using System.Collections.Generic;
using System.Globalization;
using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Maps engine nodes to <see cref="NodeSummaryDto"/> /
/// <see cref="NodeDetailsDto"/>. Titles match what the viewer renders in a
/// tree row (<see cref="ProxyNode.GetNodeText"/>); huge values are
/// shortened — the full text is available via node details / subtree text.
/// </summary>
internal static class NodeFormatter
{
    private const int MaxTitleChars = 2000;

    public static NodeSummaryDto CreateSummary(BridgeSession session, BaseNode node)
    {
        // GetNodeText yields "Build Build" for the root (TypeName + Name);
        // show the viewer-style success line instead. Package rows drop the
        // type prefix too and carry the version, like the viewer template.
        string title = node switch
        {
            Build build => build.Succeeded ? "Build succeeded" : "Build failed",
            Package package => package.ToString(),
            _ => ProxyNode.GetNodeText(node) ?? node.Title ?? string.Empty
        };

        var dto = new NodeSummaryDto
        {
            Id = BinlogMcp.NodeId.Get(node),
            Kind = node.TypeName ?? node.GetType().Name,
            Title = TextUtilities.ShortenValue(title, "...", MaxTitleChars),
            State = GetState(node)
        };

        if (node is NameValueNode nameValue)
        {
            dto.Name = nameValue.Name;
            dto.Value = TextUtilities.ShortenValue(nameValue.Value ?? string.Empty, "...", MaxTitleChars);
        }
        else if (node is NamedNode named)
        {
            dto.Name = named.Name;
        }

        if (node is TreeNode tree && tree.HasChildren)
        {
            dto.HasChildren = true;
            dto.ChildCount = tree.Children.Count;
        }

        if (node is IHasRelevance relevance && relevance.IsLowRelevance)
        {
            dto.IsLowRelevance = true;
        }

        if (node is TimedNode timed && timed.Duration != default)
        {
            dto.DurationMs = timed.Duration.TotalMilliseconds;
        }

        dto.HasSource = TryGetSourceLocation(node, out _, out _);

        if (node is IPreprocessable preprocessable &&
            session.PreprocessedFileManager.CanPreprocess(preprocessable))
        {
            dto.CanPreprocess = true;
        }

        dto.Props = GetProps(node);
        return dto;
    }

    public static NodeDetailsDto CreateDetails(BridgeSession session, BaseNode node)
    {
        var details = new NodeDetailsDto
        {
            Node = CreateSummary(session, node),
            ParentId = node.Parent is BaseNode parent ? BinlogMcp.NodeId.Get(parent) : null,
            FullText = node.GetFullText() ?? node.Title ?? string.Empty
        };

        if (node is TimedNode timed)
        {
            details.StartTime = timed.StartTime.ToString("o", CultureInfo.InvariantCulture);
            details.EndTime = timed.EndTime.ToString("o", CultureInfo.InvariantCulture);
        }

        if (TryGetSourceLocation(node, out string file, out int? line))
        {
            details.SourceFile = file;
            details.SourceLine = line;
        }

        return details;
    }

    public static bool TryGetSourceLocation(BaseNode node, out string filePath, out int? line)
    {
        filePath = null;
        line = null;

        if (node is AbstractDiagnostic diagnostic)
        {
            filePath = string.IsNullOrEmpty(diagnostic.File) ? diagnostic.ProjectFile : diagnostic.File;
            if (diagnostic.LineNumber > 0)
            {
                line = diagnostic.LineNumber;
            }

            return !string.IsNullOrEmpty(filePath);
        }

        if (node is IHasSourceFile sourceFile && !string.IsNullOrEmpty(sourceFile.SourceFilePath))
        {
            filePath = sourceFile.SourceFilePath;
            if (node is IHasLineNumber lineNumber && lineNumber.LineNumber is int value && value > 0)
            {
                line = value;
            }

            return true;
        }

        return false;
    }

    private static string GetState(BaseNode node)
    {
        return node switch
        {
            Build build => build.Succeeded ? "succeeded" : "failed",
            Target { Skipped: true } => "skipped",
            Target target => target.Succeeded ? "succeeded" : "failed",
            _ => "none"
        };
    }

    private static Dictionary<string, string> GetProps(BaseNode node)
    {
        Dictionary<string, string> props = null;

        void Add(string key, string value)
        {
            if (!string.IsNullOrEmpty(value))
            {
                props ??= new Dictionary<string, string>();
                props[key] = value;
            }
        }

        switch (node)
        {
            case Project project:
                Add("projectFile", project.ProjectFile);
                Add("targetFramework", project.TargetFramework);
                Add("configuration", project.Configuration);
                Add("platform", project.Platform);
                Add("extension", project.ProjectFileExtension);
                break;
            case ProjectEvaluation evaluation:
                Add("projectFile", evaluation.ProjectFile);
                Add("extension", evaluation.ProjectFileExtension);
                break;
            case Task task:
                Add("fromAssembly", task.FromAssembly);
                break;
            case AbstractDiagnostic diagnostic:
                Add("code", diagnostic.Code);
                Add("file", diagnostic.File);
                if (diagnostic.LineNumber > 0)
                {
                    Add("line", diagnostic.LineNumber.ToString(CultureInfo.InvariantCulture));
                }

                if (diagnostic.ColumnNumber > 0)
                {
                    Add("column", diagnostic.ColumnNumber.ToString(CultureInfo.InvariantCulture));
                }

                Add("projectFile", diagnostic.ProjectFile);
                break;
            case Import import:
                Add("projectFilePath", import.ProjectFilePath);
                Add("importedProjectFilePath", import.ImportedProjectFilePath);
                Add("line", import.Line.ToString(CultureInfo.InvariantCulture));
                Add("column", import.Column.ToString(CultureInfo.InvariantCulture));
                break;
            case NoImport noImport:
                Add("projectFilePath", noImport.ProjectFilePath);
                Add("importedFileSpec", noImport.ImportedFileSpec);
                Add("reason", noImport.Reason);
                break;
            case Package package:
                Add("version", package.Version);
                Add("versionSpec", package.VersionSpec);
                break;
            case FileCopy fileCopy:
                Add("copyKind", fileCopy.Kind);
                break;
            case Target target:
                Add("dependsOnTargets", target.DependsOnTargets);
                if (!string.IsNullOrEmpty(target.ParentTarget))
                {
                    // The navigational link the viewers render after the
                    // target name (↑ AfterTargets / ↓ BeforeTargets /
                    // → DependsOn). The destination node resolves lazily
                    // via mslog_target_parent when clicked.
                    Add("parentTarget", target.ParentTarget);
                    Add("parentTargetText", target.ParentTargetText?.Trim());
                    Add("parentTargetTooltip", target.ParentTargetTooltip);
                }

                break;
        }

        return props;
    }
}
