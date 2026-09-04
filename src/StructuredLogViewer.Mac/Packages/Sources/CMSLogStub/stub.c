/*
 * No-op definitions of the libmslog C ABI.
 *
 * ViewerUI links BinlogKit, which references these symbols, so anything that
 * links ViewerUI needs them at link time — including the ViewerUI test
 * bundle, which never opens a build and so never calls one. The app links
 * the real NativeAOT dylib instead; this target is a test-only dependency
 * and the two are never in the same binary.
 *
 * Every function reports MSLOG_ERROR (1) and writes no out-params, so a test
 * that does reach the bridge by accident fails loudly rather than reading
 * uninitialized memory. Keep in step with ../../../StructuredLogViewer.NativeBridge/include/mslog.h;
 * build-dylib.sh's export check is the source of truth for the list.
 */
#include <stdint.h>
#include <stddef.h>

#define MSLOG_STUB_FAIL return 1;

char *mslog_version(void) { return NULL; }
char *mslog_search_help(void) { return NULL; }
void mslog_string_free(char *str) { (void)str; }
void mslog_cancel(int64_t op_id) { (void)op_id; }

int32_t mslog_build_open(const char *p, int64_t o, void *cb, void *ctx, int64_t *h, char **e) {
    (void)p; (void)o; (void)cb; (void)ctx; (void)h; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_build_close(int64_t h, char **e) { (void)h; (void)e; MSLOG_STUB_FAIL }
int32_t mslog_build_info(int64_t h, char **o, char **e) { (void)h; (void)o; (void)e; MSLOG_STUB_FAIL }
int32_t mslog_build_stats(int64_t h, int64_t op, char **o, char **e) {
    (void)h; (void)op; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_node_get(int64_t h, const char *n, char **o, char **e) {
    (void)h; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_node_children(int64_t h, const char *n, int32_t off, int32_t c, int32_t s, char **o, char **e) {
    (void)h; (void)n; (void)off; (void)c; (void)s; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_node_ancestors(int64_t h, const char *n, char **o, char **e) {
    (void)h; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_node_subtree_text(int64_t h, const char *n, char **o, char **e) {
    (void)h; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_node_source(int64_t h, const char *n, char **o, char **e) {
    (void)h; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_node_preprocess(int64_t h, const char *n, char **o, char **e) {
    (void)h; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_target_parent(int64_t h, const char *n, char **o, char **e) {
    (void)h; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_semantic_file(int64_t h, const char *p, const char *v, char **o, char **e) {
    (void)h; (void)p; (void)v; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_semantic_resolve(int64_t h, const char *v, const char *k, const char *n, char **o, char **e) {
    (void)h; (void)v; (void)k; (void)n; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_search(int64_t h, const char *q, int32_t m, int64_t op, char **o, char **e) {
    (void)h; (void)q; (void)m; (void)op; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_search_properties_and_items(int64_t h, const char *c, const char *q, int32_t m, int64_t op, char **o, char **e) {
    (void)h; (void)c; (void)q; (void)m; (void)op; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_files_list(int64_t h, char **o, char **e) { (void)h; (void)o; (void)e; MSLOG_STUB_FAIL }
int32_t mslog_file_read(int64_t h, const char *p, char **o, char **e) {
    (void)h; (void)p; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_files_search(int64_t h, const char *t, int32_t m, int64_t op, char **o, char **e) {
    (void)h; (void)t; (void)m; (void)op; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_timeline(int64_t h, int64_t op, char **o, char **e) {
    (void)h; (void)op; (void)o; (void)e; MSLOG_STUB_FAIL
}
int32_t mslog_project_graph(int64_t h, char **o, char **e) { (void)h; (void)o; (void)e; MSLOG_STUB_FAIL }
