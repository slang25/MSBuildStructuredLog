//! The engine seam, platform-neutral to the views: every call is `async`,
//! returns the same DTOs, and is awaited on gpui's foreground executor.
//!
//! - Native: `libmslog.dylib` (the NativeAOT bridge the Swift viewer uses)
//!   through `libloading`; blocking calls hop to the background executor.
//! - Web: the same engine compiled to browser-wasm, running in a Web
//!   Worker; calls are JSON messages with the mslog.h payload shapes.

use crate::model::*;
use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

#[cfg(not(target_family = "wasm"))]
pub use native::{Engine, Host, OpenSource};
#[cfg(target_family = "wasm")]
pub use web::{Host, OpenSource, WorkerClient};

/// One open build, shared by every view as `Arc<Session>`.
pub struct Session {
    backend: Backend,
    next_op: AtomicI64,
}

#[cfg(not(target_family = "wasm"))]
struct Backend {
    native: Arc<native::NativeSession>,
    executor: gpui::BackgroundExecutor,
}

#[cfg(target_family = "wasm")]
struct Backend {
    client: std::rc::Rc<web::WorkerClient>,
}

impl Session {
    /// Opens, analyzes and indexes a binlog. Progress (0–1) lands in
    /// `progress` from wherever the engine runs.
    pub async fn open(host: Host, source: OpenSource, progress: Arc<AtomicU64>) -> Result<Session> {
        let backend = Backend::open(host, source, progress).await?;
        Ok(Session { backend, next_op: AtomicI64::new(1) })
    }

    pub fn allocate_op(&self) -> i64 {
        self.next_op.fetch_add(1, Ordering::Relaxed)
    }

    pub fn cancel(&self, op_id: i64) {
        self.backend.cancel(op_id);
    }

    pub async fn info(&self) -> Result<BuildInfo> {
        self.backend.info().await
    }

    pub async fn node(&self, id: &str) -> Result<NodeDetails> {
        self.backend.node(id.to_string()).await
    }

    /// Every child of a node, paging through the bridge's cap. `sort` is the
    /// bridge's sort mode: 0 natural, 1 by name, 2 by duration.
    pub async fn all_children(&self, id: &str, sort: i32) -> Result<Vec<NodeSummary>> {
        let mut all = Vec::new();
        let mut offset = 0;
        loop {
            let page = self.backend.children(id.to_string(), offset, 5000, sort).await?;
            let got = page.children.len();
            all.extend(page.children);
            offset += got;
            if got == 0 || offset >= page.total {
                break;
            }
        }
        Ok(all)
    }

    pub async fn ancestors(&self, id: &str) -> Result<Ancestors> {
        self.backend.ancestors(id.to_string()).await
    }

    pub async fn source(&self, id: &str) -> Result<SourceLocation> {
        self.backend.source(id.to_string()).await
    }

    pub async fn read_file(&self, path: &str) -> Result<String> {
        self.backend.read_file(path.to_string()).await
    }

    pub async fn preprocess(&self, id: &str) -> Result<String> {
        self.backend.preprocess(id.to_string()).await
    }

    /// The node and its descendants as indented plain text.
    pub async fn subtree_text(&self, id: &str) -> Result<String> {
        self.backend.subtree_text(id.to_string()).await
    }

    /// Cancellable through `cancel(op_id)` where the backend supports it.
    pub async fn search(&self, query: &str, max_results: usize, op_id: i64) -> Result<SearchResponse> {
        self.backend.search(query.to_string(), max_results, op_id).await
    }

    /// The Properties and items pane: the same result shape, scoped to one
    /// Project or ProjectEvaluation node.
    pub async fn search_properties_and_items(
        &self,
        context_node_id: &str,
        query: &str,
        max_results: usize,
        op_id: i64,
    ) -> Result<SearchResponse> {
        self.backend
            .search_properties_and_items(context_node_id.to_string(), query.to_string(), max_results, op_id)
            .await
    }

    /// Every source file embedded in the log, sorted by path.
    pub async fn files_list(&self) -> Result<FileList> {
        self.backend.files_list().await
    }

    /// Case-insensitive substring search across the embedded files.
    pub async fn files_search(&self, term: &str, max_results: usize, op_id: i64) -> Result<FileSearchResponse> {
        self.backend.files_search(term.to_string(), max_results, op_id).await
    }

    pub async fn semantic_file(&self, path: &str, evaluation_id: Option<String>) -> Result<SemanticFile> {
        self.backend.semantic_file(path.to_string(), evaluation_id).await
    }

    pub async fn semantic_resolve(&self, evaluation_id: &str, kind: &str, name: &str) -> Result<SemanticSymbol> {
        self.backend.semantic_resolve(evaluation_id.to_string(), kind.to_string(), name.to_string()).await
    }

    pub async fn timeline(&self) -> Result<Timeline> {
        self.backend.timeline().await
    }
}

// ===================================================================
// Native backend
// ===================================================================

#[cfg(not(target_family = "wasm"))]
mod native {
    use super::*;
    use anyhow::{Context, anyhow};
    use libloading::Library;
    use serde::de::DeserializeOwned;
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::{Path, PathBuf};

    type ProgressCb = unsafe extern "C" fn(*mut c_void, f64);
    type OutStr = *mut *mut c_char;

    #[allow(non_snake_case)]
    struct Fns {
        mslog_version: unsafe extern "C" fn() -> *mut c_char,
        mslog_string_free: unsafe extern "C" fn(*mut c_char),
        mslog_cancel: unsafe extern "C" fn(i64),
        mslog_build_open:
            unsafe extern "C" fn(*const c_char, i64, Option<ProgressCb>, *mut c_void, *mut i64, OutStr) -> i32,
        mslog_build_close: unsafe extern "C" fn(i64, OutStr) -> i32,
        mslog_build_info: unsafe extern "C" fn(i64, OutStr, OutStr) -> i32,
        mslog_node_get: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32,
        mslog_node_children: unsafe extern "C" fn(i64, *const c_char, i32, i32, i32, OutStr, OutStr) -> i32,
        mslog_node_ancestors: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32,
        mslog_node_source: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32,
        mslog_node_subtree_text: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32,
        mslog_search: unsafe extern "C" fn(i64, *const c_char, i32, i64, OutStr, OutStr) -> i32,
        mslog_search_properties_and_items:
            unsafe extern "C" fn(i64, *const c_char, *const c_char, i32, i64, OutStr, OutStr) -> i32,
        mslog_files_list: unsafe extern "C" fn(i64, OutStr, OutStr) -> i32,
        mslog_files_search: unsafe extern "C" fn(i64, *const c_char, i32, i64, OutStr, OutStr) -> i32,
        mslog_file_read: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32,
        mslog_node_preprocess: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32,
        mslog_semantic_file: unsafe extern "C" fn(i64, *const c_char, *const c_char, OutStr, OutStr) -> i32,
        mslog_semantic_resolve:
            unsafe extern "C" fn(i64, *const c_char, *const c_char, *const c_char, OutStr, OutStr) -> i32,
        mslog_timeline: unsafe extern "C" fn(i64, i64, OutStr, OutStr) -> i32,
    }

    /// The loaded dylib. One per process.
    pub struct Engine {
        _lib: Library,
        f: Fns,
    }

    unsafe impl Send for Engine {}
    unsafe impl Sync for Engine {}

    impl Engine {
        /// `$MSLOG_DYLIB`, beside the executable, or the bridge's `out/`.
        pub fn locate() -> Result<PathBuf> {
            if let Ok(p) = std::env::var("MSLOG_DYLIB") {
                return Ok(PathBuf::from(p));
            }
            let mut candidates = Vec::new();
            if let Ok(exe) = std::env::current_exe()
                && let Some(dir) = exe.parent()
            {
                candidates.push(dir.join("libmslog.dylib"));
            }
            candidates.push(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../StructuredLogViewer.NativeBridge/out/libmslog.dylib"),
            );
            candidates.into_iter().find(|p| p.exists()).ok_or_else(|| {
                anyhow!("libmslog.dylib not found. Build it with src/StructuredLogViewer.NativeBridge/build-dylib.sh or set MSLOG_DYLIB.")
            })
        }

        pub fn load(path: &Path) -> Result<Arc<Engine>> {
            // SAFETY: the bridge has no load-time side effects beyond NativeAOT runtime init.
            let lib = unsafe { Library::new(path) }.with_context(|| format!("loading {}", path.display()))?;
            macro_rules! sym {
                ($name:ident) => {{
                    // SAFETY: signatures mirror mslog.h; the Library outlives the copied pointers.
                    let s: libloading::Symbol<'_, _> = unsafe { lib.get(concat!(stringify!($name), "\0").as_bytes()) }
                        .with_context(|| format!("missing export {}", stringify!($name)))?;
                    *s
                }};
            }
            let f = Fns {
                mslog_version: sym!(mslog_version),
                mslog_string_free: sym!(mslog_string_free),
                mslog_cancel: sym!(mslog_cancel),
                mslog_build_open: sym!(mslog_build_open),
                mslog_build_close: sym!(mslog_build_close),
                mslog_build_info: sym!(mslog_build_info),
                mslog_node_get: sym!(mslog_node_get),
                mslog_node_children: sym!(mslog_node_children),
                mslog_node_ancestors: sym!(mslog_node_ancestors),
                mslog_node_source: sym!(mslog_node_source),
                mslog_node_subtree_text: sym!(mslog_node_subtree_text),
                mslog_search: sym!(mslog_search),
                mslog_search_properties_and_items: sym!(mslog_search_properties_and_items),
                mslog_files_list: sym!(mslog_files_list),
                mslog_files_search: sym!(mslog_files_search),
                mslog_file_read: sym!(mslog_file_read),
                mslog_node_preprocess: sym!(mslog_node_preprocess),
                mslog_semantic_file: sym!(mslog_semantic_file),
                mslog_semantic_resolve: sym!(mslog_semantic_resolve),
                mslog_timeline: sym!(mslog_timeline),
            };
            Ok(Arc::new(Engine { _lib: lib, f }))
        }

        pub fn version(&self) -> String {
            // SAFETY: returns a caller-owned C string or null.
            unsafe { self.take_string((self.f.mslog_version)()) }.unwrap_or_default()
        }

        pub fn cancel(&self, op_id: i64) {
            if op_id != 0 {
                // SAFETY: best-effort, any thread.
                unsafe { (self.f.mslog_cancel)(op_id) }
            }
        }

        unsafe fn take_string(&self, ptr: *mut c_char) -> Option<String> {
            if ptr.is_null() {
                return None;
            }
            // SAFETY: bridge strings are NUL-terminated UTF-8 owned by us.
            let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
            unsafe { (self.f.mslog_string_free)(ptr) };
            Some(s)
        }

        fn error(&self, status: i32, err: *mut c_char) -> anyhow::Error {
            // SAFETY: err is either null or a bridge string.
            let payload = unsafe { self.take_string(err) }.and_then(|s| serde_json::from_str::<BridgeError>(&s).ok());
            match status {
                2 => anyhow!("cancelled"),
                3 => anyhow!("the build is no longer open"),
                _ => match payload {
                    Some(p) => anyhow!("{} ({})", p.message, p.code),
                    None => anyhow!("bridge error (status {status})"),
                },
            }
        }

        fn call_json<T: DeserializeOwned>(&self, body: impl FnOnce(OutStr, OutStr) -> i32) -> Result<T> {
            let text = self.call_text(body)?;
            serde_json::from_str(&text).context("decoding bridge payload")
        }

        fn call_text(&self, body: impl FnOnce(OutStr, OutStr) -> i32) -> Result<String> {
            let mut out: *mut c_char = std::ptr::null_mut();
            let mut err: *mut c_char = std::ptr::null_mut();
            let status = body(&mut out, &mut err);
            if status != 0 {
                // SAFETY: on failure out is null per the contract; free anyway.
                unsafe { self.take_string(out) };
                return Err(self.error(status, err));
            }
            // SAFETY: success: out is a bridge string.
            unsafe { self.take_string(out) }.ok_or_else(|| anyhow!("the bridge returned no payload"))
        }
    }

    /// Blocking calls on one open build handle.
    pub struct NativeSession {
        engine: Arc<Engine>,
        handle: i64,
    }

    unsafe impl Send for NativeSession {}
    unsafe impl Sync for NativeSession {}

    impl NativeSession {
        fn open(engine: Arc<Engine>, path: &Path, progress: Arc<AtomicU64>) -> Result<NativeSession> {
            unsafe extern "C" fn trampoline(ctx: *mut c_void, ratio: f64) {
                // SAFETY: ctx is the Arc<AtomicU64> below, alive for the whole call.
                let sink = unsafe { &*(ctx as *const AtomicU64) };
                sink.store(ratio.to_bits(), Ordering::Relaxed);
            }
            let c_path = CString::new(path.to_string_lossy().as_bytes())?;
            let ctx = Arc::as_ptr(&progress) as *mut c_void;
            let mut handle: i64 = 0;
            let mut err: *mut c_char = std::ptr::null_mut();
            // SAFETY: pointers valid for the call; the progress Arc outlives it.
            let status = unsafe { (engine.f.mslog_build_open)(c_path.as_ptr(), 0, Some(trampoline), ctx, &mut handle, &mut err) };
            if status != 0 {
                return Err(engine.error(status, err));
            }
            Ok(NativeSession { engine, handle })
        }

        fn text(&self, id: &str, f: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32) -> Result<String> {
            let c = CString::new(id)?;
            self.engine.call_text(|out, err| unsafe { f(self.handle, c.as_ptr(), out, err) })
        }

        fn json<T: DeserializeOwned>(&self, id: &str, f: unsafe extern "C" fn(i64, *const c_char, OutStr, OutStr) -> i32) -> Result<T> {
            let c = CString::new(id)?;
            self.engine.call_json(|out, err| unsafe { f(self.handle, c.as_ptr(), out, err) })
        }
    }

    impl Drop for NativeSession {
        fn drop(&mut self) {
            let mut err: *mut c_char = std::ptr::null_mut();
            // SAFETY: closing blocks until in-flight calls drain.
            let status = unsafe { (self.engine.f.mslog_build_close)(self.handle, &mut err) };
            if status != 0 {
                let _ = self.engine.error(status, err);
            }
        }
    }

    /// What the app needs to open builds: the dylib and somewhere to block.
    #[derive(Clone)]
    pub struct Host {
        pub engine: Arc<Engine>,
        pub executor: gpui::BackgroundExecutor,
    }

    pub enum OpenSource {
        Path(PathBuf),
    }

    impl OpenSource {
        pub fn display_name(&self) -> String {
            match self {
                OpenSource::Path(p) => p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            }
        }
    }

    impl Backend {
        pub(super) async fn open(host: Host, source: OpenSource, progress: Arc<AtomicU64>) -> Result<Backend> {
            let OpenSource::Path(path) = source;
            let engine = host.engine.clone();
            let native = host.executor.spawn(async move { NativeSession::open(engine, &path, progress) }).await?;
            Ok(Backend { native: Arc::new(native), executor: host.executor })
        }

        pub(super) fn cancel(&self, op_id: i64) {
            self.native.engine.cancel(op_id);
        }

        fn run<T: Send + 'static>(&self, f: impl FnOnce(&NativeSession) -> Result<T> + Send + 'static) -> gpui::Task<Result<T>> {
            let native = self.native.clone();
            self.executor.spawn(async move { f(&native) })
        }

        pub(super) async fn info(&self) -> Result<BuildInfo> {
            self.run(|n| {
                let e = &n.engine;
                e.call_json(|out, err| unsafe { (e.f.mslog_build_info)(n.handle, out, err) })
            })
            .await
        }

        pub(super) async fn node(&self, id: String) -> Result<NodeDetails> {
            self.run(move |n| n.json(&id, n.engine.f.mslog_node_get)).await
        }

        pub(super) async fn children(&self, id: String, offset: usize, count: usize, sort: i32) -> Result<ChildrenPage> {
            self.run(move |n| {
                let c = CString::new(id)?;
                let e = &n.engine;
                e.call_json(|out, err| unsafe {
                    (e.f.mslog_node_children)(n.handle, c.as_ptr(), offset as i32, count as i32, sort, out, err)
                })
            })
            .await
        }

        pub(super) async fn ancestors(&self, id: String) -> Result<Ancestors> {
            self.run(move |n| n.json(&id, n.engine.f.mslog_node_ancestors)).await
        }

        pub(super) async fn source(&self, id: String) -> Result<SourceLocation> {
            self.run(move |n| n.json(&id, n.engine.f.mslog_node_source)).await
        }

        pub(super) async fn read_file(&self, path: String) -> Result<String> {
            self.run(move |n| n.text(&path, n.engine.f.mslog_file_read)).await
        }

        pub(super) async fn preprocess(&self, id: String) -> Result<String> {
            self.run(move |n| n.text(&id, n.engine.f.mslog_node_preprocess)).await
        }

        pub(super) async fn subtree_text(&self, id: String) -> Result<String> {
            self.run(move |n| n.text(&id, n.engine.f.mslog_node_subtree_text)).await
        }

        pub(super) async fn search(&self, query: String, max_results: usize, op_id: i64) -> Result<SearchResponse> {
            self.run(move |n| {
                let c = CString::new(query)?;
                let e = &n.engine;
                e.call_json(|out, err| unsafe {
                    (e.f.mslog_search)(n.handle, c.as_ptr(), max_results as i32, op_id, out, err)
                })
            })
            .await
        }

        pub(super) async fn search_properties_and_items(
            &self,
            context_node_id: String,
            query: String,
            max_results: usize,
            op_id: i64,
        ) -> Result<SearchResponse> {
            self.run(move |n| {
                let c_context = CString::new(context_node_id)?;
                let c_query = CString::new(query)?;
                let e = &n.engine;
                e.call_json(|out, err| unsafe {
                    (e.f.mslog_search_properties_and_items)(
                        n.handle,
                        c_context.as_ptr(),
                        c_query.as_ptr(),
                        max_results as i32,
                        op_id,
                        out,
                        err,
                    )
                })
            })
            .await
        }

        pub(super) async fn files_list(&self) -> Result<FileList> {
            self.run(|n| {
                let e = &n.engine;
                e.call_json(|out, err| unsafe { (e.f.mslog_files_list)(n.handle, out, err) })
            })
            .await
        }

        pub(super) async fn files_search(&self, term: String, max_results: usize, op_id: i64) -> Result<FileSearchResponse> {
            self.run(move |n| {
                let c = CString::new(term)?;
                let e = &n.engine;
                e.call_json(|out, err| unsafe {
                    (e.f.mslog_files_search)(n.handle, c.as_ptr(), max_results as i32, op_id, out, err)
                })
            })
            .await
        }

        pub(super) async fn semantic_file(&self, path: String, evaluation_id: Option<String>) -> Result<SemanticFile> {
            self.run(move |n| {
                let c_path = CString::new(path)?;
                let c_eval = evaluation_id.map(CString::new).transpose()?;
                let eval_ptr = c_eval.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
                let e = &n.engine;
                e.call_json(|out, err| unsafe { (e.f.mslog_semantic_file)(n.handle, c_path.as_ptr(), eval_ptr, out, err) })
            })
            .await
        }

        pub(super) async fn semantic_resolve(&self, evaluation_id: String, kind: String, name: String) -> Result<SemanticSymbol> {
            self.run(move |n| {
                let c_eval = CString::new(evaluation_id)?;
                let c_kind = CString::new(kind)?;
                let c_name = CString::new(name)?;
                let e = &n.engine;
                e.call_json(|out, err| unsafe {
                    (e.f.mslog_semantic_resolve)(n.handle, c_eval.as_ptr(), c_kind.as_ptr(), c_name.as_ptr(), out, err)
                })
            })
            .await
        }

        pub(super) async fn timeline(&self) -> Result<Timeline> {
            self.run(|n| {
                let e = &n.engine;
                e.call_json(|out, err| unsafe { (e.f.mslog_timeline)(n.handle, 0, out, err) })
            })
            .await
        }
    }
}

// ===================================================================
// Web backend: the engine in a Web Worker, JSON messages over postMessage
// ===================================================================

#[cfg(target_family = "wasm")]
mod web {
    use super::*;
    use anyhow::anyhow;
    use futures::channel::oneshot;
    use js_sys::{JSON, Object, Reflect};
    use serde_json::{Value, json};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;
    use web_sys::{MessageEvent, Worker, WorkerOptions, WorkerType};

    struct State {
        pending: RefCell<HashMap<u64, oneshot::Sender<Result<Value>>>>,
        next_id: Cell<u64>,
        ready: Cell<bool>,
        ready_waiters: RefCell<Vec<oneshot::Sender<()>>>,
        progress: RefCell<Option<Arc<AtomicU64>>>,
    }

    /// The page-side end of `engine-worker.js`. Protocol: requests are
    /// `{id, method, args}`, replies `{id, ok, result|error}`, plus
    /// unsolicited `{event: "ready"}` and `{event: "progress", ratio}`.
    pub struct WorkerClient {
        worker: Worker,
        state: Rc<State>,
        _onmessage: Closure<dyn FnMut(MessageEvent)>,
    }

    impl WorkerClient {
        pub fn new(url: &str) -> Result<Rc<WorkerClient>> {
            let options = WorkerOptions::new();
            options.set_type(WorkerType::Module);
            let worker = Worker::new_with_options(url, &options)
                .map_err(|e| anyhow!("starting engine worker: {}", js_string(&e)))?;
            let state = Rc::new(State {
                pending: RefCell::new(HashMap::new()),
                next_id: Cell::new(1),
                ready: Cell::new(false),
                ready_waiters: RefCell::new(Vec::new()),
                progress: RefCell::new(None),
            });
            let onmessage = {
                let state = state.clone();
                Closure::wrap(Box::new(move |event: MessageEvent| {
                    State::on_message(&state, event.data());
                }) as Box<dyn FnMut(MessageEvent)>)
            };
            worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            Ok(Rc::new(WorkerClient { worker, state, _onmessage: onmessage }))
        }

        /// Resolves once the .NET runtime in the worker has booted.
        pub async fn ready(&self) {
            if self.state.ready.get() {
                return;
            }
            let (tx, rx) = oneshot::channel();
            self.state.ready_waiters.borrow_mut().push(tx);
            let _ = rx.await;
        }

        /// A JSON request; `extra` lets `open` attach a `File` to args.
        pub async fn call(&self, method: &str, args: Value, extra: Option<(&str, JsValue)>) -> Result<Value> {
            self.ready().await;
            let id = self.state.next_id.get();
            self.state.next_id.set(id + 1);
            let (tx, rx) = oneshot::channel();
            self.state.pending.borrow_mut().insert(id, tx);

            let message = JSON::parse(&json!({ "id": id, "method": method, "args": args }).to_string())
                .map_err(|e| anyhow!("building request: {}", js_string(&e)))?;
            if let Some((key, value)) = extra {
                let args_obj = Reflect::get(&message, &JsValue::from_str("args")).unwrap_or(JsValue::UNDEFINED);
                let args_obj = if args_obj.is_object() { args_obj } else { Object::new().into() };
                Reflect::set(&args_obj, &JsValue::from_str(key), &value).ok();
                Reflect::set(&message, &JsValue::from_str("args"), &args_obj).ok();
            }
            self.worker.post_message(&message).map_err(|e| anyhow!("posting to engine worker: {}", js_string(&e)))?;
            rx.await.unwrap_or_else(|_| Err(anyhow!("engine worker dropped the request")))
        }

        pub fn set_progress_sink(&self, sink: Option<Arc<AtomicU64>>) {
            *self.state.progress.borrow_mut() = sink;
        }
    }

    impl State {
        fn on_message(state: &Rc<State>, data: JsValue) {
            let text = match JSON::stringify(&data) {
                Ok(s) => String::from(s),
                Err(_) => return,
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else { return };
            if let Some(event) = value.get("event").and_then(Value::as_str) {
                match event {
                    "ready" => {
                        state.ready.set(true);
                        for waiter in state.ready_waiters.borrow_mut().drain(..) {
                            let _ = waiter.send(());
                        }
                    }
                    "progress" => {
                        if let (Some(ratio), Some(sink)) = (value.get("ratio").and_then(Value::as_f64), state.progress.borrow().as_ref()) {
                            sink.store(ratio.to_bits(), Ordering::Relaxed);
                        }
                    }
                    _ => {}
                }
                return;
            }
            let Some(id) = value.get("id").and_then(Value::as_u64) else { return };
            let Some(tx) = state.pending.borrow_mut().remove(&id) else { return };
            let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let result = if ok {
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            } else {
                let err = value.get("error");
                let message = err.and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or("engine error");
                let code = err.and_then(|e| e.get("code")).and_then(Value::as_str).unwrap_or("Unknown");
                Err(anyhow!("{message} ({code})"))
            };
            let _ = tx.send(result);
        }
    }

    fn js_string(value: &JsValue) -> String {
        value.as_string().unwrap_or_else(|| format!("{value:?}"))
    }

    #[derive(Clone)]
    pub struct Host {
        pub client: Rc<WorkerClient>,
    }

    pub enum OpenSource {
        Url(String),
        File(web_sys::File),
    }

    impl OpenSource {
        pub fn display_name(&self) -> String {
            match self {
                OpenSource::Url(u) => u.rsplit('/').next().unwrap_or(u).to_string(),
                OpenSource::File(f) => f.name(),
            }
        }
    }

    impl Backend {
        pub(super) async fn open(host: Host, source: OpenSource, progress: Arc<AtomicU64>) -> Result<Backend> {
            host.client.set_progress_sink(Some(progress));
            let result = match source {
                OpenSource::Url(url) => host.client.call("open", json!({ "url": url }), None).await,
                OpenSource::File(file) => host.client.call("open", json!({}), Some(("file", file.into()))).await,
            };
            host.client.set_progress_sink(None);
            result?;
            Ok(Backend { client: host.client })
        }

        pub(super) fn cancel(&self, _op_id: i64) {
            // The worker runs one call at a time; a superseded search simply
            // has its result discarded by the caller's generation check.
        }

        async fn json<T: serde::de::DeserializeOwned>(&self, method: &str, args: Value) -> Result<T> {
            let value = self.client.call(method, args, None).await?;
            serde_json::from_value(value).map_err(|e| anyhow!("decoding {method} payload: {e}"))
        }

        async fn text(&self, method: &str, args: Value) -> Result<String> {
            let value = self.client.call(method, args, None).await?;
            Ok(value.get("text").and_then(Value::as_str).unwrap_or_default().to_string())
        }

        pub(super) async fn info(&self) -> Result<BuildInfo> {
            self.json("build_info", json!({})).await
        }

        pub(super) async fn node(&self, id: String) -> Result<NodeDetails> {
            self.json("node_get", json!({ "id": id })).await
        }

        pub(super) async fn children(&self, id: String, offset: usize, count: usize, sort: i32) -> Result<ChildrenPage> {
            self.json("node_children", json!({ "id": id, "offset": offset, "count": count, "sortMode": sort })).await
        }

        pub(super) async fn ancestors(&self, id: String) -> Result<Ancestors> {
            self.json("node_ancestors", json!({ "id": id })).await
        }

        pub(super) async fn source(&self, id: String) -> Result<SourceLocation> {
            self.json("node_source", json!({ "id": id })).await
        }

        pub(super) async fn read_file(&self, path: String) -> Result<String> {
            self.text("file_read", json!({ "path": path })).await
        }

        pub(super) async fn preprocess(&self, id: String) -> Result<String> {
            self.text("node_preprocess", json!({ "id": id })).await
        }

        pub(super) async fn subtree_text(&self, id: String) -> Result<String> {
            self.text("node_subtree_text", json!({ "id": id })).await
        }

        pub(super) async fn search(&self, query: String, max_results: usize, _op_id: i64) -> Result<SearchResponse> {
            self.json("search", json!({ "query": query, "maxResults": max_results })).await
        }

        pub(super) async fn search_properties_and_items(
            &self,
            context_node_id: String,
            query: String,
            max_results: usize,
            _op_id: i64,
        ) -> Result<SearchResponse> {
            self.json(
                "search_properties_and_items",
                json!({ "contextNodeId": context_node_id, "query": query, "maxResults": max_results }),
            )
            .await
        }

        pub(super) async fn files_list(&self) -> Result<FileList> {
            self.json("files_list", json!({})).await
        }

        pub(super) async fn files_search(&self, term: String, max_results: usize, _op_id: i64) -> Result<FileSearchResponse> {
            self.json("files_search", json!({ "term": term, "maxResults": max_results })).await
        }

        pub(super) async fn semantic_file(&self, path: String, evaluation_id: Option<String>) -> Result<SemanticFile> {
            self.json("semantic_file", json!({ "path": path, "evaluationId": evaluation_id })).await
        }

        pub(super) async fn semantic_resolve(&self, evaluation_id: String, kind: String, name: String) -> Result<SemanticSymbol> {
            self.json("semantic_resolve", json!({ "evaluationId": evaluation_id, "kind": kind, "name": name })).await
        }

        pub(super) async fn timeline(&self) -> Result<Timeline> {
            self.json("timeline", json!({})).await
        }
    }
}
