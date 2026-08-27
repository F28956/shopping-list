# What to build, and in what order

A plan, not a design. Everything it sequences is decided somewhere else:
[offline.md](offline.md) for what already works without a connection,
[configuration.md](configuration.md) for the address and admission,
[tls.md](tls.md) for HTTPS, [self-hosting.md](self-hosting.md) for the store and the
threat model, [encryption.md](encryption.md) for the keys.

This exists because those five documents each end with their own ordering, and read
together they do not agree about what comes first.

## Where this stands

**Built.** Offline reading, client-minted identity, an outbox on both native clients
covering every operation but tags, `POST /api/sync` with per-operation outcomes, Sign
in with Apple on the Apple clients traded for a server-issued session, and one person
signing in with more than one provider.

**Designed and not built.** The runtime server address, admission and ownership as
data, TLS, and everything in [encryption.md](encryption.md).

**Neither, and small.** Tags offline. The "what changed" summary after a long time
away — [offline.md](offline.md)'s step 6, half of which exists.

One correction owed: [offline.md](offline.md)'s "Deliberately not doing" says offline
sign-in is impossible because a token expires in an hour. That stopped being true when
the session exchange landed — a token is now a keychain read good for ninety days, and
the only thing still impossible is the *first* sign-in on a device that has never seen
the server.

## The finding that decides the order

The obvious reading is that there are two competing programmes: make it hostable and
shippable, or make it encrypted. That reading is wrong, and it is worth being explicit
about why, because it changes the sequence.

**The self-hosting work is a prerequisite for the encryption work, not a rival to
it.** [self-hosting.md](self-hosting.md)'s S1 — a list that can be made and used with
no server — is the same change [encryption.md](encryption.md) needs: clients that are
authoritative rather than mirrors. Admission and ownership are untouched by
encryption. TLS is untouched. Account deletion changes meaning under encryption
(crypto-shredding) but still has to be wired either way.

So there is one line of work, not two, and the encryption is the far end of it. That
matters because it means nothing in the first half is thrown away by the second — with
three exceptions, below.

## What to stop investing in

Three things are demolished by [encryption.md](encryption.md), and every hour spent on
them between now and then is an hour spent twice.

* **The server-rendered web UI.** htmx renders lists the server can read. Freeze it:
  fix what breaks, add nothing.
* **The resource routes** — `GET /api/lists/{id}/items` and friends, with their
  ordering and paging. They become a log fetch. They stay working; they do not get
  richer.
* **Anything server-side that reads content** — sorting by `done_at`, grouping by
  tag, the quick-add parser. These move to the clients. Do not optimise them, and do
  not add to them.

Everything else in the codebase survives.

---

## P0 · The afternoon

Cheap, unrelated to each other, and all of them reduce a risk that exists today.

1. ~~**Prune `app_sessions`.**~~ **Done.** Expired rows were never deleted; the idle
   rule only applied on read. One `housekeeping` task in `main`, which is where any
   future retention goes so that "what does this server delete, and when" has one
   answer in one place.
2. ~~**Encrypted backups**, key held off the machine.~~ **Done.** `ops/backup.sh`
   writes to an `age` recipient, so the server cannot read what it just wrote;
   `ops/restore.sh` opens one on a machine that holds the private half, checks every
   page, and prints the counts — because a backup nobody has opened is a belief.

*A third item was planned here and turned out to exist:* `item_history` is capped at
`MAX_ENTRIES` and trimmed by `history::Entry::prune` on every write. Nothing to do.

**Done.** All of it was worth doing even though the plan carried on.

## P1 · Somebody else can run it

The server becomes a thing you can hand to a person who is not you.

1. ~~**TLS**~~ **Done.** Both arrangements, neither assumed: `TLS_MODE` is `off`
   (behind a proxy), `files` (PEM somebody else obtained) or `acme` (ordered and
   renewed in the process). `off` stays the default, and the operator guide leads
   with the proxy — which settles T1 versus S4 as emphasis rather than capability.
2. **The address at runtime** — [configuration.md](configuration.md) C1–C7. The
   single change that makes one binary serve everybody. Client work, on both
   platforms, and the only part of P1 that is not the server.

   *The mechanism is in place on iOS; the screens are not.* `GET /api/server` names
   the software, its version and whether it is open, closed or unclaimed.
   `ServerAddress` normalises what somebody types, which is where C3's trap lives.
   `ServerDirectory` stores it, asks a candidate whether it is this software, and
   says when the server changed — which is the cue for C4. `Config.apiBaseURL` reads
   it, so nothing is pointed anywhere by a build setting any more.

   ~~The screens.~~ **Done.** The address is asked for before sign-in (C1), changing
   it clears the device (C4), the watch is told in the same message as its token (C5),
   and a copied share link offers its origin (C7) — on both platforms. C6 is the
   cleartext policy, refused in release builds and permitted in debug, in the address
   parser and in Android's network security config, which have to agree.
3. ~~**Admission and ownership as data**~~ **Done.** A1–A7: rows read on every
   request, `ALLOWED_EMAILS` demoted to a seed, a claim code printed at boot, and the
   last-owner rule in the service layer with tests from three directions.
4. ~~**Account deletion**~~ **Done.** `DELETE /api/me`. A shared list changes hands
   rather than cascading away, because the person left behind did nothing; the
   address is forgotten too, since somebody asking to be erased asked for that;
   and the last owner of the server is refused, which is A5 arrived at a third way.

**Done.** At the end of P1 a friend can run their own server and
install the app from a build you hand them. No store, no encryption, no rewrite. If
the plan stops here it has delivered the thing that started this conversation.

## P2 · Somebody else can install it

1. **The owner's screens.** P1 gave the server `/api/admissions` and `/api/server`
   and gave the clients nothing, so an owner can admit somebody over HTTP and nowhere
   else. That is P1 work counted as done because the server half was; it is written
   down here rather than left implicit. It also earns its place in P2 on its own
   terms: a reviewer who opens an app and finds a working administration screen is
   looking at an app with substance, which is the 4.2 argument again.
2. **Local-only lists** — S1. The largest item in the first half, and the one that
   decides App Store review. Also the first step of P3, which is the argument for
   doing it here rather than arguing about 4.2 with a demo instance.

   *The server half is done*: `make_list` is a sync operation, so a list started with
   no signal arrives with its items when one turns up. What is left is the clients —
   letting a list be made before there is anywhere to send it, which means the caches
   keying off `uuid` where they use the server's integer `id`.
3. **ATS** — S4's client half. Small, and currently a silent failure for anybody
   typing an address that is not `localhost`.
4. **The demo instance and the listing** — S5 and S6.

**Off-ramp:** TestFlight needs 1 to 3 and none of 4. If the store turns out not to
be worth a permanent demo server, stop after item 2 and distribute that way — which
[self-hosting.md](self-hosting.md)'s Open already suspects is the right answer.

## P3 · The clients own the merge

The largest phase in the plan, the one no document names as a milestone, and the one
that decides whether encryption is affordable at all. It is worth doing on its own
terms: it is what makes tags-offline and the last of [offline.md](offline.md) fall
out, and it removes logic that is currently written twice.

1. **The spike.** See below. Timeboxed, and it decides the shape of everything after.
2. **The quick-add parser**, from Rust into both clients, deleting the Swift and
   Kotlin implementations of it. Small, real, and both test suites stay as they are.
3. **The merge rules** — `service::sync` evaluated client-side against the sequence
   the server assigns. The thirteen tests in `sync_tests.rs` come along and are what
   says it still works.
4. **Tags offline**, which stops being a special case once the rules are local.
5. **Step 6**, the "what changed" summary.

**Off-ramp:** at the end of P3 the app is better offline than it is today and nothing
is encrypted. Every step is independently useful, and stopping here wastes nothing.

## P4 · The lists are sealed

[encryption.md](encryption.md)'s own ordering, unchanged, because it was written to be
abandonable at each step:

1. Keys generated and stored, used for nothing — K1, K3.
2. Pairing and platform sync, still encrypting nothing — K5.
3. Sealing operations, single-member lists only.
4. Sharing by invite — K4.
5. Epochs and rotation — K7.
6. Recovery — K10, K11.
7. The panic button — K8.

Steps 1 and 2 change nothing anybody sees and are how you find out what the rest
costs.

**Off-ramp:** after step 3, one person's own lists are sealed and sharing is not. That
is a coherent product.

## P5 · The browser comes back

A PWA with `domain` compiled to WASM, holding a key. Unbounded, last, and possibly
never — [encryption.md](encryption.md)'s Open already asks whether a browser should be
a client at all once it cannot keep a key across a cleared cache.

---

## The decision that gates P3

**Does the shared core own storage, or is it pure?**

*Owns storage:* `domain` carries SQLite through `rusqlite`, and GRDB and Room are
deleted. One implementation of everything, and a rewrite of the storage layer on both
platforms — including the caches and outboxes that were written recently and work.

*Pure:* the shared core takes state and an operation and returns new state. Storage
stays GRDB and Room. Nothing that works today is touched.

**Take pure.** The rules are the part that must not diverge; the storage is the part
that is already written, already tested, and platform-shaped for good reasons. Owning
storage buys uniformity nobody is asking for at the price of the only code in this
project that has been through a real conflict matrix on two handsets.

**The spike that proves it** is P3's item 2, and it should be run before committing to
any of P3: compile `domain`'s line parser for iOS and Android, call it from Swift and
Kotlin, and delete the two native implementations. It is small, it removes duplication
that exists today, and it exercises every part of the toolchain the rest depends on —
the build, the bindings, the string marshalling, the CI. If that is miserable, P4 is
not affordable and the plan ends at P3.

## Risks worth naming

**The spike fails or is too heavy.** Then the merge rules would have to be written
three times to encrypt anything, and encryption should not be attempted. This is the
risk the spike exists to find early, and it is cheap to find.

**Recovery proves unusable.** [encryption.md](encryption.md)'s K11 is a phrase people
lose. If the honest answer turns out to be "lists only you can see are gone", that may
be a product nobody wants, and it is better discovered at P4 step 1 than at step 6.

**App Review rejects anyway.** 4.2 is a judgement call and S1 improves the odds
without guaranteeing them. The mitigation is that TestFlight is a complete answer and
P2's off-ramp reaches it.

**The web UI.** P5 is genuinely unbounded, and freezing the web UI at P0 means living
with it frozen for however long the middle takes. That is the cost of the whole plan
that is easiest to underestimate.

**Attrition.** This is evenings. The plan is ordered so that stopping is always a
decision rather than an accident, which is the only real defence.

## How big this actually is

P0 is an afternoon. P1 is weeks and is mostly designed already. P2 is weeks, with S1
the bulk of it. P3 is months. P4 is months and touches everything. P5 is a rewrite of
a client.

The honest summary is that P0 through P2 is a finishable piece of work with a clear
end, and P3 onwards is a second version of the application. They are sequenced
together because the order is the same either way — but they should be decided
separately, and P2's off-ramp is where that decision belongs.
