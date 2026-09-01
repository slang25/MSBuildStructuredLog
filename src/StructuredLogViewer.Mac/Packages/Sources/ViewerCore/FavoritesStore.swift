import Foundation
import Observation

/// Session-only favorites (parity with the Avalonia viewer). Behind a
/// small mutable store so persistence can be added later without touching
/// callers.
@MainActor
@Observable
public final class FavoritesStore {
    public private(set) var favorites: [NodeSummary] = []

    public init() {}

    public func isFavorite(_ id: String) -> Bool {
        favorites.contains { $0.id == id }
    }

    public func toggle(_ node: NodeSummary) {
        if let index = favorites.firstIndex(where: { $0.id == node.id }) {
            favorites.remove(at: index)
        } else {
            favorites.append(node)
        }
    }

    public func remove(id: String) {
        favorites.removeAll { $0.id == id }
    }

    public func clear() {
        favorites = []
    }
}
