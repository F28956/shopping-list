# Shopping list, on a phone

A SwiftUI app over the same API the web UI uses. It is a client and nothing more:
every rule about who may see or change what lives in `domain::service`, and this app
gets the same answers as the browser because it asks the same questions.

## Building

```sh
cd ios
xcodegen generate          # writes ShoppingList.xcodeproj from project.yml
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
- Bundle ID: `com.rimantas.shoppinglist` (match `PRODUCT_BUNDLE_IDENTIFIER` in
  `project.yml` if you change it)

That gives you a client id and its *reversed* form. Put both in `ios/Config.xcconfig`,
which is gitignored:

```
GOOGLE_IOS_CLIENT_ID = 000000000000-xxxx.apps.googleusercontent.com
GOOGLE_IOS_REVERSED_CLIENT_ID = com.googleusercontent.apps.000000000000-xxxx
SHOPPING_LIST_API_BASE_URL = http://192.168.1.10:8080
```

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
