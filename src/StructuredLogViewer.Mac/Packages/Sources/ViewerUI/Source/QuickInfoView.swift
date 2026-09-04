import SwiftUI
import ViewerCore

/// Read-only quick info for the token under the pointer: what the build
/// actually recorded for it, in the tab's evaluation context. Navigation is
/// Cmd-click; this popover only explains.
///
/// Nothing here is elided — MSBuild values are mostly paths, and a truncated
/// path is worse than useless. Overflow scrolls instead, and clicking the
/// token pins the popover so the text can be selected and copied.
struct QuickInfoView: View {
    /// Fixed, not min/ideal/max: the popover is positioned from the hosting
    /// controller's size, and a size that settles after the popover is shown
    /// leaves it detached from the token (badly so when it opens upwards).
    static let width: CGFloat = 460

    /// How many locations to list before collapsing the rest to a count.
    private static let maxLocations = 20

    let info: SemanticQuickInfo

    /// A pinned popover can be interacted with, so the hint changes.
    var isPinned = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(info.title)
                .font(.system(.body, design: .monospaced))
                .fontWeight(.semibold)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)

            switch info.body {
            case .symbol(let symbol):
                symbolBody(symbol)
            case .imports(let locations):
                importsBody(locations)
            case .skippedImports(let skipped):
                skippedImportsBody(skipped)
            case .unavailable(let reason):
                Text(reason)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if let contextLabel = info.contextLabel {
                Divider()
                Label(contextLabel, systemImage: "scope")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(12)
        .frame(width: Self.width, alignment: .leading)
    }

    // MARK: - bodies

    @ViewBuilder
    private func symbolBody(_ symbol: SemanticSymbol) -> some View {
        if !symbol.found {
            Text("Not defined in this evaluation.")
                .font(.callout)
                .foregroundStyle(.secondary)
        } else {
            if let value = symbol.value, !value.isEmpty {
                wrapped(value)
            }

            if let facts = symbol.facts, !facts.isEmpty {
                Divider()
                factsGrid(facts)
            }

            if let definitions = symbol.definitions, !definitions.isEmpty {
                locationList(
                    title: definitions.count == 1 ? "Defined in" : "Defined in \(definitions.count) places",
                    locations: definitions)
            }

            if let executions = symbol.executions, !executions.isEmpty {
                locationList(
                    title: executions.count == 1 ? "Ran once" : "Ran \(executions.count) times",
                    locations: executions)
            }

            if let note = symbol.note {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            hint
        }
    }

    @ViewBuilder
    private func importsBody(_ locations: [SemanticLocation]) -> some View {
        locationList(
            title: locations.count == 1 ? "Imports" : "Imports \(locations.count) files",
            locations: locations)
        hint
    }

    /// An import the build evaluated and declined. The condition and what it
    /// expanded to are the whole answer, so they lead; the prose reason is
    /// the fallback for skips MSBuild reports without one (missing file, no
    /// matches, unresolved SDK).
    @ViewBuilder
    private func skippedImportsBody(_ skipped: [SemanticSkippedImport]) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Label("Not imported", systemImage: "arrow.down.doc")
                .font(.caption)
                .foregroundStyle(.secondary)

            ForEach(Array(skipped.enumerated()), id: \.offset) { _, record in
                if record.hasCondition {
                    Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 4) {
                        GridRow {
                            Text("Condition")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .gridColumnAlignment(.trailing)
                                .fixedSize()
                            wrapped(record.condition ?? "")
                        }
                        GridRow {
                            Text("Evaluated")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .gridColumnAlignment(.trailing)
                                .fixedSize()
                            wrapped(record.evaluatedCondition ?? "")
                        }
                        GridRow {
                            Text("Result")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .gridColumnAlignment(.trailing)
                                .fixedSize()
                            Text("false")
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(.orange)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                } else if let reason = record.reason {
                    wrapped(reason, secondary: true)
                }
            }

            // This is the only condition evaluation a binlog contains, so
            // it's worth saying that the answer is the build's, not a guess.
            Text("As evaluated during the build.")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
    }

    private var hint: some View {
        Text(isPinned ? "⌘-click to navigate" : "⌘-click to navigate · click to keep open")
            .font(.caption2)
            .foregroundStyle(.tertiary)
    }

    // MARK: - pieces

    /// Monospaced, selectable, and allowed to take as many lines as it needs.
    private func wrapped(_ text: String, secondary: Bool = false) -> some View {
        Text(text)
            .font(.system(.caption, design: .monospaced))
            .foregroundStyle(secondary ? AnyShapeStyle(.secondary) : AnyShapeStyle(.primary))
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func factsGrid(_ facts: [SemanticFact]) -> some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 8, verticalSpacing: 4) {
            ForEach(Array(facts.enumerated()), id: \.offset) { _, fact in
                GridRow {
                    Text(fact.label ?? "")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .gridColumnAlignment(.trailing)
                        .fixedSize()
                    wrapped(fact.value ?? "")
                }
            }
        }
    }

    private func locationList(title: String, locations: [SemanticLocation]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)

            ForEach(locations.prefix(Self.maxLocations)) { location in
                VStack(alignment: .leading, spacing: 1) {
                    Text(location.label ?? location.path ?? "")
                        .font(.system(.caption, design: .monospaced))
                        .foregroundStyle(isReachable(location) ? .primary : .tertiary)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)

                    // The full path (or the value assigned here) — the part
                    // that is actually worth copying.
                    if let detail = detail(for: location) {
                        wrapped(detail, secondary: true)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }

            if locations.count > Self.maxLocations {
                Text("…and \(locations.count - Self.maxLocations) more")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
    }

    private func isReachable(_ location: SemanticLocation) -> Bool {
        location.available != false || location.nodeId != nil
    }

    /// The secondary line, when it says something the label doesn't.
    private func detail(for location: SemanticLocation) -> String? {
        let label = location.label
        for candidate in [location.path, location.detail] {
            guard let candidate, !candidate.isEmpty, candidate != label else { continue }
            return candidate
        }
        return nil
    }
}
