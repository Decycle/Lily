# Lily on Android

The Android app is the same Rust game in a small `NativeActivity` app (`app.lily`). It contains no game data.
On first start it asks for the web address of a **Lily pack** (your `assets/` and `data/` folders in one file, made
on a computer from your own WACCA Reverse) and downloads it into the app's folder. It needs Android 8.0 or newer on
a 64-bit ARM device (almost every phone and tablet since 2017). A tablet is recommended: on a phone the ring is
small. Background movies are not supported yet.

## Install

1. Download `Lily-android.apk` from the Releases page on the tablet.
2. Open it. Android asks to allow installing apps from your browser or file manager: allow it once.
3. Open **Lily**.

Updates install over the old version and keep the game files and the save.

## Get the game files

1. On a computer, run Lily's setup once and make a pack, then share it (see
   [ios/README.md](../ios/README.md#1-make-a-lily-pack-on-your-mac): the steps are the same):
   ```
   tools/make-pack.sh <game-dir> lily-pack.tar
   tools/serve-pack.sh lily-pack.tar
   ```
2. On the tablet, tap **ENTER ADDRESS**, type the address `serve-pack.sh` printed and tap **Download**. Keep Lily
   open until it says SETUP COMPLETE (the screen stays on by itself).

The files live in `Android/data/app.lily/files` (`assets/`, `data/`, `save/`). Uninstalling Lily deletes them.

## Playing

- **Back button:** pauses in play (RESUME / RETRY / QUIT), goes back in the menus.
- **Pause:** also the **II** button in the top-right corner.
- **Touch guide:** on by default (OPTIONS → TOUCH GUIDE).
- **Timing:** audio delay differs a lot between Android devices. Use OPTIONS → TIMING CHECK once.

## Build

Needs the Android SDK and NDK (Android Studio → SDK Manager → NDK), JDK 17 or 21, `rustup target add
aarch64-linux-android` and `cargo install cargo-ndk`.

```
android/build-rust.sh                      # libwacca_app.so into app/src/main/jniLibs
cd android && ./gradlew assembleDebug      # app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

A release build (`./gradlew assembleRelease`) is signed with the key named by `LILY_KEYSTORE`,
`LILY_KEYSTORE_PASSWORD`, `LILY_KEY_ALIAS` and `LILY_KEY_PASSWORD` (debug key without them). Android installs an
update only if it is signed with the same key as the installed app, so keep the release key safe.

Logs: `adb logcat -s lily`. In the emulator, the computer running `serve-pack.sh` is `10.0.2.2`.

## Status

Tested in the Android 14 tablet emulator (2560 × 1600): installs, downloads a pack (395 songs), reaches the title
with menu music. Not yet tested on a real device: play, touch and audio latency.
