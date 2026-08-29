#!/bin/bash
# Compiles the device's own server for Android, into the jniLibs the APK packages.
#
# The twin of build-parser.sh, and separate for the same reason the Apple side has two:
# they are different libraries with different reasons to be rebuilt, and one script that
# did both would rebuild the world whenever either moved.
#
# What this carries is not a copy of the server — it *is* the server. `web/embedded`
# links `domain`, so an Android phone with no connection runs the same service layer
# over the same schema as the machine in the cupboard. See web/embedded/src/jni.rs.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${here}/../../web"

sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-${HOME}/Library/Android/sdk}}"
ndk="${ANDROID_NDK_HOME:-$(ls -d "${sdk}"/ndk/* 2>/dev/null | sort -V | tail -1)}"
if [ ! -d "${ndk}" ]; then
    echo "error: no Android NDK. Install one with sdkmanager 'ndk;27.2.12479018'." >&2
    exit 1
fi

bin="${ndk}/toolchains/llvm/prebuilt/$(uname -s | tr 'A-Z' 'a-z')-x86_64/bin"
# 26, matching `minSdk` in app/build.gradle.kts — see build-parser.sh for why that
# number and not a newer one.
api=26

declare -a targets=(aarch64-linux-android x86_64-linux-android)
declare -a abis=(arm64-v8a x86_64)

if [ "${1:-debug}" = "release" ]; then
    profile=release
    dir=release
else
    profile=dev
    dir=debug
fi

out="${here}/../app/src/main/jniLibs"

for n in "${!targets[@]}"; do
    target="${targets[$n]}"
    upper="$(echo "${target}" | tr 'a-z-' 'A-Z_')"
    export CARGO_TARGET_${upper}_LINKER="${bin}/${target}${api}-clang"
    export CC_${target//-/_}="${bin}/${target}${api}-clang"
    export AR_${target//-/_}="${bin}/llvm-ar"

    "${HOME}/.cargo/bin/cargo" build -p embedded --target "${target}" --profile "${profile}"

    mkdir -p "${out}/${abis[$n]}"
    cp "target/${target}/${dir}/libembedded.so" "${out}/${abis[$n]}/libembedded.so"
done
