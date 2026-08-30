# Handing it to somebody

Three signed builds, none of them through a store. The reasoning for going this way
rather than the store is in [docs/self-hosting.md](../docs/self-hosting.md); this is
what to type.

All three write into `release/out/install/`. That one directory is what you upload —
it holds the builds, an over-the-air manifest for iOS, and an `index.html` that
indexes them.

## Once, before any of it

| For | What | Where |
|---|---|---|
| iPhone, Watch, Mac | Apple Developer Program, and `DEVELOPMENT_TEAM` in `ios/Config.xcconfig` | already done if the app builds |
| Mac only | notarisation credentials in the keychain | see below |
| Android | a keystore, and `android/keystore.properties` | see `keystore.properties.example` |

Notarisation needs an app-specific password from appleid.apple.com, stored once so it
is never in a shell history:

```bash
xcrun notarytool store-credentials shopping-list --apple-id you@example.com --team-id ABCDE12345
```

## iPhone and Apple Watch

```bash
BASE_URL=https://list.example.com/install release/ios.sh
```

Ad Hoc, so **every device has to be registered by UDID** at developer.apple.com first
— a hundred per year, which is ninety-nine more than most people running their own
server need. In exchange the profile lasts a year, where a TestFlight build expires
after ninety days, and Apple is not involved between running this and somebody tapping
the link.

**There is nothing to install on the Watch.** The watch app is embedded in the phone
app, so it travels inside the `.ipa`; the Watch app on the phone pushes it across once
the phone app lands.

Over-the-air installation has three requirements and no useful error messages for any
of them:

1. **HTTPS, with a certificate the device already trusts.** A private CA fails here
   exactly as it does inside the app. `BASE_URL` is refused if it is not `https://`.
2. **Safari.** Other iOS browsers ignore `itms-services://` links silently, which
   reads as a dead button.
3. **Absolute URLs in the manifest**, which is why `BASE_URL` is required rather than
   worked out.

## Mac

```bash
release/mac.sh
```

Developer ID and notarisation: no device list, no expiry, no review. This is the only
one of the three with none of those, and it is why the Mac app is the easiest to hand
somebody — a `.dmg` they download and open.

The **disk image** is notarised and stapled, not just the app inside it, so the ticket
travels with the file people actually download and a first launch works offline.

`SKIP_NOTARISE=1` builds a signed but unstapled image for looking at locally. It opens
on the machine that built it and nowhere else, so do not hand it to anybody.

## Android

```bash
release/android.sh
```

An **APK**, not an App Bundle — an `.aab` is a description of the APKs Google Play
would build and only Play can open it. No Play Console is involved and none is needed.

Two things about the keystore, both of which are worse than they sound:

* **Losing it means nobody can update.** Android identifies an app by its package *and*
  its signing certificate. A different key produces an app the device refuses to
  install over the old one, and the only way through is for every person holding it to
  uninstall first — which on this app throws away any list their device has not sent to
  a server.
* **Its SHA-1 is registered with Google** against `com.cernauskas.shoppinglist`. A new
  key means sign-in fails in exactly the builds you hand out, and works in every build
  you test.

## Build numbers

`versionCode` on Android and `CURRENT_PROJECT_VERSION` on Apple both come from
`git rev-list --count HEAD`, so they are the same number, they increase on their own,
and neither needs a commit of its own to change. Both platforms refuse an install
whose build number is not greater than the one already there, and neither says so in
those words.

It only increases while the history does not shrink. A rebase that drops commits
lowers it, and the next build then looks older than the one already installed.
`BUILD_NUMBER=n` in the environment overrides it, and that is what it is for.

The marketing version — `0.1`, what a person sees — is edited by hand, in
`ios/project.yml` and `android/app/build.gradle.kts`.

## After uploading

The page at `BASE_URL/` explains the rest to whoever you send it to, including the
part people trip on: **the server address has to be `https://`.** These are release
builds and they refuse plain HTTP, so a home server on cleartext cannot be reached by
any of them. See the HTTPS section of [ops/README.md](../ops/README.md).
