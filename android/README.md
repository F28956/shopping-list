# The Android app

Kotlin and Jetpack Compose, Material 3. Every rule that decides what a list *means* —
parsing a typed line, merging a repeat, which tag an item sits under, who may share it —
lives in `domain`, the server's own crate. This app reaches it two ways: over the same
JSON API the browser and the Apple clients use, and — with no server configured — through
`libembedded.so`, which is that crate compiled for the handset.

## Layout

One Gradle module, `app`, and three packages under
`app/src/main/kotlin/com/cernauskas/shoppinglist/`.

| Package | Holds |
|---|---|
| `data` | Models, the `Backend` interface and its conformers, the cache, the outbox, identity, the server address |
| `ui` | Compose screens and the view models behind them |
| `diagnostics` | The rolling log, metrics, and what the settings screen shows about them |
| (root) | `MainActivity` and `ShoppingListApp` — the entry point and the navigation |

Three things in the APK are not Kotlin, are not checked in, and are produced by Gradle
tasks during the build:

| Output | What it is | Task, and what it runs |
|---|---|---|
| `jniLibs/*/libembedded.so` | `web/embedded` — the server's `domain` crate, cross-compiled for the handset | `scripts/build-embedded.sh` |
| `jniLibs/*/libquickadd.so` | `web/quickadd-ffi` — `2 kg apples`, read the same way here as on the server | `scripts/build-parser.sh` |
| `assets/reference.json` | The seeded units and tags, so a device that has never reached a server knows what a kilogram is | copied from `reference/`, which is generated from the migrations |

The two Rust builds are separate scripts on purpose: they are different libraries with
different reasons to be rebuilt, and one script doing both would rebuild the world
whenever either moved. Both need an NDK; `build-embedded.sh` names the one to install if
it cannot find it.

## What answers the app's questions

`Backend` is the interface every view model talks to, and there are three conformers:

* **`Api`** — HTTP, against a configured server.
* **`CachingBackend`** — wraps another one. Keeps the last-loaded answer so the app never
  claims an emptiness it has not verified, and puts writes made with no signal into a
  durable `Outbox` that drains on the next successful load.
* **`LocalBackend`** — over `Embedded`, the JNI binding to `libembedded.so`. A device with
  no server gets real answers from a real database rather than a transport error every
  screen has to be taught to ignore.

`ServerDirectory` decides which is in play, and `Capabilities` is how a screen asks what
this arrangement can do — sharing needs a server, because a share link names one. The
address is stored rather than compiled in: `BuildConfig` is what a fresh install starts
from, not what it is stuck with.

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
