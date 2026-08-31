# The server, and the browser it serves

One Rust workspace. One executable that serves the browser UI and the JSON API from a
single listener, over a service layer that owns authorization — and, compiled a
different way, the same rules running inside a phone.

The reasoning behind all of this is [docs/architecture.md](../docs/architecture.md).
This is the map and the commands.

## Layout

Eight crates. Only `server` has a `main`; `api` and `web` are libraries exporting
`router()`.

| Crate | Contains | Depends on a database? |
|---|---|---|
| `parsing` | Reading what somebody typed: quick-add, fuzzy matching, suggestion and history ranking | no — standard library only |
| `domain` | Models, the service layer, `Actor`, `Ctx`, migrations, fixtures, reference data | yes |
| `observability` | The log somebody reads and the numbers something scrapes | no |
| `api` | Bearer auth, JWKS, JSON routes, `ServiceError` → status mapping | through `domain` |
| `web` | OIDC login, sessions, CSRF, maud pages, htmx fragments | through `domain` |
| `server` | Config, pool, migrations, tracing, TLS, one listener, router composition | yes |
| `quickadd-ffi` | `2 kg apples` over a C ABI, for callers that are not Rust | no |
| `embedded` | `domain` itself, built for iOS and Android | yes |

Three of those exist because of the phones, and each is worth a sentence.

**`parsing` is split out of `domain` so it can be compiled for a handset.** Everything
else in `domain` reaches a database, which means sqlx, which means tokio, which means
several megabytes of static library to answer "does `kg` name a unit". Keeping `parsing`
in a crate with no dependencies is what keeps it small, and a build failure is a better
guard than a comment asking people not to add one. `domain` re-exports it, so
`crate::quick_add` still resolves inside it.

**`quickadd-ffi` is deliberately tiny and deliberately C.** Every argument is a
nul-terminated string and the answer is JSON; there are no structs across the boundary
and no ownership rules to remember beyond "free what you were given". It exists so the
phones do not get a second opinion about what a typed line means.

**`embedded` links `domain`, rather than something like it.** A device kept to itself is
not this app with the server parts removed — it is the same app talking to a server that
happens to be in the same process. The device's database has `users`, `list_members` and
roles, with exactly one row in `users`, so adopting a real server later is a merge rather
than a migration. It builds `staticlib` for Apple, `cdylib` for Android, and `rlib` so
the Rust tests can link it; JNI needs its own entry points (`src/jni.rs`) because a JNI
symbol is mangled from its class and every string is a `jstring`, but everything beneath
them is shared with the C side.

### Inside `domain`

```
domain/
  migrations/        33 files. The schema, and the seeded units and tags.
  src/models/        Rows. Deliberately actor-agnostic: Note::get hands you anybody's note.
  src/service/       Every rule about who may do what. Takes an &Actor and enforces it.
    authorization_tests.rs   One file, so the coverage is countable.
```

The split between `models` and `service` is the load-bearing one. A row does not know
who is asking, so the models do not pretend to; every check lives in `service`, which is
what lets the browser call it in-process without skipping anything an HTTP handler would
have done.

It is called `domain` and not `core` because a package named `core` shadows the standard
library's, and `use core::…` then resolves ambiguously.

### Inside `web` and `api`

Both are shaped the same way — `lib.rs` exports `router()`, `routes/` or `pages/` holds
the handlers, `error.rs` maps `ServiceError` onto that transport's idea of a refusal.
They keep separate error types on purpose: not-signed-in is a redirect in a browser and
a `401` on the API, and the right answer differs by transport even though the underlying
fault was decided once.

`web` additionally holds `sessions.rs` (a hand-written SQLite session store),
`csrf.rs` (unsafe methods must say they came from here) and `base.rs` (`BASE_PATH`, so
the whole application can live under a path on a shared domain).

## Running it

```sh
cp -n .env.example .env      # DATABASE_URL, GOOGLE_CLIENT_ID, ALLOWED_EMAILS at minimum
cargo run -p server
```

Use an **absolute** `sqlite:///` path. A relative one is resolved against the current
working directory, and two different things resolve it at two different moments — the
sqlx query macros at compile time with cargo's CWD at the workspace root, and the binary
at run time against wherever you launched it. `create_if_missing` is off, so a wrong path
fails to start rather than silently serving an empty database.

Every setting is documented in `.env.example`, and the reasoning for the awkward ones is
in [docs/configuration.md](../docs/configuration.md).

### First time, and after a schema change

```sh
sqlx database create
sqlx migrate run
cargo sqlx prepare --workspace     # refreshes .sqlx/, which is committed
```

The `.sqlx` directory is checked in so the workspace builds without a database — which
is what makes the phone builds and CI possible.

### Tests

```sh
cargo test
```

The suite includes request-level tests that drive the real composed router. That is not
belt-and-braces: mounting an API route inside the session layer is a one-line mistake
with **no visible symptom** — the endpoint keeps working, it just also works for any
website that links to it. `server`'s tests drive a valid session cookie at an API route
and require `401`.

`AuthMode::TrustTheToken` makes the API testable without a real provider. It sits behind
a `test-support` feature enabled only from dev-dependencies, and a `compile_error!`
planted behind that feature proves it: the release build compiles, the test build does
not.

### Building it for a device

The client repositories drive this — `ios/Scripts` and `android/scripts/build-embedded.sh`
— but the crate being built is `embedded`, and it cross-compiles for
`aarch64-apple-ios`, `aarch64-apple-ios-sim`, `aarch64-apple-darwin` and both Android
targets without a line of change to `domain`.

## What to read next

* [architecture.md](../docs/architecture.md) — the decisions, numbered, with the
  reasoning and the mutations used to prove the tests notice.
* [ops/README.md](../ops/README.md) — backups, systemd, TLS, running it for real.
