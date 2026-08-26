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

* **Adding is idempotent.** `items::create` finds an item with the same name and
  unit and leaves it alone. Two devices each adding `2 kg apples` offline produce
  one row, in either order, however many times the event arrives.
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
| `add(list, line)` | Apply as-is: already there means nothing happens | Idempotent, so arriving twice or late is the same as arriving once |
| `setDone(item, done, at)` | Last write by `at` wins | The flag is already a timestamp; the later decision is the real one |
| `rename(item, name, at)` | Splits into a second row if the row changed under it, otherwise renames in place | Nobody loses an edit; see scenario 5 |
| `setAmount(item, amount, at)` | Last write by `at` | **Not** the same as adding — see below |
| `attach/detach(item, tag)` | Attach wins over a concurrent detach | Filing something is a positive act; losing it is worse than an extra tag |
| `delete(item)` | Delete is final — it beats edits arriving after it too; a *later* `add` of the same name creates a new row | Deletion is a fact about the server, not an intention on a device; re-adding is a new intention |
| `clearDone(list, ids)` | Deletes **only the listed ids** | The device is the only thing that can say what was clearable; see "the dangerous one" |
| `createList / renameList` | Last write by `at` | Single-owner data |
| `deleteList(list)` | Delete wins | Owner-only already |
| `setTagOrder(list, tags)` | Last write by `at`, per person | Already per person; nobody else is affected |

Not queued, ever: **invitations and joining.** A share code is a secret the
server issues, and an offline device cannot invent one. These stay online-only
and say so.

## Adding and setting are different, and only one of them is risky

* `add(2 kg apples)` means **put apples on the list**. Already there? Nothing
  happens. Safe to replay any number of times.
* `setAmount(item, 5)` means **the amount is now 5**. Also safe to replay: the
  second application does nothing the first did not.

Neither is an increment, which is what makes replay cheap here. Operations still
carry ids, because a duplicate `delete` or a duplicate `clearDone` is worth
recognising and because a rejection needs something to name — but no rule
depends on exactly-once delivery.

## The dangerous one: clear done

`clearDone` today means "delete every row on this list that is currently done".
Replayed an hour later, that sentence is a different sentence — it would sweep
away things somebody ticked off in the meantime, which nobody asked for.

So the operation records **the ids it meant**, decided on the device at the time.
Replayed late, it deletes those rows and nothing else. Rows already gone are a
no-op, and a row somebody has put back on the list since is left alone — putting
something back is a newer decision than a sweep queued before it.

This is not a compromise, it is the only thing the device can honestly say. It
cannot know what anybody else ticked off while it had no signal, so it clears
what it could see and leaves the rest to whoever can see it.

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
* This governs **items only**. Whether somebody is still allowed to write to a
  list is decided by arrival at the server, and never by the device's claim
  about when it acted.

## The wire — built

One route, because a batch is the unit a device has: everything it did since it
was last heard from, in the order it did it.

```
POST /api/sync
{ "operations": [
    { "id": "<uuid>", "at": "2026-08-26T10:00:00Z", "list": "<list uuid>",
      "kind": "set_done", "item": "<item uuid>", "done": true },
    …
] }
→ { "operations": [
    { "id": "<uuid>", "outcome": "applied", "item": { … } },
    { "id": "<uuid>", "outcome": "already_applied", "item": { … } },
    { "id": "<uuid>", "outcome": "refused", "why": "gone" }
] }
```

* **Everything is named by uuid**, never by id. That is what `items.uuid` is for:
  a device that added something with no signal has no id for it and never will
  until this route answers.
* **Idempotent**: applied operation ids are recorded in `applied_operations`; a
  resend comes back `already_applied` and changes nothing. That is what a lost
  answer produces, and "most of these operations are naturally idempotent" is not
  a promise worth making when a table costs so little.
* **Atomic per operation**, not per batch. Somebody who ticked six things off and
  edited a seventh that had been deleted loses the seventh, not all seven.
* **Rejections are data.** `200` even when every operation was refused: the
  request was fine, the changes in it were not. `gone`, `list_gone`,
  `not_allowed`, `invalid` — each is a sentence an app can put in front of
  somebody.
* **The row each operation produced comes back.** Not news about other people —
  the answer to "what did my own change turn into". It is the only way a device
  learns the id of something it created offline, or the row a rename split off.
* **The device's clock travels with each change**, clamped forward only. A tick
  is stamped with when it was made, not when it arrived; a phone in a drawer for
  a month is telling the truth about the past, and a clock set to next year is
  not telling the truth about the future.
* The existing event streams stay: they are how a client learns to pull. Sync is
  how it pushes.

**Push only, deliberately.** The sketch above once had a cursor and a `changes`
payload coming back. It does not: the event streams already say "something moved,
re-read", and a second way to learn the same thing is a second thing to keep in
step.

### What is decided where

| Decision | Where | Why there |
|---|---|---|
| May this person write to this list? | On arrival | A device can claim any time it likes — (8) |
| When did this tick happen? | The device's clock, clamped | Stale work must not lose to fresh work — (7) |
| Rename, or split? | The `seen` fields on the operation | Only the device knows what it was looking at — (5) |
| Which rows did the sweep mean? | The ids on the operation | Only the device could see them — (4) |

## On the device

* **Android** — Room, with an `operations` outbox table and `pending` flags on
  rows, driven by WorkManager so a queued change survives the app being killed.
* **iOS / macOS** — **GRDB**, same shape, with a background task. The Mac is not
  the exception it looks like: a laptop is opened on a train and closed in a
  tunnel, and "it has a proper connection" describes a desk, not a machine you
  carry. It gets the same cache, the same queue and the same cues as the phones. SwiftData is
  built in and would cost no dependency, but it is a model graph with its own
  change tracking, and an outbox is a strictly ordered queue that gets dequeued
  in a transaction. GRDB is SQLite with the SQL written down, which is also the
  server's mental model, so the two halves of a merge rule can be read side by
  side.
* **watchOS** — the same cache and the same outbox. This document used to call the
  watch read-only, and it was simply wrong: crossing things off is exactly what a
  watch is for, and it is the screen most likely to be doing it somewhere with no
  signal. A tick made there used to be thrown away, and the error replaced the
  list, so the change and the list were lost together.

  It cannot sign in — Google has no watchOS SDK — so the token still comes from
  the phone, cached for half an hour. **The two stores are separate and have to
  be**, because they are two devices: an App Group shares a container between an
  app and its extensions on one device, never across a pair. So each queues its
  own work and sends its own, and a watch without cellular reaches the server
  through the phone it is paired to.

  What it says is one dot rather than a sentence. Green: this came from the
  server and nothing is waiting to go back. Orange: one of those is not true. A
  wrist has no line to spare, and the difference between "offline" and "queued"
  is not one anybody acts on mid-shop.
* **The browser** — online-only, and it says so. The web UI is server-rendered
  HTML with htmx: making it work offline means a service worker and a
  client-side store, which is a second copy of the app rather than a feature of
  this one. A person with no signal has the phone app in their pocket.

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

1. ~~**Read offline.**~~ **Done.** The last-loaded lists, items, units and tags
   are persisted on both native clients; the false "no lists" is gone, and so is
   every other emptiness the app had not verified — only a server that answered
   earns an empty state.
2. ~~**Client ids.**~~ **Done.** `items.uuid` and `lists.uuid`, minted wherever
   the row was created, backfilled, and carried by every client.
3. ~~**The outbox, one operation at a time.**~~ **Done for `setDone`.** A tick
   with no signal changes the screen, goes in the queue, and is sent on the next
   successful load from anywhere in the app.
4. ~~**The rest of the operations**, in the table's order.~~ **Done, bar tags.**
   `add`, `setDone`, `update`, `delete` and `clearDone` all have an offline path
   on both native clients, and all of them go over `POST /api/sync`. Attaching
   and detaching tags is the last one without: it stays online-only and says so.
5. ~~**`POST /api/sync`** and batch replay.~~ **Done.** Every operation, named by
   uuid, carrying the device's clock, answered one by one.
6. **The "what changed" note**, once there is something worth telling. Half of it
   exists: a refusal reaches the screen as the sentence for it, and the state
   that says so is the one of the three that interrupts. What is missing is the
   summary after a long time away.

Each step is useful on its own, and the app is never half-migrated: an operation
either has an offline path or it does not, and the ones that do not stay
online-only until they get one.

## What the outbox does today

Queued operations replay through the existing REST routes, one at a time, oldest
first, stopping at the first one that cannot be sent. Which means:

* **A tick made offline survives the app being killed.** It lives in the same
  database as the cache, and that database is migrated by hand rather than
  discarded on a schema change — the cache is a copy of what the server holds,
  but a queued change exists nowhere else in the world.
* **The screen changes first.** A tick in a shop is a decision already made, and
  an app that waits for a server before showing it has made somebody wait for
  something they cannot influence. The queue is the promise that the server will
  hear about it.
* **A successful load anywhere drains it.** Coming back into signal reconnects
  the change stream, the stream triggers a load, and the load sends what has been
  waiting. Draining only from the list's own screen meant a phone that came out
  of a shop and went into a pocket held its ticks until somebody happened to
  reopen that list.
* **A load does not undo what is still queued.** The server has not been told
  yet, so it answers with the old state; the unsent operations are laid back over
  its answer, or the row would flick back for as long as the queue is stuck.
* **Rows with unsent changes are marked quietly**, on the row, and the count is
  in the offline note — "Offline. 2 changes waiting to be sent."

**One thing it does not do yet.** The REST routes stamp `done_at` themselves, so
a tick replayed an hour later lands with the time it was *sent*, not the time it
was made. The device records the real time on the operation from the first day,
and carrying it to the server is what `POST /api/sync` is for — step 5. Until
then the clamped-device-clock rule in (7) is written down but not yet in force,
which matters only where two devices genuinely conflict.

## Settled

* **Every change is an event.** Devices accumulate them offline and send them
  when they can; the server replays them in order and that dictates the state.
* **The latest event wins** where two events genuinely contradict. "Latest" for
  items means the device's own clock, clamped by the server to a plausible
  window; a tampered clock is accepted, because the most it wins is an argument
  about a shopping list.
* **A queue never expires.** A phone left in a drawer for a month still has its
  changes applied when it comes back.
* **Losing access ends your influence.** Somebody removed from a list has no
  effect on it from that moment, whatever is still queued on their device.
  "That moment" is measured by **arrival at the server**, never by the removed
  device's clock — losing work they genuinely did is an accepted cost.
* **Delete is final.** Deletion is a fact about the server, not an intention held
  on a device. Nothing that arrives afterwards resurrects the row; the person
  whose work was dropped is told.

Most pairs of changes never contradict and need no rule: two people crossing the
same thing off agree; one crossing off while the other files it under a tag are
about different fields and both apply.

## Scenarios that need a decision

These are the ones where "latest wins" either has no answer or gives an answer
somebody would be surprised by. Each is written as it would actually happen.

### 1. Add is idempotent — settled

*Anna adds `2 kg apples`. Ben, offline, adds `2 kg apples` too.*

Adding something the list already wants **changes nothing**: it is already there,
and that is the whole answer. It does not become 4 kg — somebody adding a thing
has not looked at the amount and is not asking for it to move. Only the editor
sets an amount.

Crossed off is the one exception, and barely one: adding something already ticked
off puts it back, with the amount it had. Otherwise a person types a name, sees
nothing happen, and reasonably concludes the app is broken.

This is the decision that makes the rest of the design easy. An event saying "put
milk on the list" can arrive twice, or an hour late, or interleaved with anybody
else's, and mean the same thing every time. There is no increment to double-apply
and no need for the server to remember which adds it has already seen.

### 2. Editing something that has been deleted — settled

*Anna deletes `Milk`. Ben, offline, renames it to `Whole milk` and ticks it off.*

Ben's events are later, so "latest wins" says apply them — but there is nothing
to apply them to.

**Delete is final.** A deletion is a fact about the server, not an intention held
on somebody's device: once the row is gone it is gone, and no event that arrives
afterwards brings it back. Ben's rename and tick are dropped.

This is deliberately not symmetrical with the other rules. Everything else is a
claim about what the list should say, and the latest claim wins; a delete is a
statement that there is no longer anything to make claims about. Resurrection by
edit would mean no deletion is ever safe while any device is offline.

Ben is told, in the "what changed" note (see *What the screen must say*), so
that the work he watched himself do does not vanish unexplained.

Re-adding is untouched by this: a *later* `add` of `Milk` is a new intention and
creates a new row, as it always did.

### 3. Crossing off something that has been deleted — settled

The same shape as (2) but far more common — you tick things off in a shop while
somebody at home tidies the list. It takes the same answer: the row is gone, the
tick has nothing to land on and is dropped, and Ben is told.

"My tick did nothing" is the more annoying of the two, which is why the note
matters more here than anywhere else.

### 4. Clear done, replayed late

*Ben taps "clear 3 done" in the shop, offline. An hour later Anna ticks off four
more things. Ben's phone finds signal.*

"Clear everything that is done" replayed now removes Anna's four as well.

The fix is that the event records **the ids it meant** at the time, so it clears
Ben's three and nothing else. This seems clearly right; it is listed because it
is invisible until it bites and it changes the event's shape.

### 5. Two people editing different fields of the same item — settled

*Anna changes Milk to 5 kg. Ben, offline, renames it to `Whole milk`.*

**Both survive.** `Milk` keeps Anna's 5 kg, and `Whole milk` appears beside it
carrying what Ben's screen showed. Nobody's edit is discarded, and the cost is a
row somebody may have to tidy up — which is a cost you can see and undo, unlike
an edit that quietly vanished.

The rule, precisely:

* **A rename splits only when the row changed under it.** The operation carries
  the name, amount and unit the device saw when somebody typed the new name. If
  the row still looks like that, nothing was contested and it is a plain rename —
  one row, new name. If it does not, somebody else edited it meanwhile, and the
  rename becomes a second row rather than an overwrite.
* **An edit that is not a rename never splits.** Two people changing the amount
  are arguing about one number and one of them has to win; two rows both called
  `Milk` would be a worse answer than either. Latest wins, as everywhere else.
* **The new row carries what the renaming device saw** — Ben's 1 kg, not Anna's
  5 kg. It is the row he was looking at, renamed. Giving it Anna's amount would
  hand him a number he never saw and leave two rows claiming the same thing.
* **It inherits the original's tags.** A rename is not a re-filing, and an
  unfiled row is a worse answer than one filed where its predecessor was.

*The rough edge this used to have is gone.* A device that renames and then ticks
off, all offline, once sent both against the id it knew, and a rename that split
left the tick landing on the wrong row. Over `POST /api/sync` the rename's answer
carries the new row, so the device knows which row it made before the next
operation is sent.

### 6. A rename that splits or merges a row

*The list has `Milk`. Anna renames it to `Whole milk`. Ben, offline, adds
`milk`.*

Replayed in that order there are two rows; in the other order, one row of two
units called `Whole milk`. Both are defensible and the outcome depends on
timing, which is the uncomfortable part.

* **(a) Accept it.** Adding by name is how merging works, and a rename genuinely
  changes what the name means.
* **(b) Adds match on the identity they were made against**, not the name, when
  the device could see the row.

*Recommendation: (a),* on the grounds that (b) makes `add` behave differently
depending on what was on screen, which is harder to explain than an extra row.

### 7. Whose clock decides "latest"? — settled

Every rule above says "later". Devices disagree about the time, sometimes by
hours, and a phone that is wrong by a day would win every conflict for a day.

**For items, the device's clock decides, clamped.** The device stamps each
operation; the server clamps a timestamp that is wildly ahead of arrival back
into a plausible window; ties break on device id so every replica reaches the
same answer.

A tampered clock is accepted here. The worst it buys somebody is winning an
argument about what a shopping list says — with a person they already share the
list with, who can simply change it back. That is not worth the cost of the
alternative: ordering items by arrival would let a phone left in a drawer for a
month sync last and overwrite a month of everybody else's work with its stale
view. Stale work clobbering fresh work is the more common harm and the more
confusing one, because nothing was lost in transit — it was silently reverted.

**Access is the exception, and does not use this clock at all** — see (8). The
distinction is deliberate: what the list says is a matter between people who
trust each other, and who is allowed to write to it is not.

### 8. Losing access, and what "after that time" means — settled

*Ben is editing a shared list on a train with no signal. At 14:00 Anna removes
him. At 14:30 he reaches signal, and his phone says his edits happened at 13:50.*

**Access is decided by arrival at the server.** Ben was removed before his batch
arrived, so it is refused, whatever his phone says about when he did the work.

Losing work he genuinely did while he still had access is an accepted cost. This
is a shopping list: the loss is a few lines somebody can retype, and the
alternative is that a removed person keeps writing to a list they were removed
from by claiming an earlier time. A device can claim any time it likes, so
trusting one to police access is not a choice worth the safety it costs.

Ben's phone keeps the refused events and can say what was lost.

**Follow-on — settled.** The refused events stay on the device, with a note
naming what was refused. If Ben is invited back they are still there to send; if
he is not, nothing was quietly binned behind him. A queue for a list he can no
longer see costs a few rows, and the alternative is the same silent-loss habit
that made this document worth writing.

### 9. Events for a list that no longer exists

A queue never expires, so a device can arrive with a fortnight of changes for a
list somebody deleted. They cannot be applied and never will be.

* Drop them silently.
* Drop them and say so.
* Offer to recreate the list from them.

*Recommendation: drop and say so.* Recreating a deleted list from somebody
else's queue is the sort of clever that people find alarming.

### 10. Tags: does a later detach beat an earlier attach?

*Anna files `Bread` under `bakery`. Ben, offline and later, takes it off.*

Strict latest-wins says the tag goes. The earlier draft of this document argued
attach should always win, on the grounds that losing filing is worse than an
extra tag. Under your model, latest-wins is the consistent answer and the
special case is the odd one out.

*Recommendation: follow the rule — latest wins,* and drop the special case.
