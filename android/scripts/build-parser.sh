#!/bin/bash
# Compiles the shared parser for Android, into the jniLibs the APK packages.
#
# Called from Gradle rather than by hand, for the same reason Xcode calls its twin:
# "by hand" means the day somebody changes `parsing/src/quick_add.rs`, rebuilds the
# app, and is served a stale answer by a library nobody rebuilt.
#
# The Apple side of this is ios/Scripts/build-parser.sh. They are deliberately not one
# script -- Xcode says what it is building through the environment, Gradle does not,
# and merging them would mean a script that reads neither clearly.
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
# 26, matching `minSdk` in app/build.gradle.kts. A library built against a newer API
# than the app claims to support links against symbols that are not there on the
# oldest phone it will install on -- and it installs fine, then dies at first call.
api=26

# The arm64 phone and the x86_64 emulator. Not the 32-bit pair: Play has required a
# 64-bit build since 2019, and every emulator image this project uses is x86_64.
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
    # Cargo wants these per-target and spells them in screaming snake case.
    upper="$(echo "${target}" | tr 'a-z-' 'A-Z_')"
    export CARGO_TARGET_${upper}_LINKER="${bin}/${target}${api}-clang"
    export CC_${target//-/_}="${bin}/${target}${api}-clang"
    export AR_${target//-/_}="${bin}/llvm-ar"

    "${HOME}/.cargo/bin/cargo" build -p quickadd-ffi --target "${target}" --profile "${profile}"

    mkdir -p "${out}/${abis[$n]}"
    cp "target/${target}/${dir}/libquickadd.so" "${out}/${abis[$n]}/libquickadd.so"
done
