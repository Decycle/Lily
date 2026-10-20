#!/bin/sh
# Builds the game (crates/wacca-app) as libwacca_app.so for arm64 Android phones and tablets into
# app/src/main/jniLibs, where Gradle packages it. Needs the Android NDK (ANDROID_NDK_HOME, or the
# newest one in the SDK), `rustup target add aarch64-linux-android` and `cargo install cargo-ndk`.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
sdk=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    ANDROID_NDK_HOME=$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)
    export ANDROID_NDK_HOME
fi
[ -d "${ANDROID_NDK_HOME:-}" ] || { echo "error: Android NDK not found (set ANDROID_NDK_HOME)" >&2; exit 1; }
cd "$here/.."
cargo ndk -t arm64-v8a -P 26 rustc --locked --release -p wacca-app --lib --crate-type cdylib
mkdir -p "$here/app/src/main/jniLibs/arm64-v8a"
cp target/aarch64-linux-android/release/libwacca_app.so "$here/app/src/main/jniLibs/arm64-v8a/"
echo "built $here/app/src/main/jniLibs/arm64-v8a/libwacca_app.so"
