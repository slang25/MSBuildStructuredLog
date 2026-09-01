import SwiftUI
import ViewerCore

/// Statistics sheet: binlog record breakdown (mirrors the viewer's
/// Statistics dialog), computed on demand by re-reading the file.
struct StatisticsView: View {
    let engine: any BinlogEngine
    @Environment(\.dismiss) private var dismiss

    @State private var stats: BuildStats?
    @State private var error: String?

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Binlog Statistics")
                    .font(.headline)
                Spacer()
                Button("Done") { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
            .padding(12)

            Divider()

            if let stats {
                statsBody(stats)
            } else if let error {
                ContentUnavailableView(error, systemImage: "exclamationmark.triangle")
            } else {
                ProgressView("Analyzing binlog records…")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .frame(width: 560, height: 480)
        .task {
            do {
                stats = try await engine.stats()
            } catch {
                self.error = (error as? EngineError)?.message ?? error.localizedDescription
            }
        }
    }

    private func statsBody(_ stats: BuildStats) -> some View {
        VStack(spacing: 0) {
            Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 4) {
                GridRow {
                    summaryCell("File size", bytes: stats.fileSize)
                    summaryCell("Uncompressed", bytes: stats.uncompressedStreamSize)
                    summaryCell("Records", count: Int(stats.recordCount))
                    summaryCell("Format", text: "v\(stats.fileFormatVersion)")
                }
                GridRow {
                    summaryCell("Strings", count: stats.stringCount)
                    summaryCell("String bytes", bytes: stats.stringTotalSize)
                    summaryCell("Name-value lists", count: stats.nameValueListCount)
                    summaryCell("Blobs", count: stats.blobCount)
                }
            }
            .padding(12)

            Divider()

            if let records = stats.records {
                List {
                    RecordRows(record: records, depth: 0)
                }
            }
        }
    }

    private func summaryCell(_ label: String, bytes: Int64) -> some View {
        summaryCell(label, text: ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file))
    }

    private func summaryCell(_ label: String, count: Int) -> some View {
        summaryCell(label, text: count.formatted())
    }

    private func summaryCell(_ label: String, text: String) -> some View {
        VStack(alignment: .leading, spacing: 1) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(text)
                .font(.callout.monospacedDigit())
        }
    }
}

private struct RecordRows: View {
    let record: StatsRecord
    let depth: Int

    var body: some View {
        HStack {
            Text(String(repeating: "    ", count: depth) + record.type)
                .font(.caption)
            Spacer()
            Text("\(record.count.formatted()) × \(ByteCountFormatter.string(fromByteCount: record.totalLength, countStyle: .file))")
                .font(.caption.monospacedDigit())
                .foregroundStyle(.secondary)
        }

        if let children = record.children {
            ForEach(Array(children.enumerated()), id: \.offset) { _, child in
                RecordRows(record: child, depth: depth + 1)
            }
        }
    }
}
