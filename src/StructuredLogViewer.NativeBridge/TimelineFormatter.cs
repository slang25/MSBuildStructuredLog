using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Threading;
using Microsoft.Build.Logging.StructuredLogger;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// Builds the per-MSBuild-node timeline (the WPF viewer's Timeline /
/// Tracing tab). Port of <c>StructuredLogViewer.Core/Timeline</c>'s
/// population rules — kept here rather than referencing the whole Core
/// project just for ~40 lines: one lane per <see cref="TimedNode.NodeId"/>,
/// every timed node except the Build root and MSBuild/CallTarget tasks
/// (their time is double-counted by their nested builds), nesting depth
/// from the parent chain. The C++ analyzer pass is not ported.
/// </summary>
internal static class TimelineFormatter
{
    public static TimelineDto Build(BridgeSession session, CancellationToken cancellationToken)
    {
        var build = session.Build;
        var lanes = new Dictionary<int, List<(TimedNode Node, int Indent)>>();

        build.VisitAllChildren<TimedNode>(node =>
        {
            if (node is Build)
            {
                return;
            }

            if (node is Microsoft.Build.Logging.StructuredLogger.Task task &&
                (string.Equals(task.Name, "MSBuild", StringComparison.OrdinalIgnoreCase) ||
                 string.Equals(task.Name, "CallTarget", StringComparison.OrdinalIgnoreCase)))
            {
                return;
            }

            if (node.StartTime == default || node.EndTime == default || node.EndTime <= node.StartTime)
            {
                return;
            }

            if (!lanes.TryGetValue(node.NodeId, out var blocks))
            {
                blocks = new List<(TimedNode, int)>();
                lanes[node.NodeId] = blocks;
            }

            int indent = 0;
            var parent = node.Parent;
            while (parent != null)
            {
                indent++;
                parent = parent.Parent;
            }

            blocks.Add((node, indent));
        }, cancellationToken);

        cancellationToken.ThrowIfCancellationRequested();

        DateTime origin = DateTime.MaxValue;
        DateTime latest = DateTime.MinValue;
        foreach (var blocks in lanes.Values)
        {
            foreach (var (node, _) in blocks)
            {
                if (node.StartTime < origin)
                {
                    origin = node.StartTime;
                }

                if (node.EndTime > latest)
                {
                    latest = node.EndTime;
                }
            }
        }

        var dto = new TimelineDto
        {
            Lanes = new List<TimelineLaneDto>(lanes.Count)
        };

        if (origin == DateTime.MaxValue)
        {
            dto.StartTime = default(DateTime).ToString("o", CultureInfo.InvariantCulture);
            dto.DurationMs = 0;
            return dto;
        }

        dto.StartTime = origin.ToString("o", CultureInfo.InvariantCulture);
        dto.DurationMs = (latest - origin).TotalMilliseconds;

        foreach (var laneId in lanes.Keys.OrderBy(k => k))
        {
            cancellationToken.ThrowIfCancellationRequested();

            var source = lanes[laneId];

            // Normalize indents so each lane's outermost block sits at 0.
            int minIndent = int.MaxValue;
            foreach (var (_, indent) in source)
            {
                if (indent < minIndent)
                {
                    minIndent = indent;
                }
            }

            var lane = new TimelineLaneDto
            {
                NodeId = laneId,
                Blocks = new List<TimelineBlockDto>(source.Count)
            };

            foreach (var (node, indent) in source.OrderBy(b => b.Node.StartTime))
            {
                int level = indent - minIndent;
                if (level > lane.MaxIndent)
                {
                    lane.MaxIndent = level;
                }

                lane.Blocks.Add(new TimelineBlockDto
                {
                    Id = BinlogMcp.NodeId.Get(node),
                    Kind = node.TypeName ?? node.GetType().Name,
                    Text = node.Name,
                    Start = (node.StartTime - origin).TotalMilliseconds,
                    End = (node.EndTime - origin).TotalMilliseconds,
                    Indent = level,
                    HasError = node is Microsoft.Build.Logging.StructuredLogger.Task &&
                        node.FindFirstDescendant<Error>() != null
                });
            }

            dto.Lanes.Add(lane);
        }

        return dto;
    }
}
