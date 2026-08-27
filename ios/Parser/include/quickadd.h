// The Rust parser, as C sees it.
//
// Hand-written rather than generated. It is two functions that have to keep agreeing
// with `web/quickadd-ffi/src/lib.rs` for ever, and a generator would be a build-time
// dependency and a second thing to install for the sake of eleven lines.
#ifndef QUICKADD_H
#define QUICKADD_H

/// Reads `line` against the unit names in `units_json`, and answers with JSON:
/// `{"name": "apples", "amount": 2.0, "unit": "kg"}`. `unit` is null when the line
/// named none. Never returns NULL.
///
/// The answer is yours, and must go back to `quickadd_free`.
char *quickadd_parse(const char *line, const char *units_json);

/// Hands back what `quickadd_parse` returned. NULL is ignored.
void quickadd_free(char *answer);

#endif
