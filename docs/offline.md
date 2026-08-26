# Working without the server

A design, not an implementation. Nothing here is built yet.

## What we are aiming at

The app works with no connection at all: you can open it, read the list, add
things, cross them off, edit and delete — and none of it waits for a server. When
a connection comes back, everything that happened is sent in the order it
happened, and the server merges it with whatever else arrived meanwhile.

Two things follow that are worth saying out loud:

* **Conflicts are normal, not exceptional.** Two people in two shops with the
  same list will conflict every week. A design that prevents conflicts by
  refusing edits has moved the problem onto the person holding the phone.
* **Merging beats blocking.** The rule for every case below is "what would a
  person have wanted", and only where that has no answer do we ask them.

## The shape everybody uses

The pattern is well trodden: a local database that is the app's only source of
truth, an outbox of operations waiting to be sent, optimistic updates so the
screen never waits, and per-entity merge rules on the server — last-write-wins
for single-owner fields, structured merges for independent ones, CRDTs where
data is genuinely collaborative
([Hasura's guide](https://hasura.io/blog/design-guide-to-offline-first-apps),
[offline-first Android](https://medium.com/@anandwana/offline-first-android-system-design-a-complete-guide-1-dae47eac680c),
[sync and conflict patterns](https://www.sachith.co.uk/offline-sync-conflict-resolution-patterns-architecture-trade%E2%80%91offs-practical-guide-feb-19-2026/)).

We want the operation-log half of that, and only as much CRDT thinking as this
domain actually needs.

## Why this domain is unusually kind

A shopping list is nearly all commutative, and this server already leans that
way:

* **Adding merges.** `items::create` already finds an item with the same name
  and unit and adds the amounts. Two devices each adding `2 kg apples` offline
  produce 4 kg, in either order, with no special handling.
* **Crossing off is a timestamp**, not a flag, so the later one plainly wins.
* **Tags are a set.** Attaching twice is attaching once.
* **The memory and the suggestions are derived.** They never need syncing;
  they are rebuilt from what arrives.

So the work is not "build a CRDT". It is: give every change a name, decide what
each one means when it arrives late, and be careful with the two or three that
are genuinely destructive.

## The operations

Each carries an id (a UUID, for idempotency), the device that made it, a
timestamp, and its arguments.

| Operation | Late-arrival rule | Why |
|---|---|---|
| `add(list, line)` | Apply as-is: the service merges by name and unit | Adding twice is one intention entered twice — already true today |
| `setDone(item, done, at)` | Last write by `at` wins | The flag is already a timestamp; the later decision is the real one |
| `rename(item, name, at)` | Last write per field by `at` | Two people renaming the same row is rare and one of them must win |
| `setAmount(item, amount, at)` | Last write by `at` | **Not** the same as adding — see below |
| `attach/detach(item, tag)` | Attach wins over a concurrent detach | Filing something is a positive act; losing it is worse than an extra tag |
| `delete(item)` | Delete wins over earlier edits; a *later* `add` of the same name creates a new row | You cannot edit what somebody removed, but re-adding is a new intention |
| `clearDone(list, ids)` | Deletes **only the listed ids** | See "the dangerous one" |
| `createList / renameList` | Last write by `at` | Single-owner data |
| `deleteList(list)` | Delete wins | Owner-only already |
| `setTagOrder(list, tags)` | Last write by `at`, per person | Already per person; nobody else is affected |

Not queued, ever: **invitations and joining.** A share code is a secret the
server issues, and an offline device cannot invent one. These stay online-only
and say so.

## Add is not the same as set

The single most important distinction, and the one a naive design gets wrong.

* `add(2 kg apples)` means **+2**. Two devices, two adds, offline: 4 kg.
* `setAmount(item, 5)` means **=5**. Two devices, two sets: the later wins.

Today both go through the same door and the difference is implicit in which
screen you used. Offline, they must be different operations, because replaying
"the amount is now 5" three times is fine and replaying "+2" three times is not.

This is why every operation carries an id and the server records which it has
applied: a client that times out and resends must not add twice.

## The dangerous one: clear done

`clearDone` today means "delete every row on this list that is currently done".
Replayed an hour later, that sentence is a different sentence — it would sweep
away things somebody ticked off in the meantime, which nobody asked for.

So the operation records **the ids it meant**, decided on the device at the time.
Replayed late, it deletes those rows and nothing else. Rows already gone are a
no-op.

The same reasoning applies to any "all the ones that…" operation we add later.

## Identity for things made offline

An item created offline has no server id, and later operations need to name it.
Options considered:

1. **Temporary ids rewritten on sync** — the client mints a negative id and the
   server returns a mapping. Every queued operation then needs rewriting, and a
   crash mid-sync leaves a half-rewritten queue.
2. **Client-generated ids as the real identity** — every item gets a UUID at
   creation, on whichever device made it, and the server stores it.

**Take (2).** It costs a column and an index, makes replay naturally idempotent
(the same add twice is the same UUID twice), and removes a whole class of
"which id did that become" bugs. The integer primary key stays for the database's
own use; the UUID is what operations talk about.

## Order, and clocks that lie

Device clocks are wrong, sometimes by hours, and "last write wins" on a wrong
clock loses the right write.

* Each device keeps a **monotonic sequence number**; its own operations are
  applied in that order, always.
* Across devices, order is decided by a **hybrid logical clock**: the wall clock
  when it is plausible, a logical counter when it is not. The server stamps
  arrival and clamps a client timestamp that is wildly ahead.
* For the LWW rules above, ties break on device id — arbitrary but stable, so
  every replica reaches the same answer.

## The wire

One route, because a batch is the unit that has to succeed or fail:

```
POST /api/sync
{ "since": "<cursor>", "operations": [ … ] }
→ { "applied": ["<op-id>", …], "rejected": [{ "id": …, "why": … }],
    "changes": { … }, "cursor": "<new cursor>" }
```

* **Idempotent**: applied operation ids are recorded; a resend is a no-op.
* **Atomic per operation**, not per batch: one refusal must not discard the rest.
* **Rejections are data**, not errors — an operation on a list you have been
  removed from comes back with a reason the app can show.
* The existing event streams stay: they are how a client learns to pull. Sync is
  how it pushes.

## On the device

* **Android** — Room, with an `operations` outbox table and `pending` flags on
  rows, driven by WorkManager so a queued change survives the app being killed.
* **iOS / macOS** — SwiftData or GRDB, same shape, with a background task.
* **watchOS** — reads only. It already asks the phone for a token; asking it to
  hold a queue as well is a lot of machinery for a screen you glance at. It
  stays online-only, and says so when it cannot reach anything.

## What the screen must say

Offline handling is mostly a communication problem:

* **Never claim emptiness you have not verified.** "No lists yet" when the
  server was unreachable is the bug that made this worth doing.
* A row with unsent changes is marked as such — quietly, not with a banner.
* The state is one of three: **up to date**, **offline with N changes waiting**,
  **something was refused**. Only the third interrupts.
* A merge that went a way the person might not expect — a delete that beat their
  edit — is worth a line in a "what changed while you were away" note. Silently
  correct is not the same as understood.

## Deliberately not doing

* **A general CRDT library.** Automerge or Yjs would solve this and bring a
  document model, a sync protocol and a size budget the domain does not need.
  The rules above are a semantic CRDT for exactly these ten operations.
* **Operational transformation.** For text, not for a shopping list.
* **Offline sign-in.** A token expires in an hour; a person who has never signed
  in on this device cannot start offline. That is a real limit and it should be
  said plainly rather than papered over.

## In what order

1. **Read offline.** Persist the last-loaded lists and items; fix the false
   "no lists"; show the offline state. Most of the value, none of the conflict
   theory.
2. **Client ids.** Add the UUID column and migrate; nothing user-visible.
3. **The outbox, one operation at a time.** Start with `setDone`, which is the
   commonest and the most forgiving.
4. **The rest of the operations**, in the table's order.
5. **`POST /api/sync`** and batch replay, once the operations exist.
6. **The "what changed" note**, once there is something worth telling.

Each step is useful on its own, and the app is never half-migrated: an operation
either has an offline path or it does not, and the ones that do not stay
online-only until they get one.

## Open questions

1. **How long may a queued change wait** before we stop trying and ask? A week?
   Never?
2. **Should a rejected operation be recoverable** — kept so it can be retried
   against a list you have been re-invited to — or discarded with a note?
3. **Is `attach wins` right for tags**, or should a detach made later on a
   different device stick? The first is safer, the second is more obedient.
