# Lily on iPad

The iPad app is the same Rust game, linked into a small Xcode app. It contains no game data. On
first start it asks for the web address of a **Lily pack**: one file holding the `assets/` and
`data/` folders that Lily's setup made on a computer from your own WACCA Reverse 3.07.01. The app
downloads the pack over your network into its Documents folder. The pack is about 4 GB. It
resumes if the Wi-Fi drops and keeps the screen on while it runs. Background movies are not
supported on iPad.

## 1. Make a Lily pack (on your Mac)

Run Lily's setup once on the Mac. The folder picker is Windows-only, so use the command line
(needs `brew install sevenzip`):

```
cargo run --release -p wacca-setup -- ~/Downloads --dest ~/Documents/lily-game
tools/make-pack.sh ~/Documents/lily-game lily-pack.tar
```

To check the setup on the Mac first:
`cargo run --release -p wacca-app --bin lily -- --assets ~/Documents/lily-game/assets --data ~/Documents/lily-game/data --windowed`

## 2. Share it

```
tools/serve-pack.sh lily-pack.tar
```

It prints the address to enter, for example `http://192.168.1.20:8000/lily-pack.tar`. The iPad
must be on the same network. Keep it running until the iPad shows SETUP COMPLETE. Any web server
works if it supports HTTP `Range` (for resuming). Don't put your pack on a public server: it is
your copy of SEGA's game data.

## 3. Build and install

Needs full Xcode (App Store), `rustup target add aarch64-apple-ios aarch64-apple-ios-sim` and
`brew install xcodegen`.

1. Create `ios/Local.xcconfig` (git-ignored) with your Team ID (developer.apple.com → Membership
   details) and a bundle id of your own:
   ```
   DEVELOPMENT_TEAM = A1B2C3D4E5
   PRODUCT_BUNDLE_IDENTIFIER = com.yourname.lily
   ```
2. `cd ios && xcodegen`, then `open Lily.xcodeproj`.
3. On the iPad, turn on Settings → Privacy & Security → Developer Mode. The switch appears after
   Xcode first tries to install to the iPad. Then choose the iPad as the run destination and press
   Run. The build phase compiles the Rust game for iOS (the first build takes a few minutes).

Over the cable you can also copy the game files straight into the app (instead of downloading):
`xcrun devicectl device copy to --device <udid> --domain-type appDataContainer
--domain-identifier <bundle id> --source <game-dir>/assets --destination Documents/assets`, and
the same for `data`.

### TestFlight

1. In App Store Connect, create an app with the same bundle id.
2. In Xcode, choose **Any iOS Device (arm64)**, then **Product → Archive**.
3. Click **Distribute App → App Store Connect → Upload**. Transporter also works with an exported
   `.ipa`.
4. In App Store Connect → TestFlight, add yourself as an internal tester (no review needed), then
   install from the TestFlight app on the iPad. Builds expire after 90 days. Increase
   `CURRENT_PROJECT_VERSION` for every upload.

## Playing on iPad

- **Pause:** the **II** button in the top-right corner, outside the ring, opens RESUME / RETRY / QUIT.
- **Touch guide:** on by default on iPad (OPTIONS → TOUCH GUIDE). It marks where the touch area starts and ends.
  A save made before this default keeps its old setting; turn it on once in OPTIONS.
- **Menu sounds:** the title and menus play the game's system music, and taps make the game's menu sounds. Which
  cue the game plays where is not recorded, so they were picked by length (`crates/wacca-app/src/menu_audio.rs`).
- **Timing:** audio delay on iPad differs from a PC. Use OPTIONS → TIMING CHECK once.

## Notes

- The game files and the save live in the app's Documents folder: **Files → On My iPad → Lily**.
  You can also copy `assets/` and `data/` there by hand (Finder → iPad → Files) instead of
  downloading.
- To replace the game files, use the setup button on the title screen.
- Tested on an iPad (10th generation, iOS 18). The window currently leaves a 50 px strip
  unused (2360 × 1590 on a 2360 × 1640 screen).
- Open points: audio latency on iPad and the size of the ring on smaller iPads.
