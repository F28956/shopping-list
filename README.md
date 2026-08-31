# Shopping list

A shopping list for a phone, a watch, a Mac, an Android handset and any browser,
running against a server you own. One set of rules about what a list *means*, written
once in Rust, reached four ways.

Everything here is in one repository on purpose. The rules that decide what `2 kg
apples` parses to, which category an item files under and who may read a list are not
duplicated per client — they are one crate, `domain`, which the server runs and which
the phones now link directly. Two earlier attempts at "share the rules by writing them
twice" drifted within a week; the history is in
[docs/architecture.md](docs/architecture.md).

## The shape of it

```
  iOS / macOS / watchOS ─── HTTP bearer ──┐
  Android ──────────────── HTTP bearer ───┼─→  /api/*  ─┐
  browser ──────────────── cookie ────────┴─→  /*      ─┴─→  domain::service ─→ SQLite
                                                             (owns authorization)
```

One executable serves the browser UI and the JSON API on one listener. The browser
reaches the service layer in-process rather than over HTTP; the native clients go over
the network. Every rule about who may see or change what lives in `domain::service`, so
a phone gets the same answer as a browser because it asks the same question.

The newer move is that a device can stop asking over the network at all: `web/embedded`
compiles `domain` for iOS and Android, so a handset kept to itself runs the same crate
over the same schema in its own process. That work is partly built — see each client's
README for what is wired today.

## Where things are

| Directory | What it is | README |
|---|---|---|
| `web/` | The Rust workspace: server, JSON API, browser UI, the domain, and the device build of it | [web/README.md](web/README.md) |
| `ios/` | The Apple clients — iPhone, Apple Watch and a native Mac app, sharing most of their code | [ios/README.md](ios/README.md) |
| `android/` | The Android client, Kotlin and Compose | [android/README.md](android/README.md) |
| `site/` | The public site: what it is, how to install it, privacy, support | [site/README.md](site/README.md) |
| `ops/` | Running a server — backups, systemd, TLS | [ops/README.md](ops/README.md) |
| `release/` | Building signed installables for all four platforms | [release/README.md](release/README.md) |
| `docs/` | Design and reasoning, as opposed to instructions | below |
| `branding/` | The logo and the store icons |  |
| `reference/` | `reference.json` — the seeded units and tags, generated from the migrations and shipped into the clients |  |

`reference.json` is worth a line. It is generated from `web/domain/migrations` rather
than written by hand, and `domain::reference::the_seed_and_the_file_agree` fails if the
two ever part company. Both clients bundle this one file — the Apple targets list it as a
resource, Gradle copies it into the APK's assets — so a device that has never reached a
server still knows what a kilogram is, **and knows it by the server's own ids**, which is
what lets something added offline arrive measured.

## Getting it running

The server first, because every client wants an address to point at:

```sh
cd web
cp -n .env.example .env      # then fill in DATABASE_URL, GOOGLE_CLIENT_ID, ALLOWED_EMAILS
cargo run -p server
```

It binds `0.0.0.0` and logs the address a device on the same network can use.
`localhost` on a handset is the handset, which is the first thing to get wrong.

Then whichever client you want — [ios](ios/README.md#building),
[android](android/README.md#running-it) — each of which needs a little
platform-specific setup that only you can do, and each README says which.

## The design docs

These carry the reasoning. Where a README says *what to type*, these say *why it is
that and not something else*.

| Doc | Question it answers |
|---|---|
| [architecture.md](docs/architecture.md) | Why one process, three transports, and authorization in the service layer |
| [configuration.md](docs/configuration.md) | Who decides which server a client talks to, and who is let in |
| [offline.md](docs/offline.md) | What a device does with no signal — the cache, the outbox, and the merge rules |
| [encryption.md](docs/encryption.md) | What is encrypted, and what deliberately is not |
| [self-hosting.md](docs/self-hosting.md) | Why distribution avoids the stores, and what that costs |
| [tls.md](docs/tls.md) | Terminating TLS in this process, and rotating certificates without a restart |
| [review.md](docs/review.md) | A standing list of known problems in the Apple clients, struck through as they are fixed |
| [plan.md](docs/plan.md) | What is being built next, and in what order |

## Testing

The server's suite is the large one and runs with `cargo test` from `web/`. It includes
request-level tests that drive the real composed router, because the boundary between
the cookie-authenticated web routes and the bearer-authenticated API ones is a security
property rather than plumbing — and mounting an API route inside the session layer is a
one-line mistake with no visible symptom.

The Apple UI tests are slow and are not part of a routine check; run them deliberately.
