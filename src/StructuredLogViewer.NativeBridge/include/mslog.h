/*
 * mslog.h — C ABI for libmslog.dylib, the NativeAOT-compiled bridge over
 * the MSBuild Structured Log engine (StructuredLogger +
 * StructuredLogger.Utils). This header is the single source of truth for
 * the export surface; build-dylib.sh cross-checks it against `nm -gU`.
 *
 * == Conventions ==
 *
 * Strings: all char* parameters and results are null-terminated UTF-8.
 *   Every char* RETURNED by the library (out-params and direct returns)
 *   is owned by the caller and must be released with mslog_string_free.
 *   Input strings are borrowed for the duration of the call only.
 *
 * Status codes (mslog_status): every function returning int32_t uses
 *   0 = ok
 *   1 = error         (*error_json receives {"code","message"})
 *   2 = cancelled     (via mslog_cancel on the call's op_id)
 *   3 = bad handle    (unknown or already-closed build handle)
 *   4 = bad node id   (*error_json receives {"code","message"})
 *   On any nonzero status, out-params other than error_json are NULL.
 *   error_json may be NULL if the caller doesn't want details.
 *
 * Handles: mslog_build_open returns an opaque int64 handle. Handles stay
 *   valid until mslog_build_close, which BLOCKS until in-flight calls on
 *   that handle drain (cancel long operations first), then releases the
 *   build graph and forces a compacting GC.
 *
 * Node ids: strings, stable for the same binlog file bytes. "42" for
 *   timed nodes (their dense index), "42/3.7" path form for others
 *   (child 7 of child 3 of node 42). Not portable across binlog files.
 *
 * Cancellation: pass a caller-chosen nonzero op_id to a cancellable call
 *   and invoke mslog_cancel(op_id) from any thread. op_id 0 = not
 *   cancellable. Use unique ids per operation.
 *
 * Threading: after open, node/file reads (node_get, node_children,
 *   node_ancestors, node_subtree_text, node_source, files_list,
 *   file_read) are lock-free and may run concurrently. Searches,
 *   properties-and-items, preprocess and stats are serialized per
 *   session internally (the engine's search index is not thread-safe).
 *   The progress callback fires on a background thread and must not
 *   call back into the library.
 */

#ifndef MSLOG_H
#define MSLOG_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    MSLOG_OK = 0,
    MSLOG_ERROR = 1,
    MSLOG_CANCELLED = 2,
    MSLOG_BAD_HANDLE = 3,
    MSLOG_BAD_NODE_ID = 4
};

/* Progress for mslog_build_open; ratio is 0.0–1.0. */
typedef void (*mslog_progress_cb)(void *ctx, double ratio);

/* --- lifecycle-free ----------------------------------------------------- */

/* Returns the bridge version string (caller frees). */
char *mslog_version(void);

/* Releases any string returned by this library. NULL is a no-op. */
void mslog_string_free(char *str);

/* Cancels the in-flight operation registered under op_id (best-effort). */
void mslog_cancel(int64_t op_id);

/* Returns the search query syntax reference, Markdown (caller frees). */
char *mslog_search_help(void);

/* --- build lifecycle ---------------------------------------------------- */

/* Opens, analyzes and indexes a .binlog. Slow (seconds for large logs);
 * progress/progress_ctx may be NULL. On success *out_handle is set. */
int32_t mslog_build_open(const char *path,
                         int64_t op_id,
                         mslog_progress_cb progress,
                         void *progress_ctx,
                         int64_t *out_handle,
                         char **error_json);

/* Closes a build. Blocks until in-flight calls drain. */
int32_t mslog_build_close(int64_t handle, char **error_json);

/* BuildInfo JSON: {rootId, succeeded, errorCount, warningCount, nodeCount,
 * hasSourceArchive, msBuildVersion, filePath, fileSize, durationMs,
 * startTime, endTime}. */
int32_t mslog_build_info(int64_t handle, char **out_json, char **error_json);

/* --- nodes -------------------------------------------------------------- */

/* NodeDetails JSON: {node: NodeSummary, parentId, startTime, endTime,
 * fullText, sourceFile, sourceLine}. NodeSummary: {id, kind, title, name,
 * value, hasChildren, childCount, isLowRelevance, state, durationMs,
 * hasSource, canPreprocess, props{}}. */
int32_t mslog_node_get(int64_t handle,
                       const char *node_id,
                       char **out_json,
                       char **error_json);

/* ChildrenPage JSON: {parentId, total, offset, count, sortMode,
 * children: [NodeSummary]}. sort_mode: 0 natural, 1 by name,
 * 2 by duration (longest first). count <= 0 defaults to 512; capped
 * at 5000. */
int32_t mslog_node_children(int64_t handle,
                            const char *node_id,
                            int32_t offset,
                            int32_t count,
                            int32_t sort_mode,
                            char **out_json,
                            char **error_json);

/* Ancestors JSON: {chain: [NodeSummary]} — root first, parent last. */
int32_t mslog_node_ancestors(int64_t handle,
                             const char *node_id,
                             char **out_json,
                             char **error_json);

/* Indented plain-text rendering of the node and its descendants. */
int32_t mslog_node_subtree_text(int64_t handle,
                                const char *node_id,
                                char **out_text,
                                char **error_json);

/* SourceLocation JSON: {filePath, line, text}. text is the embedded
 * file's full content, or absent when not archived. */
int32_t mslog_node_source(int64_t handle,
                          const char *node_id,
                          char **out_json,
                          char **error_json);

/* Effective MSBuild XML with all imports recursively inlined, for a
 * preprocessable node (Project / ProjectEvaluation / Import). */
int32_t mslog_node_preprocess(int64_t handle,
                              const char *node_id,
                              char **out_text,
                              char **error_json);

/* --- search ------------------------------------------------------------- */

/* SearchResponse JSON: {query, resultCount, overflow, elapsedMs, roots:
 * [{node: NodeSummary|absent, text, highlights: [{text, isHighlight,
 * style}], children: [...]}]}. Viewer query syntax (mslog_search_help).
 * max_results <= 0 defaults to 500; capped at 5000. */
int32_t mslog_search(int64_t handle,
                     const char *query,
                     int32_t max_results,
                     int64_t op_id,
                     char **out_json,
                     char **error_json);

/* Same response shape, scoped to a Project or ProjectEvaluation node's
 * properties/items/assignments (the viewer's Properties+Items pane). */
int32_t mslog_search_properties_and_items(int64_t handle,
                                          const char *context_node_id,
                                          const char *query,
                                          int32_t max_results,
                                          int64_t op_id,
                                          char **out_json,
                                          char **error_json);

/* --- embedded source files ---------------------------------------------- */

/* FileList JSON: {total, files: [{path, lines, length}]}, sorted by path. */
int32_t mslog_files_list(int64_t handle, char **out_json, char **error_json);

/* Full text of one embedded file; path as returned by mslog_files_list. */
int32_t mslog_file_read(int64_t handle,
                        const char *path,
                        char **out_text,
                        char **error_json);

/* FileSearchResponse JSON: {query, totalMatches, overflow, files: [{path,
 * matches: [{line, text, spans: [{start, length}]}]}]}. Case-insensitive
 * substring; max_results <= 0 defaults to 500, capped at 5000. */
int32_t mslog_files_search(int64_t handle,
                           const char *term,
                           int32_t max_results,
                           int64_t op_id,
                           char **out_json,
                           char **error_json);

/* --- statistics --------------------------------------------------------- */

/* Stats JSON mirroring the viewer's Statistics dialog (re-reads the file
 * from disk): sizes, counts and a recursive record-type breakdown. */
int32_t mslog_build_stats(int64_t handle,
                          int64_t op_id,
                          char **out_json,
                          char **error_json);

#ifdef __cplusplus
}
#endif

#endif /* MSLOG_H */
