# What is wrong with the Apple clients

A review of `ios/` on 28 August 2026, kept because a list of known problems is worth
more than a memory of them. Ordered by what would hurt most.

Anything fixed should be struck here with the commit that did it, rather than deleted:
the reason something was a problem is usually the reason it comes back.

## 1. iOS and the Mac duplicate their whole domain layer — **being fixed**

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

**The cause is structural.** Android has `ItemsViewModel` and `ListsViewModel`; the
Apple clients have none, so the logic lives inside SwiftUI `View` structs. Which is
also why nothing tests it: no test in `ShoppingListTests` mentions `withUnsent`,
`drain` or `apply`, because reaching them means hosting a view.

## 2. An invitation token travels in the URL path

`POST /api/invites/{token}` puts a bearer credential in a URL, where it is written to
every access log, proxy log and analytics trace between here and the server. It
belongs in the body. It is also not percent-encoded, so it depends on `token(in:)`
having rejected a `/` further up.

## 3. The cleartext rule has five doors and one lock

`ServerAddress.allowsCleartext` is `false` in release. Five call sites pass
`allowingCleartext: true` regardless, including `JoinLink.swift:51`, which parses a
**pasted, untrusted** share link. The release guarantee survives only because
`remember` happens to be reached through a default-rule parse. That is an invariant
held by convention rather than by a type.

## 4. Six `catch {}` blocks swallow everything

Including `loadReference` on both platforms: reference data and history can fail to
load for any reason and nothing is said anywhere, ever.

## 5. `Cache` is `@unchecked Sendable` and nothing records why that is true

It is true today — every stored property is a `let` and GRDB serialises access — but
the compiler is being told to trust rather than check, and nothing says what would
break the promise. `cacheChanged` is also posted on whichever thread wrote, and
consumed straight into SwiftUI `@State`.

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
