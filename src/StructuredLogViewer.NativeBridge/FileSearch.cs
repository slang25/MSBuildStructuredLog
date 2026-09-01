using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using Microsoft.Build.Logging.StructuredLogger;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Operations over the text files embedded in the binlog (the
/// .buildsources.zip payload exposed via <c>Build.SourceFiles</c>).
/// </summary>
internal static class FileSearch
{
    public const int DefaultMaxMatches = 500;
    public const int MaxAllowedMatches = 5000;
    private const int MaxMatchesPerFile = 50;

    private static IReadOnlyDictionary<string, SourceText> GetFiles(BridgeSession session)
    {
        var resolver = session.SourceFileResolver?.ArchiveFile;
        return resolver?.Files ?? new Dictionary<string, SourceText>(0);
    }

    public static FileListDto ListFiles(BridgeSession session)
    {
        var files = GetFiles(session);
        var entries = files
            .OrderBy(kvp => kvp.Key, StringComparer.OrdinalIgnoreCase)
            .Select(kvp => new FileEntryDto
            {
                Path = kvp.Key,
                Lines = kvp.Value?.Lines.Count ?? 0,
                Length = kvp.Value?.Text?.Length ?? 0
            })
            .ToList();

        return new FileListDto { Total = entries.Count, Files = entries };
    }

    public static string ReadFile(BridgeSession session, string path)
    {
        var files = GetFiles(session);
        if (files.Count == 0)
        {
            throw new FileNotFoundException("No embedded files in this binlog.");
        }

        string normalized = ArchiveFile.CalculateArchivePath(path);
        if (files.TryGetValue(normalized, out var text))
        {
            return text?.Text ?? string.Empty;
        }

        // Tolerate slash-direction differences from the exact list_files paths.
        foreach (var kvp in files)
        {
            if (string.Equals(
                kvp.Key.Replace('\\', '/'),
                normalized.Replace('\\', '/'),
                StringComparison.OrdinalIgnoreCase))
            {
                return kvp.Value?.Text ?? string.Empty;
            }
        }

        throw new FileNotFoundException($"No embedded file matches '{path}'.");
    }

    public static FileSearchResponseDto SearchFiles(
        BridgeSession session,
        string term,
        int maxResults,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrEmpty(term))
        {
            throw new ArgumentException("Search term is empty.", nameof(term));
        }

        int cap = Math.Clamp(maxResults <= 0 ? DefaultMaxMatches : maxResults, 1, MaxAllowedMatches);

        var ordered = GetFiles(session)
            .OrderBy(kvp => kvp.Key, StringComparer.OrdinalIgnoreCase);

        var response = new FileSearchResponseDto
        {
            Query = term,
            Files = new List<FileMatchesDto>()
        };

        int total = 0;
        foreach (var kvp in ordered)
        {
            cancellationToken.ThrowIfCancellationRequested();

            var text = kvp.Value;
            if (text == null)
            {
                continue;
            }

            List<FileMatchDto> matches = null;
            int lineCount = text.Lines.Count;
            for (int i = 0; i < lineCount && total < cap; i++)
            {
                string lineText = text.GetLineText(i);
                var spans = FindSpans(lineText, term);
                if (spans == null)
                {
                    continue;
                }

                matches ??= new List<FileMatchDto>();
                matches.Add(new FileMatchDto
                {
                    Line = i + 1,
                    Text = lineText,
                    Spans = spans
                });
                total++;

                if (matches.Count >= MaxMatchesPerFile)
                {
                    break;
                }
            }

            if (matches != null)
            {
                response.Files.Add(new FileMatchesDto { Path = kvp.Key, Matches = matches });
            }

            if (total >= cap)
            {
                response.Overflow = true;
                break;
            }
        }

        response.TotalMatches = total;
        return response;
    }

    private static List<FileMatchSpanDto> FindSpans(string line, string term)
    {
        int first = line.IndexOf(term, StringComparison.OrdinalIgnoreCase);
        if (first < 0)
        {
            return null;
        }

        var spans = new List<FileMatchSpanDto>();
        int index = first;
        while (index >= 0)
        {
            spans.Add(new FileMatchSpanDto { Start = index, Length = term.Length });
            index = line.IndexOf(term, index + term.Length, StringComparison.OrdinalIgnoreCase);
        }

        return spans;
    }
}
