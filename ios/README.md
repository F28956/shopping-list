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
sign-in screen and no further, because signing in needs the two values below.

## The two things only you can do

### 1. An iOS client id

Google issues a different client id per platform, so the phone cannot use the web
one. In the [Google Cloud console](https://console.cloud.google.com/apis/credentials)
for the same project:

- **Create credentials → OAuth client ID → iOS**
- Bundle ID: `com.cernauskas.shoppinglist` (match `PRODUCT_BUNDLE_IDENTIFIER` in
  `project.yml` if you change it)

That gives you a client id and its *reversed* form. Put both in `ios/Config.xcconfig`
and regenerate:

```
GOOGLE_IOS_CLIENT_ID = 000000000000-xxxx.apps.googleusercontent.com
GOOGLE_IOS_REVERSED_CLIENT_ID = com.googleusercontent.apps.000000000000-xxxx
```

None of those are secrets: an iOS client id ships inside every copy of the app, and
Google's iOS clients have no client secret at all — which is why the flow uses PKCE.

Then tell the server to accept tokens from it, in `web/.env`:

```
GOOGLE_IOS_CLIENT_ID="000000000000-xxxx.apps.googleusercontent.com"
```

Without that last step the server rejects everything the phone sends: a Google token
names the client it was minted for, and the API only accepts audiences it knows.

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

It does not. Google's SDK has no watchOS build, and a watch has no browser to run the
flow in, so the watch asks the paired phone for a current ID token over
WatchConnectivity and calls the API itself. Asked rather than pushed: a Google ID
token lasts about an hour, and one pushed when the phone felt like it is stale exactly
when the watch needs it.

The consequence is visible and deliberate: with the phone unreachable or signed out,
the watch says "Open Shopping on your phone" rather than pretending to work.

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
xcrun simctl launch <watch-udid> com.cernauskas.shoppinglist.watchkitapp
```

Sign in on the paired phone first, or the watch has nothing to ask.
