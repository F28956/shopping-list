#!/usr/bin/env bash
#
# The iPhone and the Watch, as one file somebody can install from a link.
#
# Ad Hoc rather than TestFlight, for a household: the provisioning profile is good
# for a year where a TestFlight build expires after ninety days, and nothing goes
# near Apple between running this and somebody tapping the link. The cost is that
# every device has to be registered by UDID at developer.apple.com first -- a hundred
# of them per year, which is ninety-nine more than most people running their own
# server need.
#
# **The watch app is not installed separately and cannot be.** It is embedded in the
# phone app (`embed: true`, ios/project.yml), so it travels inside this .ipa; the
# Watch app on the phone pushes it across once the phone app is installed.
#
# Over-the-air installation is an Apple protocol with three requirements, all of them
# unforgiving:
#
#   1. The manifest and the .ipa are served over **HTTPS with a certificate the
#      device already trusts.** A private CA fails here exactly as it does in the app.
#   2. The link is `itms-services://?action=download-manifest&url=<manifest>`, and it
#      has to be tapped in Safari. Other browsers on iOS do not handle the scheme.
#   3. The manifest names the .ipa by absolute URL. There is no relative form, which
#      is why BASE_URL is required rather than guessed.
#
# Usage:
#
#   BASE_URL=https://list.example.com/install release/ios.sh
#
# It writes release/out/install/ -- upload that directory to that address.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=release/common.sh
. "$ROOT/release/common.sh"

BASE="${BASE_URL:?set BASE_URL to the https:// directory these files will be served from}"
# Trailing slashes make double slashes in the manifest, which some CDNs turn into a
# 404 and Safari reports as "cannot connect to the server".
BASE="${BASE%/}"

# Refused here rather than by the device, whose refusal is a dialog saying "Cannot
# connect" with no mention of the scheme.
check_base_url "$BASE"

needs xcodebuild xcodegen sips plutil

# `-allowProvisioningUpdates` is passed to both xcodebuild calls below, and without it
# automatic signing does not work from a command line at all: Xcode.app creates and
# downloads profiles silently, `xcodebuild` refuses to and says "Automatic signing is
# disabled", which is a confusing way to describe a missing flag. The watch app is
# where it bites first, because its profile is the one nobody has ever made by hand.
#
# It talks to Apple, so the first run of the day may ask the keychain for permission.

# Checked now rather than after the archive. Both icons are made from it, and the
# making happens at the end -- so a missing file would otherwise cost a full build
# before saying so.
ICON="$ROOT/branding/app-store-icon-1024.png"
[ -f "$ICON" ] || { echo "$ICON is missing; the install dialog needs an icon" >&2; exit 1; }

TEAM=$(apple_team)
BUILD=$(build_number)
OUT="$(install_dir)"
WORK="$ROOT/release/out/work-ios"

# Only this platform's files. A Mac build from yesterday stays where it is.
rm -rf "$WORK"
rm -f "$OUT/ShoppingList.ipa" "$OUT/manifest.plist" "$OUT/icon-57.png" "$OUT/icon-512.png"
mkdir -p "$OUT" "$WORK"

say "Team $TEAM, build $BUILD, for $BASE"

# The project file is generated and gitignored, so it may be absent or stale.
( cd "$ROOT/ios" && xcodegen generate )

# CURRENT_PROJECT_VERSION is passed on the command line rather than written into
# project.yml, because a setting given to xcodebuild outranks every file -- which is
# what makes the build number a property of this run and not of the checkout.
say "Archiving"
xcodebuild archive \
    -project "$ROOT/ios/ShoppingList.xcodeproj" \
    -scheme ShoppingList \
    -configuration Release \
    -destination 'generic/platform=iOS' \
    -archivePath "$WORK/ShoppingList.xcarchive" \
    CURRENT_PROJECT_VERSION="$BUILD" \
    -allowProvisioningUpdates \
    -quiet

export_options_with_team \
    "$ROOT/release/ExportOptions-adhoc.plist" "$TEAM" "$WORK/ExportOptions.plist"

say "Exporting"
xcodebuild -exportArchive \
    -archivePath "$WORK/ShoppingList.xcarchive" \
    -exportOptionsPlist "$WORK/ExportOptions.plist" \
    -exportPath "$WORK/export" \
    -allowProvisioningUpdates \
    -quiet

IPA=$(find "$WORK/export" -name '*.ipa' | head -1)
[ -n "$IPA" ] || { echo "the export produced no .ipa" >&2; exit 1; }
cp "$IPA" "$OUT/ShoppingList.ipa"

# The version a person sees, read back from what was actually built rather than
# retyped here -- the two would drift, and the manifest is what the install dialog
# quotes.
PLIST="$WORK/ShoppingList.xcarchive/Products/Applications/ShoppingList.app/Info.plist"
VERSION=$(plutil -extract CFBundleShortVersionString raw "$PLIST")
BUNDLE=$(plutil -extract CFBundleIdentifier raw "$PLIST")

# Two icons, because the install dialog draws one and the home screen placeholder
# draws the other while it downloads. Absent, iOS shows a grey square and people
# think the wrong app is arriving.
sips -Z 57 "$ICON" --out "$OUT/icon-57.png" >/dev/null
sips -Z 512 "$ICON" --out "$OUT/icon-512.png" >/dev/null

cat > "$OUT/manifest.plist" <<PLIST_END
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>items</key>
  <array>
    <dict>
      <key>assets</key>
      <array>
        <dict>
          <key>kind</key><string>software-package</string>
          <key>url</key><string>$BASE/ShoppingList.ipa</string>
        </dict>
        <dict>
          <key>kind</key><string>display-image</string>
          <key>url</key><string>$BASE/icon-57.png</string>
        </dict>
        <dict>
          <key>kind</key><string>full-size-image</string>
          <key>url</key><string>$BASE/icon-512.png</string>
        </dict>
      </array>
      <key>metadata</key>
      <dict>
        <key>bundle-identifier</key><string>$BUNDLE</string>
        <key>bundle-version</key><string>$VERSION</string>
        <key>kind</key><string>software</string>
        <key>title</key><string>Shopping List</string>
      </dict>
    </dict>
  </array>
</dict>
</plist>
PLIST_END

# Malformed XML here is a silent failure on the device -- Safari says it cannot
# connect and never mentions the manifest -- so it is checked while somebody is
# watching.
plutil -lint "$OUT/manifest.plist" >/dev/null

write_page "$BASE" "$VERSION ($BUILD)" "$OUT"

say "Done. release/out/install/ holds:"
printf '    %s\n' "$(cd "$OUT" && ls)"
say "Upload that directory to $BASE, then open $BASE/ in Safari on the device."
echo
echo "  The link Safari must be given is:"
echo "    itms-services://?action=download-manifest&url=$BASE/manifest.plist"
echo
