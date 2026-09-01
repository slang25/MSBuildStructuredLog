import Foundation

/// Stable-identity reference object for one tree row. NSOutlineView keys
/// expansion/selection state off object identity, so each (parent, index)
/// slot gets exactly one NodeRef for the store's lifetime: created as a
/// placeholder when first asked for, filled in place when its page
/// arrives.
@MainActor
public final class NodeRef: Identifiable {
    public internal(set) var summary: NodeSummary
    public internal(set) var isPlaceholder: Bool
    public internal(set) weak var parent: NodeRef?
    public internal(set) var indexInParent: Int
    public internal(set) var sortMode: ChildSortMode = .natural

    /// Sparse child slots; count matches summary.childCount.
    var childSlots: [NodeRef?] = []

    /// Generation counter bumped when children are invalidated (re-sort),
    /// so stale page fetches don't write into fresh slots.
    var childGeneration = 0

    init(summary: NodeSummary, parent: NodeRef?, indexInParent: Int, isPlaceholder: Bool = false) {
        self.summary = summary
        self.parent = parent
        self.indexInParent = indexInParent
        self.isPlaceholder = isPlaceholder
    }

    public nonisolated var id: ObjectIdentifier { ObjectIdentifier(self) }

    public var childCount: Int { summary.hasChildren ? summary.childCount : 0 }
    public var hasChildren: Bool { summary.hasChildren }
    public var childSlotsCount: Int { childSlots.count }

    /// The child NodeRef at `index` if a real (non-placeholder) one exists.
    public func materializedChild(at index: Int) -> NodeRef? {
        guard childSlots.indices.contains(index), let slot = childSlots[index], !slot.isPlaceholder else {
            return nil
        }
        return slot
    }
}

/// Pull-based tree model over BinlogEngine.children: pages of ~512 child
/// summaries fetched on demand, exactly mapping NSOutlineView's data-source
/// callbacks. All access on the main actor.
@MainActor
public final class NodeStore {
    public let engine: BinlogEngine
    public let pageSize: Int
    public private(set) var root: NodeRef

    /// Called when a page of children materialized; the range is the child
    /// index range within the parent that changed.
    public var onChildrenUpdated: ((NodeRef, Range<Int>) -> Void)?

    private var byNodeId: [String: NodeRef] = [:]

    private struct PageKey: Hashable {
        let node: ObjectIdentifier
        let page: Int
        let generation: Int
    }

    private var inflightPages: [PageKey: Task<Void, Error>] = [:]

    public init(engine: BinlogEngine, rootSummary: NodeSummary, pageSize: Int = 512) {
        self.engine = engine
        self.pageSize = pageSize
        self.root = NodeRef(summary: rootSummary, parent: nil, indexInParent: 0)
        byNodeId[rootSummary.id] = root
    }

    public func node(withId id: String) -> NodeRef? { byNodeId[id] }

    public func childCount(of ref: NodeRef) -> Int { ref.childCount }

    /// Synchronous accessor for the outline data source. Always returns a
    /// stable object; kicks off a background page fetch when the slot is
    /// still a placeholder.
    public func child(of ref: NodeRef, at index: Int) -> NodeRef {
        ensureSlots(ref)
        precondition(index >= 0 && index < ref.childSlots.count, "child index out of range")

        if let existing = ref.childSlots[index] {
            if existing.isPlaceholder {
                schedulePage(for: ref, containing: index)
            }
            return existing
        }

        let placeholder = NodeRef(
            summary: NodeSummary(id: placeholderId(ref, index), kind: "", title: ""),
            parent: ref,
            indexInParent: index,
            isPlaceholder: true)
        ref.childSlots[index] = placeholder
        schedulePage(for: ref, containing: index)
        return placeholder
    }

    /// Whether the given child slot has real data (used to decide between
    /// a real cell and a dimmed loading placeholder).
    public func isLoaded(_ ref: NodeRef, at index: Int) -> Bool {
        guard index < ref.childSlots.count, let slot = ref.childSlots[index] else { return false }
        return !slot.isPlaceholder
    }

    /// Awaitable page load (used by reveal and tests).
    public func loadPage(of ref: NodeRef, containing index: Int) async throws {
        try await pageTask(for: ref, page: index / pageSize).value
    }

    /// Changes the sort mode for one node's children and refetches.
    public func setSortMode(_ mode: ChildSortMode, for ref: NodeRef) {
        guard ref.sortMode != mode else { return }
        ref.sortMode = mode
        invalidateChildren(of: ref)
    }

    public func invalidateChildren(of ref: NodeRef) {
        ref.childGeneration += 1
        for slot in ref.childSlots {
            if let slot, !slot.isPlaceholder {
                removeFromIndex(slot)
            }
        }
        ref.childSlots = Array(repeating: nil, count: ref.childCount)
        onChildrenUpdated?(ref, 0..<ref.childCount)
    }

    /// Resolves the full chain of NodeRefs from the root to the node with
    /// the given engine id, loading every page along the way. Ancestors
    /// with a non-natural sort are reset to natural (their display order
    /// must match the engine's childIndex values).
    public func reveal(id: String) async throws -> [NodeRef] {
        if let known = byNodeId[id] {
            var chain: [NodeRef] = []
            var cursor: NodeRef? = known
            while let node = cursor {
                chain.append(node)
                cursor = node.parent
            }
            return chain.reversed()
        }

        let ancestors = try await engine.ancestors(of: id)
        guard let first = ancestors.chain.first, first.id == root.summary.id else {
            throw EngineError.badNodeId("Ancestor chain does not start at the open build's root.")
        }

        var chain: [NodeRef] = [root]
        var current = root
        for element in ancestors.chain.dropFirst() {
            guard let index = element.childIndex else {
                throw EngineError.badNodeId("Ancestor '\(element.id)' has no child index.")
            }

            if current.sortMode != .natural {
                setSortMode(.natural, for: current)
            }

            ensureSlots(current)
            guard index < current.childSlots.count else {
                throw EngineError.badNodeId("Ancestor index \(index) out of range in '\(current.summary.id)'.")
            }

            try await loadPage(of: current, containing: index)
            guard let child = current.childSlots[index], !child.isPlaceholder else {
                throw EngineError.badNodeId("Failed to load ancestor '\(element.id)'.")
            }

            chain.append(child)
            current = child
        }

        return chain
    }

    // MARK: - internals

    private func placeholderId(_ ref: NodeRef, _ index: Int) -> String {
        "pending:\(ref.summary.id):\(index)"
    }

    private func ensureSlots(_ ref: NodeRef) {
        if ref.childSlots.count != ref.childCount {
            ref.childSlots = Array(repeating: nil, count: ref.childCount)
        }
    }

    private func schedulePage(for ref: NodeRef, containing index: Int) {
        _ = pageTask(for: ref, page: index / pageSize)
    }

    private func pageTask(for ref: NodeRef, page: Int) -> Task<Void, Error> {
        let key = PageKey(node: ObjectIdentifier(ref), page: page, generation: ref.childGeneration)
        if let existing = inflightPages[key] {
            return existing
        }

        let offset = page * pageSize
        let count = pageSize
        let generation = ref.childGeneration
        let sortMode = ref.sortMode
        let parentId = ref.summary.id

        let task = Task { [weak self, weak ref] in
            guard let self, let ref else { return }
            defer { self.inflightPages[key] = nil }
            let response = try await self.engine.children(
                of: parentId, offset: offset, count: count, sortMode: sortMode)
            guard ref.childGeneration == generation else { return }
            self.apply(page: response, to: ref, offset: offset)
        }

        inflightPages[key] = task
        return task
    }

    private func apply(page: ChildrenPage, to ref: NodeRef, offset: Int) {
        ensureSlots(ref)
        guard offset < ref.childSlots.count else { return }

        let end = min(offset + page.children.count, ref.childSlots.count)
        for i in offset..<end {
            let summary = page.children[i - offset]
            if let slot = ref.childSlots[i] {
                slot.summary = summary
                slot.isPlaceholder = false
            } else {
                ref.childSlots[i] = NodeRef(summary: summary, parent: ref, indexInParent: i)
            }

            if ref.sortMode == .natural {
                byNodeId[summary.id] = ref.childSlots[i]
            }
        }

        if offset < end {
            onChildrenUpdated?(ref, offset..<end)
        }
    }

    private func removeFromIndex(_ ref: NodeRef) {
        if byNodeId[ref.summary.id] === ref {
            byNodeId.removeValue(forKey: ref.summary.id)
        }
        for slot in ref.childSlots {
            if let slot, !slot.isPlaceholder {
                removeFromIndex(slot)
            }
        }
    }
}
