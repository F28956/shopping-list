#!/bin/bash
# Compiles the device's own server for whatever Xcode is currently building.
#
# The twin of build-parser.sh, and separate from it deliberately: they build different
# crates into different archives, and one script with a loop over both would be harder
# to read than two that each do one thing.
#
# See web/embedded/src/lib.rs for what this is. In short: a phone kept to itself runs
# the server's own code over the server's own schema, rather than pretending to be a
# server that fails every request.
set -euo pipefail

cd "${SRCROOT}/../web"

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

# A release build of the app that shipped an unoptimised database layer would be a tax
# on every read. `--profile dev` rather than no flag: macOS ships bash 3.2, where
# expanding an empty array under `set -u` is an error rather than nothing.
if [ "${CONFIGURATION}" = "Release" ]; then
    profile=release
    dir=release
else
    profile=dev
    dir=debug
fi

built=()
for target in "${targets[@]}"; do
    "${HOME}/.cargo/bin/cargo" build -p embedded --target "${target}" --profile "${profile}"
    built+=("target/${target}/${dir}/libembedded.a")
done

out="${SRCROOT}/Parser/lib/${PLATFORM_NAME}"
mkdir -p "${out}"
lipo -create "${built[@]}" -output "${out}/libembedded.a"
