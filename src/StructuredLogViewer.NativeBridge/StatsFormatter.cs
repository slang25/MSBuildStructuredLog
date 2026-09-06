using System.Collections.Generic;
using System.Threading;
using Microsoft.Build.Logging.StructuredLogger;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Computes the binlog record statistics (a fresh pass over the file on
/// disk, mirroring the viewer's Statistics dialog) and shapes them for JSON.
/// </summary>
internal static class StatsFormatter
{
    public static StatsDto Calculate(string path, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        // BinlogStats has no cancellation hooks, so this is the last point at
        // which mslog_cancel can stop the call — mslog.h says as much. The
        // caller drops its session lease first, so a close is not held up.
        var stats = BinlogStats.Calculate(path);

        return new StatsDto
        {
            FileSize = stats.FileSize,
            UncompressedStreamSize = stats.UncompressedStreamSize,
            RecordCount = stats.RecordCount,
            FileFormatVersion = stats.FileFormatVersion,
            StringCount = stats.StringCount,
            StringTotalSize = stats.StringTotalSize,
            StringLargest = stats.StringLargest,
            NameValueListCount = stats.NameValueListCount,
            NameValueListTotalSize = stats.NameValueListTotalSize,
            NameValueListLargest = stats.NameValueListLargest,
            BlobCount = stats.BlobCount,
            BlobTotalSize = stats.BlobTotalSize,
            BlobLargest = stats.BlobLargest,
            Records = Serialize(stats.CategorizedRecords)
        };
    }

    private static StatsRecordDto Serialize(BinlogStats.RecordsByType records)
    {
        if (records == null)
        {
            return null;
        }

        var dto = new StatsRecordDto
        {
            Type = records.Type ?? "Records",
            TotalLength = records.TotalLength,
            Count = records.Count,
            Largest = records.Largest
        };

        if (records.CategorizedRecords is { Count: > 0 } children)
        {
            dto.Children = new List<StatsRecordDto>(children.Count);
            foreach (var child in children)
            {
                dto.Children.Add(Serialize(child));
            }
        }

        return dto;
    }
}
