import CMSLog
import Foundation
import ViewerCore

/// Async Swift wrapper over one open build in libmslog. Conforms to
/// ViewerCore's `BinlogEngine`, decoding the bridge's JSON payloads
/// directly into ViewerCore model types. Every C call hops off the
/// calling thread; the bridge itself serializes the operations that need
/// it (search, preprocess, stats).
public final class BinlogSession: BinlogEngine, @unchecked Sendable {
    public let info: BuildInfo

    private let handle: Int64
    private let queue = DispatchQueue(label: "mslog.session", qos: .userInitiated, attributes: .concurrent)
    private let closed = OSAllocatedUnfairLockBox(false)
    private static let nextOperationId = OSAllocatedUnfairLockBox(Int64(1))
    private let currentSearchOp = OSAllocatedUnfairLockBox(Int64(0))
    private let currentFileSearchOp = OSAllocatedUnfairLockBox(Int64(0))

    public static var bridgeVersion: String {
        guard let ptr = mslog_version() else { return "unknown" }
        defer { mslog_string_free(ptr) }
        return String(cString: ptr)
    }

    public static var searchHelp: String {
        guard let ptr = mslog_search_help() else { return "" }
        defer { mslog_string_free(ptr) }
        return String(cString: ptr)
    }

    /// Opens, analyzes and indexes a binlog. Slow for large files; progress
    /// is reported 0–1 from a background thread.
    public static func open(
        path: String,
        progress: (@Sendable (Double) -> Void)? = nil
    ) async throws -> BinlogSession {
        let opId = allocateOperationId()
        let handle: Int64 = try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                var handle: Int64 = 0
                var errorJson: UnsafeMutablePointer<CChar>? = nil

                var box: Unmanaged<ProgressBox>? = nil
                var callback: mslog_progress_cb? = nil
                var context: UnsafeMutableRawPointer? = nil
                if let progress {
                    let retained = Unmanaged.passRetained(ProgressBox(progress))
                    box = retained
                    context = retained.toOpaque()
                    callback = { ctx, ratio in
                        guard let ctx else { return }
                        Unmanaged<ProgressBox>.fromOpaque(ctx).takeUnretainedValue().report(ratio)
                    }
                }

                let status = mslog_build_open(path, opId, callback, context, &handle, &errorJson)
                box?.release()

                if status == 0 {
                    continuation.resume(returning: handle)
                } else {
                    continuation.resume(throwing: Self.error(from: status, json: errorJson))
                }
            }
        }

        return try BinlogSession(handle: handle)
    }

    private init(handle: Int64) throws {
        self.handle = handle
        var json: UnsafeMutablePointer<CChar>? = nil
        var errorJson: UnsafeMutablePointer<CChar>? = nil
        let status = mslog_build_info(handle, &json, &errorJson)
        guard status == 0 else {
            var closeError: UnsafeMutablePointer<CChar>? = nil
            _ = mslog_build_close(handle, &closeError)
            mslog_string_free(closeError)
            throw Self.error(from: status, json: errorJson)
        }

        self.info = try Self.decode(BuildInfo.self, consuming: json)
    }

    /// Cancels in-flight searches and blocks (off-thread) until calls
    /// drain, then releases the native build graph.
    public func close() async {
        let alreadyClosed = closed.withLock { value -> Bool in
            let was = value
            value = true
            return was
        }
        guard !alreadyClosed else { return }

        cancelCurrent(currentSearchOp)
        cancelCurrent(currentFileSearchOp)

        let handle = self.handle
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                var errorJson: UnsafeMutablePointer<CChar>? = nil
                _ = mslog_build_close(handle, &errorJson)
                mslog_string_free(errorJson)
                continuation.resume()
            }
        }
    }

    // MARK: - BinlogEngine

    public func node(_ id: String) async throws -> NodeDetails {
        try await call(NodeDetails.self) { handle, json, err in
            mslog_node_get(handle, id, json, err)
        }
    }

    public func children(of id: String, offset: Int, count: Int, sortMode: ChildSortMode) async throws -> ChildrenPage {
        try await call(ChildrenPage.self) { handle, json, err in
            mslog_node_children(handle, id, Int32(offset), Int32(count), Int32(sortMode.rawValue), json, err)
        }
    }

    public func ancestors(of id: String) async throws -> Ancestors {
        try await call(Ancestors.self) { handle, json, err in
            mslog_node_ancestors(handle, id, json, err)
        }
    }

    public func subtreeText(of id: String) async throws -> String {
        try await callText { handle, text, err in
            mslog_node_subtree_text(handle, id, text, err)
        }
    }

    public func source(of id: String) async throws -> SourceLocation {
        try await call(SourceLocation.self) { handle, json, err in
            mslog_node_source(handle, id, json, err)
        }
    }

    public func preprocess(_ id: String) async throws -> String {
        try await callText { handle, text, err in
            mslog_node_preprocess(handle, id, text, err)
        }
    }

    public func search(query: String, maxResults: Int) async throws -> SearchResponse {
        let opId = replaceOperation(currentSearchOp)
        return try await call(SearchResponse.self) { handle, json, err in
            mslog_search(handle, query, Int32(maxResults), opId, json, err)
        }
    }

    public func searchPropertiesAndItems(contextId: String, query: String, maxResults: Int) async throws -> SearchResponse {
        let opId = replaceOperation(currentSearchOp)
        return try await call(SearchResponse.self) { handle, json, err in
            mslog_search_properties_and_items(handle, contextId, query, Int32(maxResults), opId, json, err)
        }
    }

    public func listFiles() async throws -> FileList {
        try await call(FileList.self) { handle, json, err in
            mslog_files_list(handle, json, err)
        }
    }

    public func readFile(path: String) async throws -> String {
        try await callText { handle, text, err in
            mslog_file_read(handle, path, text, err)
        }
    }

    public func searchFiles(term: String, maxResults: Int) async throws -> FileSearchResponse {
        let opId = replaceOperation(currentFileSearchOp)
        return try await call(FileSearchResponse.self) { handle, json, err in
            mslog_files_search(handle, term, Int32(maxResults), opId, json, err)
        }
    }

    public func stats() async throws -> BuildStats {
        try await call(BuildStats.self) { handle, json, err in
            mslog_build_stats(handle, 0, json, err)
        }
    }

    // MARK: - plumbing

    private final class ProgressBox {
        private let handler: @Sendable (Double) -> Void
        private let lastReported = OSAllocatedUnfairLockBox(-1.0)

        init(_ handler: @escaping @Sendable (Double) -> Void) {
            self.handler = handler
        }

        func report(_ ratio: Double) {
            // The reader fires per buffer; throttle to meaningful steps so
            // the UI isn't flooded with main-actor hops during huge loads.
            let shouldReport = lastReported.withLock { last -> Bool in
                if ratio >= 1.0 || ratio - last >= 0.005 {
                    last = ratio
                    return true
                }
                return false
            }

            if shouldReport {
                handler(ratio)
            }
        }
    }

    private static func allocateOperationId() -> Int64 {
        nextOperationId.withLock { value in
            value += 1
            return value
        }
    }

    /// Cancels the previous operation registered in `slot` (if any) and
    /// registers a fresh id — the cancel-previous-search-on-keystroke path.
    private func replaceOperation(_ slot: OSAllocatedUnfairLockBox<Int64>) -> Int64 {
        let opId = Self.allocateOperationId()
        let previous = slot.withLock { value -> Int64 in
            let old = value
            value = opId
            return old
        }
        if previous != 0 {
            mslog_cancel(previous)
        }
        return opId
    }

    private func cancelCurrent(_ slot: OSAllocatedUnfairLockBox<Int64>) {
        let current = slot.withLock { value -> Int64 in
            let old = value
            value = 0
            return old
        }
        if current != 0 {
            mslog_cancel(current)
        }
    }

    private typealias RawCall = (
        Int64,
        UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
        UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
    ) -> Int32

    private func call<T: Decodable>(_ type: T.Type, _ body: @escaping RawCall) async throws -> T {
        let handle = self.handle
        return try await withCheckedThrowingContinuation { continuation in
            queue.async {
                var json: UnsafeMutablePointer<CChar>? = nil
                var errorJson: UnsafeMutablePointer<CChar>? = nil
                let status = body(handle, &json, &errorJson)
                if status == 0 {
                    do {
                        continuation.resume(returning: try Self.decode(type, consuming: json))
                    } catch {
                        continuation.resume(throwing: error)
                    }
                } else {
                    mslog_string_free(json)
                    continuation.resume(throwing: Self.error(from: status, json: errorJson))
                }
            }
        }
    }

    private func callText(_ body: @escaping RawCall) async throws -> String {
        let handle = self.handle
        return try await withCheckedThrowingContinuation { continuation in
            queue.async {
                var text: UnsafeMutablePointer<CChar>? = nil
                var errorJson: UnsafeMutablePointer<CChar>? = nil
                let status = body(handle, &text, &errorJson)
                if status == 0 {
                    let result = text.map { String(cString: $0) } ?? ""
                    mslog_string_free(text)
                    continuation.resume(returning: result)
                } else {
                    mslog_string_free(text)
                    continuation.resume(throwing: Self.error(from: status, json: errorJson))
                }
            }
        }
    }

    private static func decode<T: Decodable>(_ type: T.Type, consuming json: UnsafeMutablePointer<CChar>?) throws -> T {
        guard let json else {
            throw EngineError.failure(code: "EmptyPayload", message: "The bridge returned no payload.")
        }
        defer { mslog_string_free(json) }
        let data = Data(bytes: json, count: strlen(json))
        do {
            return try JSONDecoder().decode(type, from: data)
        } catch {
            throw EngineError.failure(code: "DecodeFailure", message: "Failed to decode bridge payload: \(error)")
        }
    }

    private static func error(from status: Int32, json: UnsafeMutablePointer<CChar>?) -> EngineError {
        var payload: BridgeErrorPayload? = nil
        if let json {
            let data = Data(bytes: json, count: strlen(json))
            payload = try? JSONDecoder().decode(BridgeErrorPayload.self, from: data)
            mslog_string_free(json)
        }

        switch status {
        case 2: return .cancelled
        case 3: return .badHandle
        case 4: return .badNodeId(payload?.message ?? "Unknown node id")
        default:
            return .failure(
                code: payload?.code ?? "Unknown",
                message: payload?.message ?? "Unknown bridge error (status \(status))")
        }
    }
}

/// Tiny generic lock box (OSAllocatedUnfairLock's value API without the
/// iOS 16/macOS 13 availability dance in one place).
final class OSAllocatedUnfairLockBox<Value>: @unchecked Sendable {
    private var value: Value
    private let lock = NSLock()

    init(_ value: Value) {
        self.value = value
    }

    func withLock<R>(_ body: (inout Value) -> R) -> R {
        lock.lock()
        defer { lock.unlock() }
        return body(&value)
    }
}
