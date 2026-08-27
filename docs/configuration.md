# The server you point at, and who it lets in

A design, not an implementation. Nothing here is built yet.

Two changes, and they are the same change seen from two ends. A self-hosted app has
to ask **which server**, because the answer is different for everybody; and a
self-hosted server has to answer **which people**, because nobody else can answer it
for you. Today the first is baked into the binary at build time and the second is
baked into the environment at boot, and neither can be changed by the person the
decision belongs to.

```
  first launch          first sign-in           afterwards
  ────────────          ─────────────           ──────────
  ask for the           the first person        the owner admits
  server address   ──→  through the door   ──→  everybody else
  and prove it              owns it             by address
```

## Where this stands today

**The address is a build constant.** iOS reads `API_BASE_URL` from `Info.plist` and
falls back to `http://localhost:8080` (`ios/Shared/Sources/Config.swift`); Android
reads `BuildConfig.API_BASE_URL` (`android/…/data/Api.kt`). A build that ships to a
store is a build pointed at whatever host the person who compiled it typed. For an
app whose whole premise is that you run the server, that is not a limitation to work
around later — it is the reason the app cannot be shipped at all.

**Admission is an environment variable.** `ALLOWED_EMAILS` is parsed once at boot into
`Admission::{Anyone, These}` and lives in `Ctx`. `identity::from_claims` checks it
*before* the user row is written, so a stranger who tries the door leaves nothing
behind, and `identity::from_session` re-checks on every request, so removing an
address ends that person's session immediately rather than whenever their cookie
happens to expire. Both properties are worth keeping. What is wrong is only where the
list lives: changing it is a redeploy, and the person who runs the server is not
always the person holding a shell.

---

# Part one · The server address

## C1 · The address is asked for before sign-in, never after

Signing in produces a token for a particular audience and then sends it somewhere.
There is no sensible order in which the app authenticates first and discovers the
destination second — and the most common refusal a new person meets, `NotAdmitted`,
is an answer that only a server can give. So the first screen of a fresh install asks
for an address, and the sign-in button does not exist until there is one.

## C2 · An address is validated by asking it, not by matching it

A regex proves the string is a URL. It does not prove there is a server there, that
it is *this* server, or that TLS will negotiate. All three fail in ways a person can
fix, and they deserve different sentences on screen.

`GET /healthz` exists and returns `ok`, which is not enough: so does every other
health endpoint on the internet, and pointing the app at an unrelated service would
succeed and then fail confusingly at the first API call. This wants a small
unauthenticated route that names the software:

```
GET /api/server  →  200 {"name": "shopping-list", "version": "1.4.0", "admission": "closed"}
```

The client accepts an address when `name` matches and refuses otherwise. `version`
lets a client say "this server is older than this app" rather than misreporting a
missing route as a network failure. `admission` lets the sign-in screen say whether
this server is open or whether you will need to be let in — which turns the most
confusing refusal in the product into something the person was warned about.

## C3 · Store an origin, and nothing else

Normalise on entry to `scheme://host[:port]`, and refuse a path, query or fragment
rather than silently dropping them.

This is not tidiness. iOS builds every request with
`URL(string: path, relativeTo: baseURL)`, which resolves against the base's *directory* —
so `https://example.com/lists` as a base silently loses `/lists` when a relative path
is appended, and `https://example.com/lists/` does not. A person pasting the address
out of their browser's location bar will paste a path. Deciding the shape once, at the
point of entry, is the only place that trap can be closed for every call site at once.

Accept and repair the obvious cases: add `https://` when no scheme is typed, strip a
trailing slash, lowercase the host.

## C4 · Changing the address throws away everything local

Sign the person out, discard the credential, and clear the cache that
`ios/Store/Sources/Cache.swift` and `android/…/data/Cache.kt` keep.

Not a precaution — a correctness requirement. Those caches hold rows keyed by ids and
UUIDs minted by the old server, and history and suggestions belong to an account on
it. Carrying them across would show one server's lists under another server's name.
The settings screen says what it is about to do and asks once.

## C5 · Where it lives on each platform

| Client | Stored in | Reached by |
|---|---|---|
| iOS / iPadOS | `UserDefaults` in the shared app group | the app, and any extension |
| macOS | the same app group | the Mac app |
| watchOS | pushed over WatchConnectivity | never typed on the watch |
| Android | `DataStore` preferences | the app |

The watch is the case worth designing rather than discovering. It already asks the
paired phone for its credential; the address has to travel the same way, in the same
message, or the watch will hold a token for a server it cannot name. A watch that has
never heard from its phone shows "Open Shopping List on your iPhone" — not an address
field, because entering a URL on a watch is not a thing to ask of anybody.

## C6 · HTTPS in release builds, cleartext only in debug

Today Android permits cleartext for exactly one host, `10.0.2.2`, via
`network_security_config.xml`, because that is the emulator's view of the development
machine. A runtime address means the app can be pointed anywhere, and the choice is
between shipping a blanket cleartext permission and refusing plain HTTP.

Refuse it. Release builds accept `https://` only, and the address screen says so when
someone types `http://`. Debug builds keep the cleartext exception for local work.

This has a cost and it should be stated rather than discovered: **it makes TLS a
prerequisite for self-hosting**, which the server does not serve today — see
`architecture.md`'s Open section. A home server on a private network is exactly the
case that hurts. The alternative, allowing arbitrary cleartext in a store build, means
every user's grocery list and bearer token travel in the clear on whatever café Wi-Fi
they are on, and is the kind of thing store review asks about.

## C7 · A share link already carries an address

`/join/<token>` links are parsed today (`ios/ShoppingListTests/JoinLinkTests.swift`),
and a share link is the ordinary way a second person arrives — often on a phone with
no app on it yet. That link names its own origin.

So: a join link offers its origin as the server, with the address shown and confirmed
rather than assumed. It turns the worst first-run experience in the product —
"somebody sent me a list and the app is asking me for a URL" — into one tap.
Confirmed, not silent: a link is a bearer credential from an untrusted sender, and
pointing an app at a host because a message said so is not something to do without
showing the host.

**Built, and by a different route than this said.** "Opening a join link" is not
available to a self-hosted app: a universal link matches an *associated domain* baked
into the app at build time, and every self-hoster's domain is different — so there is
no domain to associate and a link can never open this app. The clipboard is the only
route it has, behind an explicit "I have a share link" rather than a silent read on
appear. iOS then asks its own paste permission, which is the person consenting to
exactly this and is worth keeping rather than designing around.

## What the screens say when it fails

| What happened | What it says |
|---|---|
| Nothing answered | Cannot reach that address. Check it, and check you are on the same network as the server. |
| Answered, but not this software | Something is running there, but it is not a Shopping List server. |
| TLS failed | That server's certificate could not be verified. |
| `http://` in a release build | Addresses must start with `https://`. |
| Server older than the app | That server is running version 1.2 and this app needs 1.4. |

---

# Part two · Who may sign in

## A1 · The first person through the door owns the server

An empty server admits its first caller and makes them the owner, in the same
transaction that creates their user row and conditional on there being no user rows —
`INSERT … WHERE NOT EXISTS (SELECT 1 FROM users)`, not a read followed by a write.
SQLite's single writer makes the race cheap to lose safely, but it does not make a
read-then-write correct, and two people opening the browser at once is exactly how a
home server's first minute goes.

After that the server is closed: everyone else is a stranger until the owner names
them.

## A2 · The bootstrap window is the risk, and it needs an answer

Between the moment the server starts and the moment somebody claims it, **anyone who
can reach the port and holds a Google account becomes the owner**. On a machine
exposed to the internet that is a land grab waiting to happen, and the person it
happens to gets no warning at all — they will simply be refused from their own
server.

Three ways to close it:

1. **Seed from `ALLOWED_EMAILS`.** If it is set, only a listed address may claim. Two
   lines of code, since the parsing already exists — but it needs an environment
   variable, which is the thing this design is trying to stop requiring.
2. **A one-time claim code**, generated on first boot when no user exists and written
   to the log: `no owner yet — claim this server with code 7QK4-2M8P`. The claim screen
   asks for it. Expires when used.
3. **Accept the window** and document that the port should not be exposed until the
   server is claimed.

**Take (2), and keep (1) as a fallback.** A self-hoster starts the process and then
opens a browser, so they have the log in front of them; it needs no configuration, it
works for a packaged install where nobody is setting environment variables, and it is
the only one of the three that is safe when the port is already public.

## A3 · Admission is a list of addresses; ownership is a flag on a user

They are two tables because they answer at two different times. An owner admits
somebody who has never signed in and has no user row to flag — an address is the only
handle that exists before first contact.

```sql
CREATE TABLE admitted_emails (
    email      TEXT PRIMARY KEY,   -- normalised lowercase, as Admission already compares
    user_id    INTEGER REFERENCES users(id) ON DELETE SET NULL,  -- bound on first sign-in
    added_by   INTEGER NOT NULL REFERENCES users(id),
    added_at   TEXT NOT NULL,
    note       TEXT                -- "mum", so a list of addresses stays readable
);

ALTER TABLE users ADD COLUMN is_owner INTEGER NOT NULL DEFAULT 0;
```

`user_id` is the part that is easy to leave out and expensive to add later. **A Google
address is not stable; the `sub` is** — the schema already treats `sub` as the identity
key and folds nothing about it, precisely because folding would merge two people. If
admission were checked by address forever, somebody changing their Google email would
be locked out of a server holding their own lists. So: check by address until a user
exists, bind the row to that user on first successful sign-in, and check by `sub`
after that. The address stays as the label a person reads.

## A4 · Removal still takes effect on the next request

`identity::from_session` re-checks admission on every request and returns signed-out
rather than forbidden, so a removal heals itself: the person lands on the sign-in page
and signing in again is what tells them no. `identity::from_claims` does the same for
bearer clients. Neither behaviour changes — only where `Admission` is read from.

## A5 · The server cannot be left with nobody who can administer it

Two rules, enforced in `domain::service` per D1 and not in a handler:

- The last owner cannot be demoted.
- An owner's own admission cannot be removed while they are the last owner.

Both want a test in `authorization_tests.rs`. A server with no owner has no way back
that does not involve `sqlite3` on the host, and the person most likely to hit it is
the one tidying up their own address at two in the morning.

## A6 · "Anyone may sign in" stays sayable, as a setting

`Admission::Anyone` exists today and is a legitimate thing to want. It survives as a
stored setting rather than a magic string in an environment variable, it is logged
loudly on the way in exactly as `ALLOWED_EMAILS="*"` is today, and `GET /api/server`
reports it so the sign-in screen can stop promising a refusal that will not come.

## A7 · Reading admission per request, and the cache that must not be added carelessly

`Admission` moves out of `Ctx` and behind a load. `from_session` already fetches the
user on every request, so this is one more indexed read on a connection already in
hand; measure before optimising.

If it is ever cached, the write path has to invalidate it, or **removal stops being
immediate** — which is the single property that makes an admission list worth having.
A cache with a time-based expiry quietly converts "you are out now" into "you are out
within five minutes", and nothing in the test suite would notice. Whatever is built
here needs a test that removes an address and asserts the very next request is
refused.

## The wire

Refusal needs nothing new. `ServiceError::NotAdmitted` and the `not_admitted` reason
slug already exist, and every client already reads them — that is what commit 42f41ea
settled. What is new is management:

| Route | Who | Does |
|---|---|---|
| `GET /api/admissions` | owner | lists admitted addresses, with who added each and whether it has been used |
| `POST /api/admissions` | owner | admits an address |
| `DELETE /api/admissions/{email}` | owner | withdraws one |
| `POST /api/admissions/{email}/owner` | owner | promotes; `DELETE` demotes |
| `GET /api/me` | anyone signed in | gains `is_owner`, so a client knows whether to show the screen |

Owner-only is checked in the service layer, not in the route, and every one of these
gets a wrong-actor test.

## Migrating a server that already exists

An upgraded server has users already, so "first person through the door" cannot
apply — it would hand the server to whoever opened the app next.

On the first boot after the migration: seed `admitted_emails` from `ALLOWED_EMAILS`
if it is set, and make the **earliest-created user** the owner. Where `ALLOWED_EMAILS`
was `*`, record the open setting instead of a list. Afterwards the variable is read
only when the table is empty and there are no users, which is the fresh-install seed
path from A2.

## Open

**How many owners.** The design above is a binary flag, and promotion makes a second
owner equal to the first. A household with one person who cares about servers is the
common case and the flag is enough; the alternative — an owner who cannot be demoted
by anyone they promoted — is a hierarchy nobody asked for yet.

**Admission by link.** The invite machinery in `models/invite.rs` is per-list: hashed
token, one use, seven days. The same shape would work for admitting a person to the
server, and it would remove the need to know somebody's Google address before they
arrive. Deliberately out of scope here: it is a second bearer credential with its own
failure modes, and the address list is the thing that has to exist either way.

**Whether the address belongs in the same settings screen as the account.** Changing
the server signs you out; changing the account does not. Putting them together makes
the destructive one look routine.
