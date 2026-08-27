# Shipping a server you do not run

A design, not an implementation. Nothing here is built yet.

The app reaches people through the App Store. Their shopping does not reach me. That
is the whole of it, and it has two audiences who want opposite things: a reviewer at
Apple who must be able to use the app in five minutes without owning a server, and a
person at home who wants a server nobody else can read — including me, and including
whoever ends up with the disk.

```
  the App Store          the person who runs it        me
  ─────────────          ──────────────────────        ──
  one binary,            their box, their              no server,
  no server in it  ──→   admission list,         ──→   no database,
  useful on its own      their key                     nothing to hand over
```

The last column is the point. "I cannot read your lists" is a stronger promise than
any privacy policy, it is the honest answer to the compliance questions in
[architecture.md](architecture.md)'s security posture, and it is only true if it is
true by construction.

## Where this stands today

**The address is a build constant and admission is an environment variable.** Both
are designed in [configuration.md](configuration.md) and neither is built. That
document is the reference for parts one and two of the work below; nothing here
supersedes it.

**There is no TLS.** [tls.md](tls.md) has the design. C6 there already decides that
release builds refuse cleartext, which makes this a prerequisite rather than a
nicety.

**`users::close_account` exists, is tested, and is wired to nothing** — see the
module note in `api/src/routes/me.rs`. That was a reasonable place to stop for a
personal server. It is a blocker for a public one, for two independent reasons given
in S3.

**Everything is stored in plaintext.** The server reads every item name, and so does
anybody holding the disk.

**What is already right**, and worth knowing before reading further, because these are
the parts that usually have to be torn out:

* **The change stream carries no content.** `/api/me/events` and a list's own stream
  send a nudge and nothing else; the client re-reads through the ordinary route. An
  encrypted design needs exactly that.
* **Identity is minted on the device.** `items.uuid` and `lists.uuid` mean a thing
  exists before any server has heard of it.
* **Every client already holds the whole list and an outbox.** The clients are not
  thin, and have not been since the offline work.
* **The Apple clients sign in with Apple.** A native token's audience is the bundle
  identifier, which is the same for every install — so any self-hosted server verifies
  it against Apple's published keys with no per-instance registration. Google would
  have needed every self-hoster to create their own OAuth client.

---

# Part one · Getting through review

Self-hosted clients are approved routinely — Nextcloud, Home Assistant, Immich,
Jellyfin. The path is well trodden, and it turns almost entirely on one guideline.

## S1 · The app must be useful before it has a server

Guideline 4.2 refuses apps that do nothing without infrastructure the reviewer cannot
reach. An app that opens on a server-address field is precisely that shape, and the
demo instance in S5 only argues the point rather than settling it.

**So a list can be made, filled and crossed off with no server at all**, and syncing
is something you add afterwards. That converts the review question from "is this
useful?" to "here is a working shopping list, and it also syncs", which is not a
question.

This is a smaller change here than it would be in most apps, because the offline work
already did the hard half: the cache is the client's source of truth, the outbox
tolerates arbitrary delay, and uuids mean identity does not wait for anybody. A
local-only list is a list whose operations have never been sent.

The one real cost is that the clients key off the server's integer `id` in places
where they would have to key off `uuid`, and a list adopted by a server later needs
its operations replayed rather than its rows rewritten. That is the same replay
`POST /api/sync` already performs, pointed at a list the server has never seen.

**Not built, and the largest single item in this document.**

## S2 · The address is asked for, never compiled in

Designed in [configuration.md](configuration.md), C1–C7. Nothing to add except that
S1 changes when it is asked: the address stops being the first screen and becomes a
setting somebody visits when they have a server to point at.

## S3 · Deleting an account is a button, not a support request

Two obligations meet at the same unwired function:

* **Guideline 5.1.1(v)** requires in-app account deletion from any app that supports
  account creation. A link to a web page does not satisfy it. That the account lives
  on somebody else's server does not exempt the app that made it.
* **GDPR Article 17** is the most frequently exercised right there is, and the person
  who has to honour it is whoever runs the server — so the app has to give them a way
  to trigger it.

`users::close_account` cascades away every list, item and note. The reason it was
never wired is sound and unchanged: an irreversible `DELETE` deserves a confirmation
flow designed on purpose. Design it.

## S4 · Two ways to get HTTPS, and both have to work

[tls.md](tls.md) already designs both halves of the server side: T1 puts rustls and
ACME in the process, and T6 and T7 cover living behind a proxy — `TLS_MODE=off` for a
proxy that terminates, `acme` for one that passes through, with a warning that an L7
proxy breaks the ALPN challenge and needs HTTP-01 instead.

So the capability is planned in both shapes. What the operator documentation should
lead with is a separate question, and worth deciding deliberately:

* **Behind a reverse proxy** is what most people running a home server already have.
  Caddy gets a certificate in three lines, it is where their other services already
  live, and it puts certificate renewal in a component whose whole job that is. It
  also means the app never touches port 80, never holds a private key, and can bind
  to localhost — which removes most of part three's exposure in one step.
* **TLS in the process** is what somebody with a bare VM and one binary wants, and
  the reason T1 chose it as the default: an app whose premise is "run this one thing"
  should not require a second thing before it works at all.

**These want to be documented as a recommended path and an alternative, not as two
equals.** A self-hoster reading two options with balanced prose picks neither. My
suggestion is to lead the operator guide with the proxy — it is the configuration
most people are already in, and the failure modes are somebody else's well-documented
ones — and present in-process TLS as the answer for a bare box, which is exactly the
audience T1 was written for. That is a change of emphasis from T1, not of capability,
and it belongs in [tls.md](tls.md) rather than being decided here.

Whichever leads, the documentation has to state the T6 trap plainly, because it is
the one that produces a confusing failure rather than an obvious one: a terminating
L7 proxy in front of a server also holding a certificate must use HTTP-01, because
the proxy answers the ALPN challenge itself and the order fails with nothing useful
said.

**The client half** is App Transport Security, and there is currently no
configuration at all — so the default applies, cleartext is refused, and the reason
nobody has noticed is that the simulator exempts `localhost`. The first person to
type `http://192.168.1.10` gets a silent failure.

Three options, in order:

1. **Require HTTPS.** Cleanest, matches C6, and pushes the problem to a place
   [tls.md](tls.md) already solves either way.
2. **`NSAllowsLocalNetworking`** as well, so a server on the same Wi-Fi may be
   cleartext. Defensible for a household appliance and narrow enough not to attract
   attention at review.
3. **`NSAllowsArbitraryLoads`** — do not. It requires a written justification at
   review and is a routine cause of rejection.

Take (1), and (2) only if local-network installs turn out to matter. Note that (2)
interacts with the choice above: a proxy-first world makes cleartext-on-the-LAN more
attractive, because the proxy is the thing that would otherwise have to hold a
certificate for a name that only resolves at home.

## S5 · The demo instance is part of the submission

Whatever else is true, App Review notes must carry a live server over HTTPS and
credentials that work. Concretely:

* The demo address must be **admitted on that instance**, or the reviewer meets the
  refusal from `ServiceError::NotAdmitted` and files it as a broken app.
* It must be **seeded with real-looking lists and items**. An empty app reads as
  broken, and reviewers do not add test data for you.
* The app should **offer the demo** rather than requiring it to be typed.
* It has to stay up for the whole review, and for every resubmission.

This is running cost, indefinitely, for an app whose selling point is that I run
nothing. Worth knowing before committing to the store as a distribution channel —
TestFlight has no such requirement.

## S6 · What the listing says

Say **"requires your own Shopping List server"** in the first line of the
description. Reviewers reject surprises, not requirements. Screenshots show lists and
items, never a setup screen.

Privacy nutrition labels describe what *I* collect, which in this design is nothing —
but only if that is true including crash reporting, so decide about crash reporting
before answering the questionnaire. The privacy policy still has to exist, and can
say something unusually simple: the data goes to the server you chose, and the person
who wrote this app has no access to it.

---

# Part two · Who runs it, and who they let in

Designed in [configuration.md](configuration.md), A1–A7: the first person through the
door claims the server, a one-time claim code printed to the log closes the bootstrap
window, admitted addresses are a table, and the last owner cannot be demoted.

Two notes rather than a redesign:

**Ownership is already transferable to several people.** A3 makes it a flag on a
user, not a singleton, so promoting a second owner is an `UPDATE` and A5's last-owner
rule is what keeps it safe. Nothing needs changing for that.

**An owner is a server role, not a data role.** With S9 in place an owner administers
who may use the machine and cannot read anybody's lists — not by policy but because
no key for them exists on it. That property is worth stating in the admin screen, and
it is worth refusing every future request for an "owner can see all lists"
convenience, because the moment one exists the promise in S8 stops being true.

**`ALLOWED_EMAILS` is doing more work than it looks.** While it holds a household,
processing is personal and household and outside GDPR entirely. Setting it to `*` is
the moment somebody becomes a data controller with the full set of obligations. That
transition deserves a sentence in the admin screen, because it is currently one
environment variable and no warning.

---

# Part three · What the operator can see

## S8 · The threat model, written down

Publish this, and design against it. Vague claims about encryption age badly;
a stated boundary can be checked.

> **An attacker with root on a running server can learn who shares a list with whom,
> and when they shop. They cannot learn what is on any list.**
>
> Somebody holding a stolen disk, a snapshot or a backup learns less: the lists are
> ciphertext there too, and nothing on the machine can decrypt them.

Two things follow that are easy to get wrong.

**Self-hosting inverts the usual threat model.** The person with root *is* the user.
There is no operator to protect people from, because they are the same person. What
is actually being defended against is the discarded disk, the hosting provider, the
snapshot, and the backup on somebody's laptop.

**A shopping list is more revealing than it looks.** Medication, dietary
restrictions, alcohol, pregnancy tests, Lent, halal, kosher. None of it is Article 9
data anybody set out to collect, and all of it is inferable from what people type.
That is the reason for S9, and it is a better reason than compliance.

## S9 · Content is encrypted to its readers, not to the server

Per list: a symmetric key, held by the members and wrapped to each of them. The server
stores ciphertext and wrapped keys, and can decrypt neither.

This is not a variant of encryption at rest — it is a different thing, and a stronger
one, because **no key exists on the machine at all**. At-rest encryption assumes the
key is present and defends the moments when the disk is not being used. This defends
every moment.

The consequence is that the merge rules move to the clients: `service::sync` decides
that a rename splits into a second row by reading names, and it will not be able to.
The server keeps the total order — which is what spares this design the CRDT problem
described in [offline.md](offline.md), since with an authoritative sequence the
existing rules survive nearly verbatim, evaluated client-side.

It also ends the server-rendered web UI. htmx renders lists the server can read; a
browser holding the key is a different application, and `domain` compiling to WASM is
what makes it the same rules rather than a third implementation.

**The key management is [encryption.md](encryption.md)** — three key layers, invites
carrying the list key in the URL fragment, epochs for revocation, and the recovery
problem, which turns out to be mostly social rather than cryptographic.

## S10 · What is left in the clear, and how to make it boring

S9 covers content. It does not cover the metadata the server needs in order to be a
server:

| Left in the clear | Why | What to do |
|---|---|---|
| Membership | The server routes by it | Nothing — accept and disclose |
| Timestamps, operation counts | Ordering, idempotency | Nothing — accept and disclose |
| Addresses in `users` and `admitted_emails` | Admission, and linking one person's two providers | **Store `HMAC(address)`** |
| Session tokens | Already only hashes | Nothing — already right |

The address row is the one worth doing. Admission and the two-providers-one-person
match in `identity::from_claims` are both exact comparisons, so a keyed hash serves
them unchanged, and the database then contains no addresses at all. Note the cost: A3
keeps the address as "the label a person reads" in the admin screen, so either that
label goes, or it is stored encrypted alongside the hash and decrypted for display.

That single change does more for the metadata problem than encrypting a column would.

## S11 · Encryption at rest, honestly ranked

Say the uncomfortable thing first: **a key cannot be hidden from root on a running
machine.** If the process can decrypt, the key is in its memory, and root reads
memory. Everything below is degrees of difficulty, not a solution — which is why S9
is the answer and this is defence in depth.

* **Encrypted backups, key held elsewhere** — highest value, lowest effort, and it
  covers the copy that actually leaks. See S12.
* **Minimise what is left** — S10. Cheaper than encrypting it and it cannot be
  misconfigured.
* **LUKS on the data volume** — covers the stopped VM, the snapshot, the discarded
  disk. The catch is the key: a passphrase at boot means no unattended reboots, and
  anything automatic protects less than it appears to. Document the trade and let the
  operator choose; do not choose for them.
* **SQLCipher** — the same protection, narrower scope, and worth it only if the key
  genuinely lives on another machine. For most self-hosters it will end up in an
  environment variable next to the database, which protects against copying the file
  and nothing else.

Deliberately not designed for: an external KMS, which is a cloud pattern and an
unreasonable ask of somebody running a shopping list on a five-euro box; and
confidential computing (SEV-SNP, TDX), which is the only technology that genuinely
protects memory from the host and is far beyond what this justifies. Name them in the
operator documentation so people know what was considered.

## S12 · Backups are where data leaks

Not the running server: object storage, an old laptop, a drive that went in a bin.
Encrypt them with a key that has never been on the VM, and do it before any of the
rest of part three, because it is an afternoon's work and covers the likeliest
failure.

**Built** — `ops/backup.sh`, `ops/restore.sh` and [ops/README.md](../ops/README.md).
The server holds only an `age` public key, so it writes backups it cannot read; the
private half never goes near it, and restoring is a thing you do on a laptop.

The half that matters and is easy to skip is that restoring is *tested*. `restore.sh`
checks every page and prints the counts, so a file that decrypts and passes an
integrity check can still be recognised as the wrong one. A backup nobody has opened
is a belief.

## S13 · Erasure that reaches the backups

Article 17 versus backups is the standard unsolvable problem: you cannot rewrite last
month's archive, and "it ages out in ninety days" is an answer people accept
grudgingly.

S9 gives a better one for free. **Delete a person's wrapped keys and every backup copy
of their content becomes noise.** The ciphertext is still there and is no longer
data about anybody. That is worth writing down in the operator documentation, because
it turns the hardest question a self-hoster will be asked into a short answer.

---

## In what order

Two tracks, and only the first has a deadline attached to it.

**To ship:** S12 (an afternoon, do it now regardless) → S4 and [tls.md](tls.md) →
S2 via [configuration.md](configuration.md) → S3 → S1 → S5 and S6.

S1 is last of the store items because it is the largest, and first in importance —
everything else is mechanical and it is the one that decides the review.

**To keep the promise:** S8 written down → S10 → S9, which is a version rather than a
feature: it touches sync, the API shape, all four clients and the whole web UI.

The order matters. S8 and S10 are cheap and make the current design honest. S9 is
expensive and makes it true. Publishing the promise in S8 before S9 exists would be
a lie with a timestamp on it, so what S8 describes today is the metadata boundary
alone, and it is revised when S9 lands.

## Open

**Whether the store is the right channel at all.** S5 is a permanent hosting
commitment and S1 is a substantial change, both in service of a distribution route
that TestFlight provides for nothing. If the audience is a handful of people who
already self-host, TestFlight is the whole answer and part one can be deleted.

**Whether local-only lists ever sync.** S1 says a local list can be adopted by a
server later. The alternative — local lists stay local for ever, and joining a server
starts fresh — is much simpler and worse, and the choice should be made deliberately
rather than by whichever is easier to write.

**Which HTTPS path leads.** S4 argues for documenting the reverse proxy first and
in-process TLS as the alternative for a bare box. [tls.md](tls.md)'s T1 decided the
other way round, with a rationale — one binary, one thing to run — that has not
stopped being true. Nothing about the capability changes either way; this is about
which one a self-hoster meets first, and it should be settled in `tls.md`.

**Recovery, in S9.** Lose every device and the lists are gone in a way no backup
helps with. A recovery phrase people lose, or a passphrase that reintroduces the
password Sign in with Apple just removed. There is no third option, and it is the
single decision most likely to make this unusable for ordinary people.
