#!/usr/bin/env bash
#
# The Android app, as an APK somebody can download and install.
#
# An **APK** and not an App Bundle. An .aab is a format only Google Play can open --
# it is a description of the APKs Play would build, not an installable thing -- so a
# project distributing outside the store wants the format that predates it.
#
# There is no Play Console in any of this and none is needed. The person installing it
# allows their browser to install unknown apps once, and that is the whole ceremony.
# [Obtainium](https://github.com/ImranR98/Obtainium) will do the rest, including
# updates, if you point it at wherever these files end up.
#
# Usage:
#
#   release/android.sh
#
# It writes release/out/install/.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=release/common.sh
. "$ROOT/release/common.sh"

OUT="$(install_dir)"
BUILD=$(build_number)

# Refused before a four-minute Gradle build rather than after it. The build file
# itself is deliberately quiet about a missing keystore -- every debug build and every
# test has to keep working without one -- so this is where it is loud.
if [ ! -f "$ROOT/android/keystore.properties" ]; then
    cat >&2 <<'MISSING'

  android/keystore.properties does not exist, so this build would be unsigned and
  could not be installed on anything.

  Copy android/keystore.properties.example and fill it in. It explains how to make
  the keystore -- and why the file it names must be kept for ever.

MISSING
    exit 1
fi

say "Build $BUILD"

( cd "$ROOT/android" && BUILD_NUMBER="$BUILD" ./gradlew --console=plain assembleRelease )

APK=$(find "$ROOT/android/app/build/outputs/apk/release" -name '*.apk' | head -1)
[ -n "$APK" ] || { echo "the build produced no .apk" >&2; exit 1; }

rm -f "$OUT/shopping-list.apk"
mkdir -p "$OUT"

# The version a person sees, read out of the APK rather than retyped -- the same rule
# the iOS manifest follows, and for the same reason.
AAPT=$(find "${ANDROID_HOME:-$HOME/Library/Android/sdk}/build-tools" -name aapt2 2>/dev/null | sort -V | tail -1)
VERSION="unknown"
if [ -n "$AAPT" ]; then
    VERSION=$("$AAPT" dump badging "$APK" \
        | sed -n "s/.*versionName='\([^']*\)'.*/\1/p" | head -1)
fi

# A stable name, for the same reason the .dmg has one: the page links to it, and
# Obtainium watches an address rather than a directory listing.
cp "$APK" "$OUT/shopping-list.apk"

# Asks the question the device will ask. An APK that is not signed, or signed only
# with the v1 scheme, installs on nothing modern -- and the device's refusal names
# neither problem.
if command -v apksigner >/dev/null || [ -n "$AAPT" ]; then
    SIGNER=$(find "${ANDROID_HOME:-$HOME/Library/Android/sdk}/build-tools" -name apksigner 2>/dev/null | sort -V | tail -1)
    if [ -n "$SIGNER" ]; then
        say "Checking the signature the way the device will"
        "$SIGNER" verify --print-certs "$OUT/shopping-list.apk"
    fi
fi

say "Done: release/out/install/shopping-list.apk ($VERSION, build $BUILD)"
echo
echo "  Its SHA-1 must be registered with Google against com.cernauskas.shoppinglist,"
echo "  or sign-in will fail in this build and work in every build you test."
echo
