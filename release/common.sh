#!/usr/bin/env bash
#
# What the three release scripts agree about.
#
# Sourced, not run. Everything here is a fact both platforms need to state the same
# way -- above all the build number, which has to increase on every build somebody
# might install over an older one, on both platforms, or the install is refused with
# a message about nothing in particular.

# The build number.
#
# `git rev-list --count HEAD` rather than a counter in a file: it increases on its
# own, it is the same number on every machine with the same history, and it needs no
# commit of its own to change. The marketing version -- 0.1, and what a person sees
# -- is edited by hand and lives in project.yml and build.gradle.kts.
#
# **It only increases if the history does not shrink.** A rebase that drops commits
# lowers it, and the next build then looks older than the one already installed.
# `BUILD_NUMBER` in the environment is the way out of that, and the reason it is
# overridable at all.
build_number() {
    echo "${BUILD_NUMBER:-$(git -C "$ROOT" rev-list --count HEAD)}"
}

# The ten characters from developer.apple.com, read from the file that already holds
# them. Named in one place: Config.xcconfig is gitignored and personal, and a team id
# copied into a second file is a team id that will disagree with the first one day.
apple_team() {
    local config="$ROOT/ios/Config.xcconfig"
    [ -f "$config" ] || {
        echo "ios/Config.xcconfig does not exist. Copy Config.example.xcconfig and fill it in." >&2
        return 1
    }
    local team
    team=$(sed -n 's/^[[:space:]]*DEVELOPMENT_TEAM[[:space:]]*=[[:space:]]*\([A-Z0-9]*\).*/\1/p' "$config" | head -1)
    [ -n "$team" ] || {
        echo "DEVELOPMENT_TEAM is not set in ios/Config.xcconfig." >&2
        return 1
    }
    echo "$team"
}

# Refuses a tool that is not installed, by name, before anything long has run.
needs() {
    for tool in "$@"; do
        command -v "$tool" >/dev/null || {
            echo "$tool is not installed" >&2
            return 1
        }
    done
}

# A copy of an export options file with the team filled in.
#
# Written to the output directory rather than edited in place: the committed file
# holds a placeholder, so a team id never arrives in a diff.
export_options_with_team() {
    local template="$1" team="$2" out="$3"
    sed "s/TEAM_ID_GOES_HERE/$team/" "$template" > "$out"
}

# Says what happened, in the shape the other scripts use.
say() { printf '\n  %s\n' "$*"; }

# The directory all three scripts fill, and the one you upload.
#
# One directory rather than three, because the page that indexes it links to its
# siblings by relative name: a .dmg in out/mac and an .apk in out/android would be two
# links that 404 from a page in out/ios. Each script clears only what it made, so
# building one platform does not throw away yesterday's build of another.
install_dir() { echo "$ROOT/release/out/install"; }

# The index page, with the two placeholders filled in.
#
# Separate from any one platform's script because it describes all three, and the
# alternative -- generating it from whichever script happened to run last -- means the
# page is missing whenever you rebuild only the Mac.
write_page() {
    local base="$1" version="$2" out="$3"
    sed -e "s|BASE_URL_GOES_HERE|$base|g" \
        -e "s|VERSION_GOES_HERE|$version|g" \
        "$ROOT/release/download.html" > "$out/index.html"
}

# Refuses anything the device will refuse, before a long build rather than after.
check_base_url() {
    case "$1" in
        https://*) ;;
        *) echo "BASE_URL must be https:// -- iOS refuses over-the-air installation over anything else" >&2; return 1 ;;
    esac
}
