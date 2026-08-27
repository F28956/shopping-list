#!/bin/bash
# Compiles the shared parser for whatever Xcode is currently building.
#
# Run as a build phase rather than by hand, because "by hand" means the day somebody
# changes `parsing/src/quick_add.rs`, rebuilds the app, and is served a stale answer
# by a library nobody rebuilt. Cargo is already incremental; when nothing changed this
# costs a fraction of a second.
set -euo pipefail

cd "${SRCROOT}/../web"

# Xcode's names for platforms are not Rust's, and neither are its names for
# architectures. `PLATFORM_NAME` and `ARCHS` are what Xcode exports.
targets=()
for arch in ${ARCHS}; do
    case "${PLATFORM_NAME}:${arch}" in
        iphoneos:arm64)          targets+=(aarch64-apple-ios) ;;
        iphonesimulator:arm64)   targets+=(aarch64-apple-ios-sim) ;;
        iphonesimulator:x86_64)  targets+=(x86_64-apple-ios) ;;
        macosx:arm64)            targets+=(aarch64-apple-darwin) ;;
        macosx:x86_64)           targets+=(x86_64-apple-darwin) ;;
        *)
            echo "error: no Rust target for ${PLATFORM_NAME} ${arch}. Add one here." >&2
            exit 1
            ;;
    esac
done

# Debug builds get a debug library. A release build of the app that shipped an
# unoptimised parser would be a quiet, permanent tax on every line somebody types.
# `--profile dev` rather than no flag at all for the debug case: macOS ships bash 3.2,
# where expanding an empty array under `set -u` is an error rather than nothing.
if [ "${CONFIGURATION}" = "Release" ]; then
    profile=release
    dir=release
else
    profile=dev
    dir=debug
fi

built=()
for target in "${targets[@]}"; do
    # `rustup run` rather than bare cargo: Xcode's PATH is not a login shell's, and a
    # build phase that works in the terminal and not in Xcode is a bad afternoon.
    "${HOME}/.cargo/bin/cargo" build -p quickadd-ffi --target "${target}" --profile "${profile}"
    built+=("target/${target}/${dir}/libquickadd.a")
done

out="${SRCROOT}/Parser/lib/${PLATFORM_NAME}"
mkdir -p "${out}"
# One archive whatever the arch count, so the link step does not have to care. `lipo`
# with a single input is a copy, which keeps this branchless.
lipo -create "${built[@]}" -output "${out}/libquickadd.a"
