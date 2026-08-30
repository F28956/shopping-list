#!/usr/bin/env bash
#
# The server, built here for a Linux box that is not here.
#
# A VPS is almost always x86_64 and this machine is not, so the binary has to be
# cross-compiled. That is more involved than it sounds for this dependency set: three
# crates in it compile C or assembly rather than Rust, and each needs a compiler that
# targets the far end.
#
#   * `aws-lc-sys`   -- the cryptography behind `jsonwebtoken`. Large, needs cmake.
#   * `ring`         -- the cryptography behind rustls and `rcgen`.
#   * `libsqlite3-sys` -- SQLite itself, compiled in rather than linked from the box.
#
# The toolchain, once:
#
#   brew tap messense/macos-cross-toolchains
#   brew trust messense/macos-cross-toolchains     # Homebrew asks; it is a third-party tap
#   brew install x86_64-unknown-linux-gnu
#   rustup target add x86_64-unknown-linux-gnu
#
# **glibc, not musl.** It links against the box's own C library rather than carrying a
# static copy. This toolchain targets an old glibc, so the result runs on anything
# current; a binary built against a *newer* glibc than the server has fails at exec
# with a message about a version it cannot find, which is the trap this avoids.
#
# Usage:
#
#   release/server-linux.sh
#
# It writes release/out/server/.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=release/common.sh
. "$ROOT/release/common.sh"

TARGET=x86_64-unknown-linux-gnu
OUT="$ROOT/release/out/server"

needs cargo rustup cmake

command -v "$TARGET-gcc" >/dev/null || {
    echo "$TARGET-gcc is not installed. See the note at the top of this script." >&2
    exit 1
}

rustup target list --installed | grep -qx "$TARGET" || {
    echo "the Rust target is missing. Run: rustup target add $TARGET" >&2
    exit 1
}

# Which compiler cargo hands to every crate with a build script, and which linker it
# uses at the end. The underscored spellings are what the `cc` crate reads; the
# upper-case CARGO_TARGET_ one is what cargo itself reads for the link step. Both are
# needed and they are not the same mechanism.
export CC_x86_64_unknown_linux_gnu="$TARGET-gcc"
export CXX_x86_64_unknown_linux_gnu="$TARGET-g++"
export AR_x86_64_unknown_linux_gnu="$TARGET-ar"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$TARGET-gcc"

# cmake builds aws-lc-sys, and needs telling separately -- it does not read the
# variables above.
export CMAKE_C_COMPILER="$TARGET-gcc"
export CMAKE_CXX_COMPILER="$TARGET-g++"

# The queries, checked against the committed `.sqlx` rather than a live database.
# Without this the build wants a DATABASE_URL it can connect to, which a cross build
# has no business needing.
export SQLX_OFFLINE=true

say "Building the server for $TARGET"

( cd "$ROOT/web" && cargo build --release --target "$TARGET" -p server )

BIN="$ROOT/web/target/$TARGET/release/server"
[ -f "$BIN" ] || { echo "the build produced no binary" >&2; exit 1; }

mkdir -p "$OUT"
cp "$BIN" "$OUT/shopping-list-server"

say "Done: release/out/server/shopping-list-server"
# Read back from the file rather than asserted, so a binary that is quietly the wrong
# architecture is caught here and not by the VPS refusing to exec it.
file "$OUT/shopping-list-server"
echo
