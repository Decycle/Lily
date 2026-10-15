#!/bin/sh
# Xcode build phase: builds the game as a static library for the device or the simulator and
# puts it where the link step expects it ($TARGET_TEMP_DIR/rust/libwacca_app.a). Always a release
# build: a debug build of the renderer and audio mixer is too slow to play.
set -eu
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
case "$PLATFORM_NAME" in
    iphoneos) target=aarch64-apple-ios ;;
    iphonesimulator) target=aarch64-apple-ios-sim ;;
    *) echo "error: unsupported platform $PLATFORM_NAME" >&2; exit 1 ;;
esac
cd "$SRCROOT/.."
# Xcode's SDKROOT is the iOS SDK; build scripts compiled for the Mac must not see it (cc-rs finds
# the right SDK per target through xcrun).
env -u SDKROOT cargo rustc --locked --release -p wacca-app --lib --crate-type staticlib \
    --target "$target"
mkdir -p "$TARGET_TEMP_DIR/rust"
cp "target/$target/release/libwacca_app.a" "$TARGET_TEMP_DIR/rust/libwacca_app.a"
