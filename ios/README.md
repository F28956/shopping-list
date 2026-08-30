# Shopping list, on a phone

A SwiftUI app over the same API the web UI uses. It is a client and nothing more:
every rule about who may see or change what lives in `domain::service`, and this app
gets the same answers as the browser because it asks the same questions.

## Building

```sh
cd ios
cp -n Config.example.xcconfig Config.xcconfig   # per-machine values; gitignored
xcodegen generate                               # writes the .xcodeproj from project.yml
open ShoppingList.xcodeproj
```

The `.xcodeproj` is generated and not committed: a `.pbxproj` reviews badly and
conflicts worse. `project.yml` is the thing to edit.

It builds and runs against a simulator with no further setup — you will reach the
sign-in screen and no further, because signing in needs the two things below.

## How signing in works

**Sign in with Apple**, and then not Apple again.

Apple's identity token lasts about ten minutes and cannot be refreshed silently, so it
is a bootstrap rather than a credential: the app trades it once, at
`POST /api/sessions`, for an opaque token this server issued. That one lives in the
keychain, lasts ninety days of idleness, and needs no network to produce — which is
what lets a phone that has been offline for a fortnight still open its lists.

Android and the browser keep signing in with Google. The server accepts both and
matches one person's two identities by their verified address, so the same human with
an Android phone and a Mac has one account and one set of lists.

## The two things only you can do

### 1. A team, and a device on it

Sign in with Apple is an entitlement, and an entitlement means a real signature.

- Put your ten-character Team ID (developer.apple.com → Membership) in
  `ios/Config.xcconfig` as `DEVELOPMENT_TEAM`.
- Enable **Sign in with Apple** for the `dev.f28956.shopping-list` App ID on
  developer.apple.com → Identifiers. Xcode will offer to do this for you the first
  time you build with automatic signing.
- Building for a Mac or a handset registers that device against the account; the
  simulator needs none of this, and neither does the watch app.

Then tell the server which apps to accept tokens from, in `web/.env`:

```
APPLE_BUNDLE_IDS="dev.f28956.shopping-list"
```

Without that last step the server rejects everything the Apple apps send. A native
Sign in with Apple token has no separate client id: its audience *is* the bundle
identifier, which is why this is a bundle id and not something from a console. Leave
it unset and the server still starts — it simply does not accept Apple tokens.

### 2. Reaching the server from a phone

`localhost` on a phone is the phone. Point `SHOPPING_LIST_API_BASE_URL` at the machine
running the server — and note that iOS refuses plain HTTP to arbitrary hosts, so for a
real device you want either a tunnel with TLS or an ATS exception. The simulator shares
the Mac's network, so `http://localhost:8080` works there as it is.

## What it does

Lists, and the items on them: add by typing a line the same way the web quick-add
does, tick off, and delete. Editing an item, tags, sharing and notes are deliberately
not here — a phone in a shop is for the two things you actually do while standing in
one.

## The watch app

`ShoppingListWatch` is a watchOS app embedded in the phone app. It shows what is on a
list and crosses items off. It cannot add, edit, delete or tag — a watch is glanced at
with one hand on a trolley, and a row that does two things is a row that does the
wrong one.

### How it signs in

It does not. watchOS has no Sign in with Apple sheet and a watch has no browser to run
a flow in, so the watch asks the paired phone for a token over WatchConnectivity — a
link Apple has already authenticated — and calls the API itself.

It then *keeps* it, in its own keychain. What the phone hands over is a session token
good for ninety days of use, so a watch that has been near its phone once goes on
working in a shop with no signal and the phone left at home. It throws the token away
only when the server refuses it, which is the one reliable news that a session has
ended.

The consequence is visible and deliberate: a watch that has never been near a signed-in
phone says "Open Shopping on your phone" rather than pretending to work.

### Building and running it

Build the *phone* scheme — the watch app is embedded, so it comes along:

```sh
xcodegen generate
xcodebuild -project ShoppingList.xcodeproj -scheme ShoppingList \
    -destination 'platform=iOS Simulator,name=iPhone 17 Pro Max' build
```

Do **not** pass `-sdk iphonesimulator`. It overrides the SDK for every target,
including the watch one, which then builds against iOS: the symptom is a confusing
`WCSessionDelegate` conformance error, and if it gets past that, an install refused
for `UIDeviceFamily`.

To run it, the phone and watch simulators have to be a pair (`xcrun simctl list
pairs`), and the watch app is installed separately:

```sh
xcrun simctl install <watch-udid> \
    ~/Library/Developer/Xcode/DerivedData/.../Debug-watchsimulator/ShoppingListWatch.app
xcrun simctl launch <watch-udid> dev.f28956.shopping-list.watchkitapp
```

Sign in on the paired phone first, or the watch has nothing to ask.

## The Mac app

`ShoppingListMac` is a native macOS app, not Mac Catalyst. What it needs is a split
view, a context menu and an add field pinned under the list — none of which a
stretched phone layout gives you. Everything below the views is shared byte for byte:
`Shared/Sources` (models, API client, grouping, draft rules) and `Auth/Sources`
(the identity, which since Sign in with Apple moved onto SwiftUI's own button has no
platform difference left at all).

It carries **the same bundle id as the phone app**, deliberately. A Sign in with Apple
token's audience is the bundle identifier, so one id means one entry in
`APPLE_BUNDLE_IDS` and one App ID to keep the entitlement on — and it is one app to a
person either way.

```sh
xcodegen generate
xcodebuild -project ShoppingList.xcodeproj -scheme ShoppingListMac \
    -destination 'platform=macOS' -derivedDataPath /tmp/ddmac build
open /tmp/ddmac/Build/Products/Debug/ShoppingListMac.app
```

It is not sandboxed. This is a personal app, built locally, talking to a server on
localhost; sandboxing would mean an entitlements file and a keychain access group to
hold a token the phone already has. Turn it on before it ever leaves this machine.
