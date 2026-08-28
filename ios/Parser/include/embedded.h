// The device's own server, as C sees it.
//
// Hand-written rather than generated, for the reason quickadd.h gives: a generator
// would be a build-time dependency and a second thing to install, and this has to keep
// agreeing with web/embedded/src/ffi.rs either way.
//
// Every call answers with JSON the caller owns and must hand to embedded_free:
//
//     {"ok": …}
//     {"error": "…"}
//
// See web/embedded/src/ffi.rs for the full contract, including which calls block.
#ifndef EMBEDDED_H
#define EMBEDDED_H

#include <stdbool.h>
#include <stdint.h>

typedef struct EmbeddedLocal EmbeddedLocal;
typedef struct EmbeddedWatcher EmbeddedWatcher;
typedef struct EmbeddedStopper EmbeddedStopper;

/// Opens the database at `path`, migrating it. NULL if it could not be opened.
EmbeddedLocal *embedded_open(const char *path);
void embedded_close(EmbeddedLocal *handle);
/// This device's person. Zero for a null handle.
int64_t embedded_me(const EmbeddedLocal *handle);

char *embedded_lists(const EmbeddedLocal *handle);
char *embedded_make_list(const EmbeddedLocal *handle, const char *name);
char *embedded_rename_list(const EmbeddedLocal *handle, int64_t id, const char *name);
char *embedded_delete_list(const EmbeddedLocal *handle, int64_t id);

char *embedded_items(const EmbeddedLocal *handle, int64_t list_id);
/// `uuid` may be NULL, for a caller that has not already drawn the row.
char *embedded_add(const EmbeddedLocal *handle, int64_t list_id, const char *line,
                   const char *uuid);
char *embedded_set_done(const EmbeddedLocal *handle, int64_t item_id, bool done);
/// `unit_id` of zero means none: C has no optional, and the units are counted from one.
char *embedded_update_item(const EmbeddedLocal *handle, int64_t item_id, const char *name,
                           double amount, int64_t unit_id);
char *embedded_delete_item(const EmbeddedLocal *handle, int64_t item_id);
char *embedded_clear_done(const EmbeddedLocal *handle, int64_t list_id);

/// Brings a device's old cache across. Answers {"ok": <items brought>}.
char *embedded_import(const EmbeddedLocal *handle, const char *everything_json);

char *embedded_units(const EmbeddedLocal *handle);
/// The categories in this list's order.
char *embedded_tags(const EmbeddedLocal *handle, int64_t list_id);
char *embedded_tags_on(const EmbeddedLocal *handle, int64_t item_id);
/// The order as a JSON array of tag ids: `[5, 3, 9]`. One fact, applied at once.
char *embedded_set_tag_order(const EmbeddedLocal *handle, int64_t list_id,
                             const char *tag_ids_json);
char *embedded_create_tag(const EmbeddedLocal *handle, const char *name, const char *emoji);
char *embedded_update_tag(const EmbeddedLocal *handle, int64_t id, const char *name,
                          const char *emoji);
char *embedded_delete_tag(const EmbeddedLocal *handle, int64_t id);
char *embedded_attach_tag(const EmbeddedLocal *handle, int64_t item_id, int64_t tag_id);
char *embedded_detach_tag(const EmbeddedLocal *handle, int64_t item_id, int64_t tag_id);

char *embedded_history(const EmbeddedLocal *handle, int64_t list_id);
/// `query` may be NULL or empty, which asks for the most recent rather than a match.
char *embedded_suggestions(const EmbeddedLocal *handle, int64_t list_id, const char *query);

EmbeddedWatcher *embedded_watch_list(const EmbeddedLocal *handle, int64_t list_id);
/// Brings a device's old cache across. Answers {"ok": <items brought>}.
char *embedded_import(const EmbeddedLocal *handle, const char *everything_json);

char *embedded_units(const EmbeddedLocal *handle);
/// The categories in this list's order.
char *embedded_tags(const EmbeddedLocal *handle, int64_t list_id);
char *embedded_tags_on(const EmbeddedLocal *handle, int64_t item_id);
/// The order as a JSON array of tag ids: `[5, 3, 9]`. One fact, applied at once.
char *embedded_set_tag_order(const EmbeddedLocal *handle, int64_t list_id,
                             const char *tag_ids_json);
char *embedded_create_tag(const EmbeddedLocal *handle, const char *name, const char *emoji);
char *embedded_update_tag(const EmbeddedLocal *handle, int64_t id, const char *name,
                          const char *emoji);
char *embedded_delete_tag(const EmbeddedLocal *handle, int64_t id);
char *embedded_attach_tag(const EmbeddedLocal *handle, int64_t item_id, int64_t tag_id);
char *embedded_detach_tag(const EmbeddedLocal *handle, int64_t item_id, int64_t tag_id);

char *embedded_history(const EmbeddedLocal *handle, int64_t list_id);
/// `query` may be NULL or empty, which asks for the most recent rather than a match.
char *embedded_suggestions(const EmbeddedLocal *handle, int64_t list_id, const char *query);

EmbeddedWatcher *embedded_watch_lists(const EmbeddedLocal *handle);
/// BLOCKS until something changed. Answers {"list": 4} or {"lists": true}, or NULL when
/// the watch has ended. Call it on a thread of your own.
char *embedded_next_change(EmbeddedWatcher *watcher);
EmbeddedStopper *embedded_watcher_stopper(const EmbeddedWatcher *watcher);
/// Ends the watch, from any thread. Safe to call twice.
void embedded_stop(const EmbeddedStopper *stopper);
void embedded_stopper_free(EmbeddedStopper *stopper);
/// Stop it before freeing it: freeing a watcher a thread is parked in is a
/// use-after-free, and nothing on the Rust side can prevent that.
void embedded_watcher_free(EmbeddedWatcher *watcher);

/// Hands back any string this header's calls returned. Not free().
void embedded_free(char *answer);

#endif
