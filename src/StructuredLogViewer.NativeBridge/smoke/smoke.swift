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
typealias SemanticFileFn = @convention(c) (Int64, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32
typealias SemanticResolveFn = @convention(c) (Int64, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) -> Int32

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
let mslogSemanticFile = sym("mslog_semantic_file", SemanticFileFn.self)
let mslogSemanticResolve = sym("mslog_semantic_resolve", SemanticResolveFn.self)

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

// 11. semantics: pick a build file from the archive and resolve it
struct FileList: Decodable {
    struct Entry: Decodable { let path: String }
    let files: [Entry]
}
struct SemanticFile: Decodable {
    struct Context: Decodable { let evaluationId: String; let label: String }
    struct Import: Decodable { let line: Int; let importedPath: String; let available: Bool }
    struct TargetDef: Decodable { let name: String; let line: Int }
    let evaluationId: String?
    let contextsTotal: Int
    let contexts: [Context]?
    let imports: [Import]?
    let targets: [TargetDef]?
}
struct SemanticSymbol: Decodable {
    struct Location: Decodable { let path: String?; let line: Int; let label: String?; let nodeId: String? }
    struct Fact: Decodable { let label: String?; let value: String? }
    let kind: String
    let name: String
    let found: Bool
    let value: String?
    let note: String?
    let definitions: [Location]?
    let executions: [Location]?
    let facts: [Fact]?
}

let fileList = try! JSONDecoder().decode(FileList.self, from: files.data(using: .utf8)!)
let buildFiles = fileList.files.map(\.path).filter { path in
    [".csproj", ".props", ".targets"].contains { path.lowercased().hasSuffix($0) }
}
expect(!buildFiles.isEmpty, "archive has MSBuild files (\(buildFiles.count))")

// Prefer a file that actually imports something, so the import edges get exercised.
var semanticFile: SemanticFile? = nil
var semanticPath = ""
for candidate in buildFiles.prefix(50) {
    check(mslogSemanticFile(handle, candidate, nil, &json, &err), "semantic_file", err)
    let decodedFile = try! JSONDecoder().decode(SemanticFile.self, from: take(json).data(using: .utf8)!)
    // Prefer explicit <Import> elements: implicit SDK imports legitimately
    // report line 0, since there is no element to point at.
    if decodedFile.evaluationId != nil, (decodedFile.imports ?? []).contains(where: { $0.line > 0 }) {
        semanticFile = decodedFile
        semanticPath = candidate
        break
    }
}

if let semantic = semanticFile {
    print("  semantic file: \(semanticPath)")
    print("    contexts: \(semantic.contextsTotal), imports: \(semantic.imports?.count ?? 0), targets: \(semantic.targets?.count ?? 0)")
    expect(semantic.contextsTotal > 0, "file belongs to at least one evaluation")
    expect((semantic.contexts ?? []).contains { !$0.label.isEmpty }, "contexts are labelled")
    expect((semantic.imports ?? []).contains { $0.line > 0 }, "explicit import edges carry line numbers")
    expect((semantic.imports ?? []).allSatisfy { !$0.importedPath.isEmpty }, "import edges name a file")

    let evaluationId = semantic.evaluationId!

    // 12. resolve a property in that evaluation
    check(mslogSemanticResolve(handle, evaluationId, "property", "MSBuildProjectFile", &json, &err), "semantic_resolve", err)
    let property = try! JSONDecoder().decode(SemanticSymbol.self, from: take(json).data(using: .utf8)!)
    print("    $(MSBuildProjectFile) = \(property.value ?? "<none>") found=\(property.found)")
    expect(property.kind == "property" && property.name == "MSBuildProjectFile", "property echoes kind/name")

    check(mslogSemanticResolve(handle, evaluationId, "property", "NoSuchPropertyAnywhere", &json, &err), "semantic_resolve missing", err)
    let missing = try! JSONDecoder().decode(SemanticSymbol.self, from: take(json).data(using: .utf8)!)
    expect(!missing.found, "unknown property reports found=false rather than erroring")

    check(mslogSemanticResolve(handle, evaluationId, "target", "Build", &json, &err), "semantic_resolve target", err)
    let target = try! JSONDecoder().decode(SemanticSymbol.self, from: take(json).data(using: .utf8)!)
    print("    target Build: found=\(target.found) definitions=\(target.definitions?.count ?? 0) executions=\(target.executions?.count ?? 0)")
    expect(target.found, "target Build resolves")
    expect((target.definitions ?? []).allSatisfy { $0.line > 0 }, "target definitions carry line numbers")

    let badKind = mslogSemanticResolve(handle, evaluationId, "banana", "X", &json, &err)
    expect(badKind == 1, "unknown symbol kind is an error (status \(badKind))")
    _ = take(err); _ = take(json)
} else {
    expect(false, "found an MSBuild file with explicit import edges")
}

// 13. close, then use-after-close
check(mslogClose(handle, &err), "build_close", err)
expect(mslogInfo(handle, &json, &err) == 3, "use-after-close status")

// 12. cancellation: cancel an opId before starting the search
mslogCancel(77)
// (registering happens inside the call, so this exercises reuse-safe paths)
print(failures == 0 ? "ALL PASS" : "\(failures) FAILURES")
exit(failures == 0 ? 0 : 1)
