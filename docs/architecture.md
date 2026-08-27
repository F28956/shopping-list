# Three transports, one process

One executable serves the browser, the HTTP API and — later — MCP, over a service
layer that owns authorization. The browser reaches the database without HTTP; iOS
keeps it.

**Status:** built and in use. 515 tests. The credential for non-browser clients is the
one open decision, and it blocks MCP.

```
  iOS ─────── HTTP bearer ──┐
  MCP ─────── HTTP bearer ──┼─→  /api/*   ─┐
  browser ─── cookie ───────┴─→  /*       ─┴─→  domain::service  ─→  models  ─→  SQLite
                                                (owns authorization)
```

## Layout

| Crate | Contains |
|---|---|
| `domain` | Models, service layer, `Actor`, `Ctx`, migrations, fixtures, quick-add parsing, suggestion ranking |
| `api` | Bearer auth, JWKS, JSON routes, `ServiceError` → status mapping |
| `web` | OIDC login, sessions, maud pages |
| `server` | Config, pool, migrations, tracing, one listener, router composition |

Outside the Cargo workspace, `ios/` holds a SwiftUI app over the same API — a client
and nothing more. Every rule about who may see or change what lives in
`domain::service`, so the phone gets the same answers as the browser because it asks
the same questions. See `ios/README.md`.

`api` and `web` are libraries exporting `router()`. Only `server` has a `main`.

It is called `domain` rather than `core` because a package named `core` shadows the
standard library's, and `use core::…` then resolves ambiguously.

## Decisions

### D1 · Authorization lives in the service layer

Every service function takes an `&Actor` and enforces access itself. Transports
authenticate — turning a bearer token or a cookie into an `Actor` — and never decide
what that actor may touch.

This is the load-bearing decision. The models are deliberately actor-agnostic:
`Note::get` will hand you anybody's note, because a row does not know who is asking.
With three transports, a check living in an HTTP handler is a check the browser path
skips. Without D1 the in-process shortcut is an authorization bypass with extra steps.

Three shapes of rule:

- **Owner-scoped** (lists, notes). The owner comes from the actor, never from an
  argument, so a caller cannot pass the wrong one.
- **Reached through an owner** (items, tagging). An item id is not a capability: the
  item's own row says nothing about who may touch it, so its list is consulted.
  `lists::owned` is the single place that rule lives — when `list_members` grows
  teeth, that is the one function that has to learn the difference between "owns" and
  "can see".
- **Shared reference data** (units, tags). Read by anyone, written only by
  `Actor::System`, which no request can produce. `kg` is not anyone's kilogram.

**Unauthorized reads as `NotFound`, never `Forbidden`.** `Forbidden` confirms the row
exists, which tells someone holding a guessed id something true about another
person's data. The distinction stays in the log line. A test asserts a forbidden note
and a missing note are indistinguishable.

### D2 · One listener, path-prefixed, boundary enforced by layering

`/api/*` is bearer-authenticated JSON, `/mcp/*` arrives later, everything else is the
cookie-authenticated web UI. One hostname, one certificate.

This moves the security boundary off the network and into the router. Same origin
means the browser **will** attach the session cookie to `/api/*`, including on
requests triggered by another site. Three rules make that harmless:

1. The session layer wraps the web router **only**, applied inside `web::router`
   before merging — a router already merged with the API's is no longer safe to wrap,
   and that is the last point at which it is still obvious.
2. API handlers authenticate from `Authorization: Bearer` and nothing else. A request
   carrying only a cookie is `401`, whatever the cookie says.
3. The cookie is `SameSite=Lax` — depth behind the first two, not instead of them.

> Mounting an API route under the session layer is a one-line mistake with **no
> visible symptom**: the endpoint keeps working, it just also works for any website
> that links to it. `server`'s tests drive a valid session cookie at an API route
> through the composed router and require `401`.

### D3 · The browser calls the service layer directly

No `reqwest`, no `api_base`, no token replay. A maud handler calls
`service::notes::list(&ctx, &actor, …)` and renders the result.

### D4 · One pool, because SQLite

SQLite is a single file with a single writer. Two processes on `data.db` means lock
contention and reliance on `busy_timeout`. One process owning the pool removes a class
of production problem that would not exist on Postgres — the strongest single argument
for the merge, and the reason MCP will be HTTP rather than a stdio subprocess.

### D5 · Sessions in SQLite

The store is hand-written. `tower-sessions-sqlx-store` is built against sqlx 0.8 while
this workspace is on 0.9, so its `Pool<Sqlite>` is a different type and cannot take our
pool; pinning the workspace back a major version to borrow eighty lines was the worse
trade. Expiry is enforced on load, not by a sweeper.

The session table is created by the store rather than by a `domain` migration: the
shape of a session row belongs to whichever transport keeps sessions, and neither the
API nor MCP does.

### D6 · The HTTP API stays under test on its own

Once the browser stopped using HTTP, only iOS exercises that layer. Every route has a
request-level test driving the real router in-process.

`AuthMode` makes that possible: how a bearer token becomes an identity is a value
rather than a hard-wired call. `AuthMode::TrustTheToken` is behind a `test-support`
feature enabled only from dev-dependencies, so a release build contains no such
variant — there is no flag that could switch it on, because there is no code for it to
switch on.

### D7 · Panics unwind

`panic = "abort"` is absent from the release profile. Sharing a process means aborting
would let a panic in one template handler take the iOS API down with it.
`CatchPanicLayer` turns it into a 500 on the offending request.

Note: profiles are only honoured at the workspace root. `api/Cargo.toml` used to carry
its own, silently ignored, and cargo warned about it on every invocation.

## The service layer

```rust
pub enum Actor {
    User(user::User),   // a signed-in person
    System,             // fixtures, migrations, maintenance; never from a request
}

pub struct Ctx { pub db: SqlitePool }

pub async fn rename(ctx: &Ctx, actor: &Actor, id: list::Id, name: list::Name)
    -> Result<list::List, ServiceError>
```

`ctx` first, `actor` second, then the operation's own arguments. Every function that
touches a scoped resource **loads it, checks the actor against it, then acts** — in
that order, with no shortcut for the "obviously mine" case.

There is no `Anonymous` variant: a request without a verified identity never gets an
`Actor` at all, so "not signed in" is a shape the service layer cannot be handed.

| `ServiceError` | api | web | mcp |
|---|---|---|---|
| `NotFound` | 404 | 404 page | tool error |
| `Conflict` / `InUse` | 409 | re-render with message | tool error |
| `InvalidInput` | 400 | re-render with field error | tool error |
| `Unauthenticated` | 401 | redirect to login | tool error |
| `Internal` | 500 + log | 500 page + log | tool error + log |

`web` keeps its own `AppError` rather than sharing the API's, because the right answer
differs by transport — not-signed-in is a redirect here and a 401 there. Both map the
same `ServiceError` underneath, so whose fault a failure was is decided once.

**Every scoped operation has a test where the actor is the wrong user and the expected
result is `NotFound`.** They live together in `service/authorization_tests.rs` so the
coverage is countable: an operation missing from that file has not been checked.

## Reference data

Units and tags belong to nobody and are written only by `Actor::System`, which no
request can produce — so a migration is the only way they arrive. That makes adding or
renaming one a schema change on purpose: renaming `kg` renames it on every list in the
system.

The API exposes them read-only, with no `POST`/`PUT`/`DELETE` at all. A write route
could only ever refuse, and a route that exists and always says no is worse than one
that does not exist. Tests assert `405`, so adding one later is deliberate.

The test `pool` fixture clears both tables after migrating: tests need control of their
own baseline, and the fixtures stamp `created_at` with staggered offsets that
production has no reason to carry.

## The features that shape the data

**Quick add** (`domain::quick_add`). One typed line becomes name, amount and unit —
leading number, then a unit if what follows names one, then the rest. Pure: it takes
the known unit names rather than a database, so its rules test without one. It lives
in the service layer rather than a transport so the browser and the API cannot drift
on what `2 kg apples` means.

**History** (`item_history`, `domain::history_rank`). What a person buys, keyed on
(user, normalised name), kept apart from the lists it was gathered on — deriving it
from live rows meant clearing a list erased it. It remembers the unit and the
category too, so a re-added item arrives measured and filed. Ranking is `uses` decayed
by a thirty-day half-life, computed in Rust: the obvious formula wants `exp()`, which
a bundled SQLite may not carry, and policy deserves tests that need no database.
Capped at 500 entries and forgettable one at a time.

**Client-minted identity** (`items.uuid`, `lists.uuid`). Every item and list carries
a UUID alongside its integer primary key. `id` stays the key and the foreign-key
target; `uuid` is what an operation names a row by, because a device with no signal
has to be able to queue "tick that off" behind "add that" before any server has
turned its counter. Minted by whoever creates the row — the model on the online path,
the device on the offline one — with a mint-if-missing trigger in the schema so no
row can exist unnamed. See `docs/offline.md`.

**`POST /api/sync`** (`domain::service::sync`). One route for everything a device did
while it could not reach the server: operations named by uuid, carrying the device's
own clock, applied in order, answered one at a time. Idempotent through
`applied_operations`; a refusal is data with a reason, not a status code that fails the
batch; the row each operation produced comes back so a device can learn the id of
something it created offline. Push only — the event streams remain how a client learns
to re-read.

**Working offline** (`Cache`/`Outbox` on both native clients). The last-loaded lists
and items are kept on the device, so the app never claims an emptiness it has not
verified, and changes made with no signal go into a durable queue that drains on the
next successful load. The cache is disposable; the queue is not, and shares its file,
so that file is migrated by hand. See `docs/offline.md` — `setDone` is the operation
that has an offline path today.

**Units are never hidden.** Every item carries one — `unit` is what an item added
without a measure is given, so that `milk` and `1 unit milk` are the same row rather
than two. It used to print as nothing, on the grounds that it said nothing a number did
not. It said one thing that mattered: that the row had a unit at all, which is how a row
that had genuinely lost one went unnoticed. All four surfaces now print it, and the rule
is applied on every write rather than only on the first.

**Category grouping.** `tags.sort_order` carries the order of a shop rather than the
alphabet — perimeter first, frozen late, shop names after everything describing a
department. Aisle numbers would be more precise and are deliberately not used: they
differ by branch and change without warning, while categories travel to any shop.

**htmx, without giving up plain HTML.** Every form keeps its `method` and `action`;
handlers branch on `HX-Request` and return either a fragment or the redirect they
always returned. The no-JavaScript path is not a claim — the page tests never send
that header, so they exercise it on every run.

## Security posture

- **Session cookie** is `HttpOnly`, `SameSite=Lax`, and `Secure` unless
  `SESSION_INSECURE` says otherwise. The safe answer is the one you get by not
  thinking about it.
- **Cross-site writes are refused.** `SameSite=Lax` withholds the cookie from
  cross-site POSTs, but it was the only defence and it stops applying the day someone
  sets `SameSite=None` or adds permissive CORS. Unsafe methods on the browser router
  must also say they came from here — `Sec-Fetch-Site`, or an `Origin` matching
  `PUBLIC_ORIGIN`. A request that says nothing is allowed: `curl` sends no `Origin`,
  and nothing that lacks a browser can be tricked into carrying someone else's cookie.
- **Content-Security-Policy forbids inline script and style outright**, plus nosniff,
  a referrer policy and frame refusal. The application was changed to fit: the
  stylesheet and the two `hx-on` handlers moved into served files, because a policy
  that has to allow `unsafe-inline` is decoration. A test asserts the markup contains
  nothing the policy would block.
- **Two providers, one person.** The Apple clients sign in with Apple; Android and the
  browser sign in with Google. `user_identities(provider, subject)` is who somebody is;
  `users.sub` is a record of how they first arrived, qualified by provider on anything
  created since — a subject is only unique within the provider that issued it, and that
  column is unique across the table. A new identity whose **verified** address matches
  an existing account joins it rather than making a second one. Unverified addresses
  never reach that check: matching on a claim nobody vouched for would be a way into
  somebody else's shopping.
- **Apple sends an address once and a name never.** Admission reads the address, so it
  falls back to the one stored against that identity rather than the one on the token —
  otherwise a person would be let in on their first sign-in and refused on every one
  after. The name comes to the *client* in the credential, not in the token, so an
  Apple-only account has none and `Person::shown` falls back to the address.
- **Admission is a different refusal from authorisation**, and the wire says which.
  `ServiceError::NotAdmitted` is "this account may not use this server", raised before
  any row is written for a stranger; `ServiceError::Forbidden` is "you may read this
  list but not change it". Both are 403 because both are, so the API body carries a
  `reason` slug and the clients read it. They shared a code once, and somebody signing
  in with an unlisted address was told, on a screen with no list on it, that they could
  read the list but not change it.
- **The test-only auth mode cannot ship.** Verified by planting a `compile_error!`
  behind its feature: the release build compiles, the test build does not.

## Testing

Mutation testing, not just green ticks. Each rule was broken to confirm the suite
notices:

| Mutation | Caught by |
|---|---|
| Drop an ORDER BY `CASE` arm | ordering + `every_field_changes_the_order` |
| Swap `LIMIT`/`OFFSET` | most of the paging suite |
| Stop normalising before writes | 20 tests across three models |
| Report every FK failure as `InUse` | 4 dangling-reference tests |
| Let a cookie authenticate `/api` | composed-router boundary tests |
| Drop `lists::owned`'s ownership check | 5 domain, plus api and web |

That last exercise found a hole in a **test** rather than the code: the API's
cross-user walk covered `GET`, `POST` and `DELETE` on an item and silently omitted
`PUT`. A list of verbs goes stale when a route is added; `Field::VARIANTS` solved the
equivalent problem for ordering, but there is no such trick for routes.

## Traps worth knowing

- **A `CHECK` on a nullable column flips sqlx's inference to NOT NULL**, and a
  `#[sqlx(transparent)]` newtype then decodes NULL as `Some(Email(""))` rather than
  `None` — silently. Every nullable column carries an explicit `?:` annotation.
- **`DATABASE_URL` is resolved against the current working directory**, by the query
  macros at compile time (CWD = workspace root) and by the binary at run time (CWD =
  wherever you launched it). Use an absolute `sqlite:///` path. `create_if_missing` is
  off, so a wrong path fails to start rather than silently serving an empty database.
- **SQLite's `lower()` and `COLLATE NOCASE` fold ASCII only.** Normalisation happens in
  Rust, where `to_lowercase` is Unicode-aware. The schema's `CHECK`s are a backstop
  against writers that bypass the model, not the enforcement point.
- **`ON DELETE RESTRICT` surfaces as SQLITE_CONSTRAINT_TRIGGER (1811)**, not the
  foreign-key code sqlx maps. A blocked delete and a dangling reference are different
  failures and must not collapse into one error.
- **A page that shows a prefix must say so.** Every list view reads `has_more` and
  admits to holding back; silently truncating makes missing rows look deleted. There
  is one `PAGE_MAX` and every caller reads it, because four caps drifting apart is how
  a list ends up cut at a number nobody chose.
- **A guard that never refuses is not a guard.** Units and tags are readable by any
  actor; that is written down rather than expressed as a function returning `Ok(())`,
  which reads like a check that exists.

## Clients, and how each proves who it is

Every client talks to the same JSON API and carries `Authorization: Bearer <token>`.
Two kinds of token reach it, and the bearer path accepts both.

A **provider ID token** is re-verified on every request: signature against that
provider's JWKS, issuer, expiry, and the `aud` claim against a list the server was
configured with. That works while the provider keeps the client supplied with a fresh
one, which Google's SDKs do.

A **session token** is one this server issued: opaque, ninety days of idleness,
revocable by deleting a row. It exists because Apple's identity token lasts about ten
minutes and has no silent refresh — as a bearer it would mean signing in six times an
hour. So the Apple clients trade theirs once at `POST /api/sessions` and hold what
comes back. The two are told apart by shape, not by trying both: sixty-four lowercase
hex characters is what the exchange mints and is not a shape any JWT has.

| Client | Where the token comes from | What the server needs |
|---|---|---|
| Browser | Google OIDC code flow, server-side | `GOOGLE_CLIENT_ID` |
| iOS / macOS | Sign in with Apple, traded for a session | `APPLE_BUNDLE_IDS` |
| watchOS | asks the paired phone over WatchConnectivity, then keeps it | nothing more |
| Android | Google Credential Manager, given the web client id as `serverClientId` | usually nothing |

Sign in with Apple has no separate client id: the audience of a native app's identity
token *is* the bundle identifier, which is why `APPLE_BUNDLE_IDS` holds bundle ids and
not something copied from a console. The phone and the Mac share one, deliberately —
one app to a person, one entry to configure.

The Android row is the one that surprises. Its OAuth client — registered against the
package name and the signing certificate's SHA-1 — is what lets Google attest the app;
it does not name the audience. Expecting it to, and adding it as a required audience,
would be configuring for a claim that never arrives.
`GOOGLE_ANDROID_CLIENT_ID` exists so that finding otherwise is a line of
configuration rather than a change to the server.

The watch is worth a line of its own. It has no sign-in and cannot have one, so it asks
the phone over a link Apple has already authenticated — and then keeps what it is
given, in its own keychain. Ninety days is long enough that a watch which has been near
its phone once goes on working in a shop with the phone left at home, which is what its
cache and its outbox were built for.

Reaching the server from a real device is the other half:

* it binds `0.0.0.0`, and logs the address a device on the same network can use —
  `localhost` on a handset is the handset, which is the first thing to get wrong;
* Android blocks cleartext HTTP from API 28. On a development network that is a
  `networkSecurityConfig` exception naming the host, which is the client's business;
  anywhere else it is a reason to serve TLS, which this does not yet do — see Open.

## Open

**The credential for MCP.** `POST /api/sessions` now issues exactly the kind of token
MCP wants — opaque, long-lived, revocable — but only in exchange for a provider token,
which means a person at a sheet. MCP has no SDK to lean on and nobody present to tap
anything, so what is still missing is a way to mint one out of band: a command that
prints a token, and a way to see and revoke the ones that exist.

**Profile editing.** `users::update_profile` exists and is tested but is not wired.
Authentication resolves the identity through `User::find_or_create` on every request,
which coalesces the provider's claims over what is stored — so a self-chosen name
would be overwritten by the next request. `a_login_overwrites_a_self_chosen_name`
asserts exactly that, so deciding which side wins will break something loud.

**Closing an account.** `users::close_account` cascades away every list, item and note.
An irreversible `DELETE` deserves a confirmation flow designed on purpose.

**Sharing.** `list_members` and `Role` map the schema; nothing operates on them.
Answering it means deciding what a `viewer` may do — and, separately, whose history a
shared list draws on. History is per-user by design: what you buy is yours, and
merging two people's habits would make both sets of suggestions worse.

### Working without the server

Every client today is online-only: reads keep the last-loaded list while the app
stays open, writes fail and are discarded, and a cold start with no connection
claims you have no lists at all. The design for fixing that — a local database,
an outbox of operations, and per-operation merge rules on the server — is in
[offline.md](offline.md). Nothing of it is built.

### Which server, and who it lets in

Every client is pointed at its server by a build constant — `Info.plist` on iOS,
`BuildConfig` on Android — so a build that reaches a store is pointed at whoever
compiled it. Admission is an environment variable read once at boot, so changing who
may sign in is a redeploy. Both are the same shape of problem: a decision that belongs
to the person running the server is currently made by the person building it. The
design for asking for the address at first launch, and for an owner who admits
everybody else, is in [configuration.md](configuration.md). Nothing of it is built.

### TLS

Nothing here serves HTTPS. On a laptop talking to its own simulators that costs
nothing; a phone on the same Wi-Fi already needs a cleartext exception, and anything
beyond this network needs certificates. The token in every request is a bearer
credential, so this is the gap that matters most once the address stops being
`localhost` — and configuration.md's C6, which refuses `http://` in release builds,
turns it from a gap into a precondition for shipping at all. The design for
terminating TLS in this process, obtaining certificates from Let's Encrypt and
rotating them without a restart — including what a custom listening port does and does
not change about validation — is in [tls.md](tls.md). Nothing of it is built.

## Recorded, not taken

The item-with-tags projection that predated this work paged by **keyset**
(`WHERE id > ? ORDER BY id LIMIT ?`) rather than by offset. Keyset paging does not
drift when rows are inserted mid-scroll and does not make the database count past the
rows it skips. The pattern here is offset paging throughout — `Paging`, `OffsetPage`
and every ordering test are built on it — and mixing the two would mean two answers to
the same question. Worth revisiting only if a list gets long enough to matter.
