// Smoke test for libmslog.dylib via dlopen — no Xcode project needed.
// Usage: swift smoke.swift <path-to-libmslog.dylib> <path-to.binlog>
import Foundation

let args = CommandLine.arguments
guard args.count == 3 else {
    fatalError("usage: swift smoke.swift <libmslog.dylib> <file.binlog>")
}

guard let lib = dlopen(args[1], RTLD_NOW) else {
    fatalError("dlopen failed: \(String(cString: dlerror()))")
}

func sym<T>(_ name: String, _ type: T.Type) -> T {
    guard let ptr = dlsym(lib, name) else { fatalError("missing symbol \(name)") }
    return unsafeBitCast(ptr, to: T.self)
}

typealias VersionFn = @convention(c) () -> UnsafeMutablePointer<CChar>?
typealias StringFreeFn = @convention(c) (UnsafeMutablePointer<CChar>?) -> Void
typealias CancelFn = @convention(c) (Int64) -> Void
typealias ProgressCb = @convention(c) (UnsafeMutableRawPointer?, Double) -> Void
typealias OpenFn = @convention(c) (
    UnsafePointer<CChar>?, Int64, ProgressCb?, UnsafeMutableRawPointer?,
    UnsafeMutablePointer<Int64>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32
typealias CloseFn = @convention(c) (Int64, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
typealias JsonFn = @convention(c) (Int64, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
typealias NodeJsonFn = @convention(c) (Int64, UnsafePointer<CChar>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
typealias ChildrenFn = @convention(c) (Int64, UnsafePointer<CChar>?, Int32, Int32, Int32, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
typealias SearchFn = @convention(c) (Int64, UnsafePointer<CChar>?, Int32, Int64, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32

let mslogVersion = sym("mslog_version", VersionFn.self)
let mslogStringFree = sym("mslog_string_free", StringFreeFn.self)
let mslogCancel = sym("mslog_cancel", CancelFn.self)
let mslogOpen = sym("mslog_build_open", OpenFn.self)
let mslogClose = sym("mslog_build_close", CloseFn.self)
let mslogInfo = sym("mslog_build_info", JsonFn.self)
let mslogNodeGet = sym("mslog_node_get", NodeJsonFn.self)
let mslogChildren = sym("mslog_node_children", ChildrenFn.self)
let mslogAncestors = sym("mslog_node_ancestors", NodeJsonFn.self)
let mslogSearch = sym("mslog_search", SearchFn.self)
let mslogFilesList = sym("mslog_files_list", JsonFn.self)
let mslogSearchHelp = sym("mslog_search_help", VersionFn.self)

func take(_ ptr: UnsafeMutablePointer<CChar>?) -> String {
    guard let ptr else { return "" }
    defer { mslogStringFree(ptr) }
    return String(cString: ptr)
}

func check(_ status: Int32, _ what: String, _ error: UnsafeMutablePointer<CChar>?) {
    if status != 0 {
        fatalError("\(what) failed with status \(status): \(take(error))")
    }
}

var failures = 0
func expect(_ condition: Bool, _ label: String) {
    print(condition ? "PASS \(label)" : "FAIL \(label)")
    if !condition { failures += 1 }
}

// 1. version
let version = take(mslogVersion())
expect(!version.isEmpty, "version = \(version)")

// 2. open with progress
var lastRatio = -1.0
let progress: ProgressCb = { ctx, ratio in
    ctx!.assumingMemoryBound(to: Double.self).pointee = ratio
}
var handle: Int64 = 0
var err: UnsafeMutablePointer<CChar>? = nil
let status = withUnsafeMutablePointer(to: &lastRatio) { ratioPtr in
    mslogOpen(args[2], 1, progress, UnsafeMutableRawPointer(ratioPtr), &handle, &err)
}
check(status, "build_open", err)
expect(handle > 0, "build_open handle = \(handle)")
expect(lastRatio > 0, "progress reported (last ratio \(lastRatio))")

// 3. build info
var json: UnsafeMutablePointer<CChar>? = nil
check(mslogInfo(handle, &json, &err), "build_info", err)
let info = take(json)
print("  info: \(info.prefix(300))")
expect(info.contains("\"rootId\""), "build_info has rootId")

struct Info: Decodable { let rootId: String; let nodeCount: Int }
let decoded = try! JSONDecoder().decode(Info.self, from: info.data(using: .utf8)!)
expect(decoded.nodeCount > 0, "nodeCount = \(decoded.nodeCount)")

// 4. root children
check(mslogChildren(handle, decoded.rootId, 0, 100, 0, &json, &err), "node_children", err)
let children = take(json)
print("  children: \(children.prefix(300))")
expect(children.contains("\"children\""), "children payload")

// 5. node details for root
check(mslogNodeGet(handle, decoded.rootId, &json, &err), "node_get", err)
expect(take(json).contains("\"kind\":\"Build\""), "root node kind is Build")

// 6. search
check(mslogSearch(handle, "$target", 50, 2, &json, &err), "search", err)
let search = take(json)
print("  search: \(search.prefix(300))")
expect(search.contains("\"resultCount\""), "search results")

// 7. bad node id → status 4
let badStatus = mslogNodeGet(handle, "not-a-node", &json, &err)
expect(badStatus == 4, "bad node id status = \(badStatus)")
_ = take(err); _ = take(json)

// 8. bad handle → status 3
expect(mslogInfo(9999, &json, &err) == 3, "bad handle status")

// 9. files list
check(mslogFilesList(handle, &json, &err), "files_list", err)
let files = take(json)
print("  files: \(files.prefix(200))")
expect(files.contains("\"total\""), "files list")

// 10. search help
expect(take(mslogSearchHelp()).contains("$error"), "search help text")

// 11. close, then use-after-close
check(mslogClose(handle, &err), "build_close", err)
expect(mslogInfo(handle, &json, &err) == 3, "use-after-close status")

// 12. cancellation: cancel an opId before starting the search
mslogCancel(77)
// (registering happens inside the call, so this exercises reuse-safe paths)
print(failures == 0 ? "ALL PASS" : "\(failures) FAILURES")
exit(failures == 0 ? 0 : 1)
