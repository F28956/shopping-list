# What is wrong with the Apple clients

A review of `ios/` on 28 August 2026, kept because a list of known problems is worth
more than a memory of them. Ordered by what would hurt most.

Anything fixed should be struck here with the commit that did it, rather than deleted:
the reason something was a problem is usually the reason it comes back.

## ~~1. iOS and the Mac duplicate their whole domain layer~~ — fixed, `db13b88` and `fa0cf9d`

`ItemsView` (1140 lines) and `MacItemsView` (757) hold the same logic. Comparing
bodies with comments stripped:

* **Nine functions byte-identical**: `attempt`, `clearDone`, `keepTrying`, `remove`,
  `seedReference`, `show`, `showWhatWeHave`, `toggle`, `watch`.
* **Five more between 70% and 95%**: `add`, `drain`, `load`, `refreshUnsent`,
  `withUnsent`.
* Only `row` differs for a reason — it is the platform's own list row.

This is not theoretical. Four things diverged in a single afternoon, because each was
fixed on the phone and not the Mac:

| | iOS | Mac |
| --- | --- | --- |
| Reordering categories offline | queued | still `api.setTagOrder`, so it fails with no server |
| History synced from the server | yes | no |
| Suggestions from the device's own memory | yes | no |
| Unit and amount defaulted on an edit | yes | no |

**The cause was structural.** Android has `ItemsViewModel` and `ListsViewModel`; the
Apple clients had none, so the logic lived inside SwiftUI `View` structs. Which is
also why nothing tested it: no test in `ShoppingListTests` mentioned `withUnsent`,
`drain` or `apply`, because reaching them meant hosting a view.

**Fixed.** `ItemsModel` holds it and both views share it. `ItemsView` went from 1140
lines to 490, `MacItemsView` from 757 to 383, and the only function left in both is
`row` — the platform's own list row, which is the one thing that should differ. All
four divergences above closed with it, because the Mac now runs the same code.

There are twelve tests on that logic now, which is twelve more than there were: what a
typed line becomes, that adding the same thing twice makes one row, that a crossed-off
row comes back, that a queued tick survives a reload, that a row somebody else deleted
does not return as a ghost.

**Done too.** `ListsModel` now holds it. `ListsView` went from 465 lines to 288 and
`MacShoppingView` from 367 to 257; neither has a `private func` left. Three things had
already drifted, all on the Mac and all in the direction of the Mac being worse:

| | iOS | Mac |
| --- | --- | --- |
| Making a list with no server | queued locally since S1 | `api.createList`, so the one toolbar button raised a dialog and made nothing |
| Lists made offline, after a drain | server's id adopted | never adopted, so the list would have appeared twice |
| Somebody the server will not have | told on the sign-in screen | raw error dialog — no `notAdmitted` arm |

Eight tests on that logic now, where there were none: that a list can be made with no
server and survives a restart, that a failed load does not claim there is nothing, that
a slow cache read cannot overwrite a fast answer, and what each kind of refusal does.

**And the rest of the Mac's gaps with it.** The decision is that the Mac behaves as the
phone does, so the remaining differences were gaps rather than choices:

* **No status dot.** It kept a counter of what was waiting and rendered it nowhere, so a
  Mac holding unsent changes looked exactly like one in step. It has the phone's dot now,
  hidden with no server for the reason `StatusDot` gives.
* **No Categories screen.** Editable categories reached two of the three clients.
* **No "Who may sign in".** The owner's screen existed on the phone only, though its
  comment already said it was written to be presented from the Mac.
* **No way to use a share link.** C7 was iOS-only, so somebody sent a link on a Mac had
  to pick the host out of it by hand.

`TagsView` and `ServerPeopleView` moved to `UI/Sources` rather than being copied, and the
four modifiers that exist on one platform and not the other are behind `PlatformChrome`
— because a second copy of a screen is exactly how this file's first two entries began.

## ~~2. An invitation token travels in the URL path~~ — fixed

`POST /api/invites/{token}` put a bearer credential in a URL, where it is written to
every access log, proxy log and analytics trace between here and the server — for the
week the token stays valid.

Two changes, because there were two places it appeared. The API takes it in the body:
`POST /api/invites` with `{"token": …}`. The share link carries it in the **fragment**,
`https://host/join#TOKEN`, which is the one part of a URL a browser never sends — so it
reaches no log at all, and the link is still one thing to paste into a message. The
page at `/join` reads it back out of the address bar and hands it over in a form post,
holding it in the session across a sign-in so that following a link on a device nobody
has signed in on yet does not throw the invitation away. Both clients read either
shape, so links already sent keep working.

## ~~3. The cleartext rule has five doors and one lock~~ — fixed

`ServerAddress.allowsCleartext` is `false` in release. Five call sites passed
`allowingCleartext: true` regardless, including the one that parses a **pasted,
untrusted** share link. The release guarantee survived only because the one path that
stores an address happened to use the default — an invariant held by convention.

The parameter is gone, on both iOS and Android, where the same four-caller version of
it existed. There is nothing to pass, so nothing can opt out: a debug build allows
cleartext, a release build refuses it, and that is the whole rule.

## ~~4. Six `catch {}` blocks swallow everything~~ — fixed

Five, by the time they were counted, and they were not all the same thing.

Four are the SSE watch loops, where anything that is not an `APIError` is the
connection going away — a tunnel, a lock screen, a server restarting. Swallowing that
is right; the loop waiting and reconnecting is the whole response. What was missing was
any statement of it at the brace, so a reader had to find the comment further down and
join it up. Each now says so where it is.

The fifth was real. `WatchItemsView.loadReference` swallowed a failure and left `units`
and `tags` empty for as long as the screen was up, so every row lost its measure and its
aisle — on the device with the worst connection of the three, where the ask is relayed
through a phone and so fails most often. It now falls back the way the phone does:
the cache first, then the set that shipped with the app, same ids. `ItemsModel`'s own
`loadReference` had already gained that fallback when it was extracted.

## ~~5. `Cache` is `@unchecked Sendable` and nothing records why that is true~~ — fixed

Two halves, one of them a live bug.

The promise is now written down on `Cache` and on `Outbox`: no mutable stored state,
and every touch of the database through GRDB's `DatabaseQueue`, which serialises across
threads — with the two things that would break it named, so the next person changing
this file is told rather than left to re-derive it.

`cacheChanged` was the bug. A notification is delivered synchronously on whichever
thread posted it, this was posted from whichever thread happened to be writing, and two
of the three listeners are SwiftUI `.onReceive` closures assigning into `@State`. That
is a background write to view state — the kind that works until it does not. `announce`
now hops to the main queue, once, so a listener added later inherits the guarantee
rather than having to know about it.

## Checked and found sound

Worth writing down so nobody re-reviews them:

* **Keychain** uses `kSecAttrAccessibleAfterFirstUnlock`, not `Always`, and is not
  synchronised to iCloud.
* **TLS** has no trust overrides and no `NSAllowsArbitraryLoads`. The one exception is
  `NSAllowsLocalNetworking`, which is the narrow one Apple grants without a written
  justification.
* **The UI-test stub** is behind `#if DEBUG` *and* a launch argument, with a test
  asserting a release build cannot reach it.
* **Tokens** never appear in an error message or a log line.
* **`Grouping`'s force unwraps** are provably safe: `seen` only gains a key in the same
  branch that fills both dictionaries.
