# The Android app

Kotlin and Jetpack Compose, Material 3. It talks to the same JSON API as the browser,
the phone, the watch and the Mac, and every rule that decides what a list *means* —
parsing a typed line, merging a repeat, which tag an item sits under, who may share it
— lives in the service on the server. This is a client.

## Not a port of the iOS app

The behaviour matches; the shape does not, on purpose.

| | iOS | Android |
|---|---|---|
| Crossing off | tap the row | a **Checkbox**, which is what the platform means by done |
| Adding | a field pinned under the list | a **FAB** raising a **bottom sheet** above the keyboard |
| Per-row actions | swipe | an **overflow menu**, which announces itself |
| Editing | a sheet with a form | a bottom sheet with **FilterChips** for tags |
| The tag an item is under | an emoji beside the name | Material's **supporting line** |
| Errors | an alert | a **Snackbar** |
| Colour | a fixed palette | **dynamic colour** from the wallpaper, Android 12+ |

## Running it

```sh
export JAVA_HOME=/opt/homebrew/opt/openjdk@21
./gradlew :app:installDebug
adb shell am start -n com.cernauskas.shoppinglist/.MainActivity
```

`JAVA_HOME` matters: Android Studio bundles JDK 25, which AGP rejects with nothing but
the version number to say so. `gradle.properties` pins the daemon to 21, but the
`gradlew` launcher itself reads the environment.

## Configuration

`local.properties` (not committed — copy `local.properties.example`):

* `sdk.dir` — the SDK
* `googleWebClientId` — the **web** client id. Credential Manager takes it as its
  `serverClientId` and the token comes back addressed to it. The *Android* OAuth
  client, registered against `com.cernauskas.shoppinglist` and the signing
  certificate's SHA-1, is what lets Google attest the app and is named nowhere in this
  code.

## The emulator, and only the emulator

Debug builds point at `http://10.0.2.2:8080`, which is the machine running the
emulator. `res/xml/network_security_config.xml` permits cleartext to that host and
nothing else, so a build aimed at a real server cannot quietly fall back to plain
HTTP.

The system image must include Play services — `google_apis_playstore` — because
Credential Manager cannot sign in without them.
