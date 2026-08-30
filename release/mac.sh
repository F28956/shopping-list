#!/usr/bin/env bash
#
# The Mac app, as a disk image anybody can download and open.
#
# Developer ID rather than the App Store: no device list, no expiry, no review. This
# is the only one of the three channels here with none of those, and it is why the Mac
# app is the easiest of the three to hand somebody.
#
# Notarisation is not review. It is an automated malware scan that returns in a few
# minutes and refuses almost nothing; what it buys is that Gatekeeper opens the app
# on the first double-click instead of telling the person it cannot be checked.
#
# **The .dmg is notarised, not the .app.** Stapling the ticket to the disk image means
# it travels with the file people actually download, so a first launch works on a
# machine that is offline. Notarising the app alone leaves the download unstapled and
# Gatekeeper has to ask Apple at the worst possible moment.
#
# Once, before this will work -- an app-specific password from appleid.apple.com,
# stored in the keychain so it is never in a shell history or a script:
#
#   xcrun notarytool store-credentials shopping-list \
#       --apple-id you@example.com --team-id ABCDE12345 --password xxxx-xxxx-xxxx-xxxx
#
# Usage:
#
#   release/mac.sh
#   NOTARY_PROFILE=other-name release/mac.sh    # if you named it something else
#
# Set SKIP_NOTARISE=1 to build a signed but unstapled image for local testing. Do not
# hand that to anybody: it opens on the machine that built it and nowhere else.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=release/common.sh
. "$ROOT/release/common.sh"

PROFILE="${NOTARY_PROFILE:-shopping-list}"

needs xcodebuild xcodegen hdiutil plutil

TEAM=$(apple_team)
BUILD=$(build_number)
OUT="$(install_dir)"
WORK="$ROOT/release/out/work-mac"

rm -rf "$WORK"
rm -f "$OUT/ShoppingList.dmg"
mkdir -p "$OUT" "$WORK/stage"

say "Team $TEAM, build $BUILD"

( cd "$ROOT/ios" && xcodegen generate )

say "Archiving"
xcodebuild archive \
    -project "$ROOT/ios/ShoppingList.xcodeproj" \
    -scheme ShoppingListMac \
    -configuration Release \
    -destination 'generic/platform=macOS' \
    -archivePath "$WORK/ShoppingListMac.xcarchive" \
    CURRENT_PROJECT_VERSION="$BUILD" \
    -allowProvisioningUpdates \
    -quiet

export_options_with_team \
    "$ROOT/release/ExportOptions-developer-id.plist" "$TEAM" "$WORK/ExportOptions.plist"

say "Exporting"
xcodebuild -exportArchive \
    -archivePath "$WORK/ShoppingListMac.xcarchive" \
    -exportOptionsPlist "$WORK/ExportOptions.plist" \
    -exportPath "$WORK/export" \
    -allowProvisioningUpdates \
    -quiet

APP=$(find "$WORK/export" -maxdepth 1 -name '*.app' | head -1)
[ -n "$APP" ] || { echo "the export produced no .app" >&2; exit 1; }

VERSION=$(plutil -extract CFBundleShortVersionString raw "$APP/Contents/Info.plist")
# A stable name rather than one carrying the version. The index page links to it,
# and anything watching the address for updates wants a URL that does not move; the
# version is in the app's Info.plist and on the page.
DMG="$OUT/ShoppingList.dmg"

# A symlink to /Applications beside the app, so the disk image is the drag-and-drop
# window people already know rather than one that needs an explanation.
cp -R "$APP" "$WORK/stage/"
ln -s /Applications "$WORK/stage/Applications"

say "Building the disk image"
hdiutil create \
    -volname "Shopping List" \
    -srcfolder "$WORK/stage" \
    -ov -format UDZO \
    "$DMG" >/dev/null

if [ "${SKIP_NOTARISE:-}" = "1" ]; then
    say "SKIP_NOTARISE=1 -- $DMG is signed but not notarised. It will not open on another Mac."
    exit 0
fi

say "Notarising. This takes a few minutes."
# `--wait` blocks until Apple answers. Without it the script would finish while the
# submission was still queued and the stapling below would fail for a reason that
# reads like a signing problem.
xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait

say "Stapling"
xcrun stapler staple "$DMG"

# Asks Gatekeeper the question the person downloading it will ask, on this machine,
# now -- rather than discovering the answer from somebody else's screenshot.
say "Checking it the way Gatekeeper will"
spctl --assess --type open --context context:primary-signature -v "$DMG"

say "Done: release/out/install/ShoppingList.dmg ($VERSION, build $BUILD)"
echo
