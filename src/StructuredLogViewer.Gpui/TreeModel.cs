using Microsoft.Build.Logging.StructuredLogger;
using StructuredLogViewer.NativeBridge;

namespace StructuredLogViewer.Gpui;

/// <summary>
/// One visible row of the build tree. Rows carry a process-unique id so a
/// list row's <c>ItemId</c> and click payloads survive splices.
/// </summary>
public sealed class TreeRow
{
    private static ulong nextId = 1;

    public TreeRow(BaseNode node, int depth)
    {
        Id = nextId++;
        Node = node;
        Depth = depth;
    }

    public ulong Id { get; }
    public BaseNode Node { get; }
    public int Depth { get; }
    public bool Expanded { get; set; }
    public bool HasChildren => Node is TreeNode { HasChildren: true };

    private NodeSummaryDto? summary;

    /// <summary>Formatted lazily; the same input always yields the same output.</summary>
    public NodeSummaryDto Summary(BridgeSession bridge) => summary ??= NodeFormatter.CreateSummary(bridge, Node);
}

/// <summary>
/// The build tree flattened to what the virtual list shows: expanded
/// nodes contribute their children as following rows. Mutations report
/// the splice they made so the native list can keep its measurements.
/// The engine's tree is frozen after load, so rows only ever come from
/// walking it — nothing here is cached beyond formatted summaries.
/// </summary>
public sealed class TreeModel
{
    private readonly BridgeSession bridge;
    private readonly List<TreeRow> rows = new();

    public TreeModel(BridgeSession bridge)
    {
        this.bridge = bridge;
        rows.Add(new TreeRow(bridge.Build, 0));
        Expand(0);
    }

    public IReadOnlyList<TreeRow> Rows => rows;
    public int Count => rows.Count;
    public TreeRow this[int index] => rows[index];

    public int IndexOf(ulong rowId)
    {
        for (int i = 0; i < rows.Count; i++)
        {
            if (rows[i].Id == rowId)
            {
                return i;
            }
        }

        return -1;
    }

    public int IndexOf(BaseNode node)
    {
        for (int i = 0; i < rows.Count; i++)
        {
            if (ReferenceEquals(rows[i].Node, node))
            {
                return i;
            }
        }

        return -1;
    }

    /// <summary>Returns (start, removed, inserted) for the list splice, or null if nothing changed.</summary>
    public (int Start, int Removed, int Inserted)? Toggle(int index)
    {
        var row = rows[index];
        return row.Expanded ? Collapse(index) : Expand(index);
    }

    public (int Start, int Removed, int Inserted)? Expand(int index)
    {
        var row = rows[index];
        if (row.Expanded || !row.HasChildren)
        {
            return null;
        }

        var children = bridge.GetSortedChildren(row.Node, 0);
        var inserted = new List<TreeRow>(children.Count);
        foreach (var child in children)
        {
            inserted.Add(new TreeRow(child, row.Depth + 1));
        }

        rows.InsertRange(index + 1, inserted);
        row.Expanded = true;
        return (index + 1, 0, inserted.Count);
    }

    public (int Start, int Removed, int Inserted)? Collapse(int index)
    {
        var row = rows[index];
        if (!row.Expanded)
        {
            return null;
        }

        int end = index + 1;
        while (end < rows.Count && rows[end].Depth > row.Depth)
        {
            end++;
        }

        int removed = end - (index + 1);
        rows.RemoveRange(index + 1, removed);
        row.Expanded = false;
        return (index + 1, removed, 0);
    }

    /// <summary>Index of the row's parent row, or -1 for the root.</summary>
    public int ParentIndex(int index)
    {
        int depth = rows[index].Depth;
        for (int i = index - 1; i >= 0; i--)
        {
            if (rows[i].Depth < depth)
            {
                return i;
            }
        }

        return -1;
    }

    /// <summary>
    /// Expands every ancestor of <paramref name="node"/> and returns its
    /// row index. Callers should reset the native list afterwards: a reveal
    /// may splice at several places at once.
    /// </summary>
    public int Reveal(BaseNode node)
    {
        var chain = new List<BaseNode>();
        for (BaseNode? current = node; current is not null; current = current.Parent)
        {
            chain.Add(current);
        }

        chain.Reverse();
        if (!ReferenceEquals(chain[0], bridge.Build))
        {
            return -1;
        }

        int index = 0;
        for (int i = 1; i < chain.Count; i++)
        {
            Expand(index);
            int next = -1;
            for (int j = index + 1; j < rows.Count && rows[j].Depth > rows[index].Depth; j++)
            {
                if (ReferenceEquals(rows[j].Node, chain[i]))
                {
                    next = j;
                    break;
                }
            }

            if (next < 0)
            {
                return -1;
            }

            index = next;
        }

        return index;
    }
}
