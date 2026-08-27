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

/// Ranks remembered names against something part-typed. Takes and returns JSON; see
/// the Rust side for the shape. The answer is yours, and goes back to `quickadd_free`.
char *quickadd_suggest(const char *input);

/// What a typed line should do to a list: which unit, which row, and whether a
/// crossed-off one comes back. JSON in, JSON out; see the Rust side for the shape.
/// The answer is yours, and goes back to `quickadd_free`.
char *quickadd_resolve(const char *input);

#endif
