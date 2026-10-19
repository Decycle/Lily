# Notice

## WACCA

WACCA and WACCA Reverse are © SEGA and © Marvelous Inc. All related names, music, charts, artwork, fonts, movies,
sounds, meshes and other game data belong to their respective owners.

Lily is an unofficial, non-commercial fan project. It is **not affiliated with, endorsed by, or connected to** SEGA,
Marvelous, or any rights holder of WACCA. "Lily" is also the name of a WACCA version (WACCA Lily); this project has
no connection to it beyond the name.

**This repository contains no game assets.** It contains only original source code, documentation written for this
project, and WGSL shaders written to reproduce the look of the original game's materials. All charts, audio,
textures, fonts, movies and meshes are read at run time from a copy of the game that you supply. You must use your
own, legally obtained copy of WACCA Reverse. Do not redistribute game files, converted game data
(`data/`), or screenshots and recordings that would substitute for the game.

Lily's own code is released under the MIT license ([LICENSE](LICENSE)). That license covers this project's code
only; it grants no rights to any WACCA content.

## Third-party software

### Rust crates (compiled into the binaries)

Lily is built from about 300 crates from crates.io, listed with exact versions in `Cargo.lock`. Their licenses,
taken from each crate's `license` field (`cargo metadata`):

| License | Crates (examples) |
|---|---|
| MIT OR Apache-2.0 (most crates) | wgpu, naga, serde, serde_json, image, png, log, thiserror, parking_lot, env_logger, ... |
| MIT | cridecoder (HCA/ADX decoding), objc2 and wayland/x11 platform crates, alsa-sys, tokio, ... |
| Apache-2.0 | winit, cpal, ab_glyph, ab_glyph_rasterizer, owned_ttf_parser, codespan-reporting, spirv, gethostname |
| Unlicense OR MIT | memchr, aho-corasick, byteorder, walkdir, same-file |
| Zlib (or Zlib OR MIT OR Apache-2.0) | foldhash, zlib-rs, miniz_oxide, bytemuck, raw-window-handle, ... |
| BSD-2-Clause / BSD-3-Clause (alone or as an option) | arrayref, tiny-skia, tiny-skia-path, encoding_rs (with MIT/Apache-2.0), moxcms, num_enum, zerocopy |
| ISC | libloading |
| CC0-1.0 | hexf-parse |
| Unicode-3.0 (with MIT OR Apache-2.0) | unicode-ident |

No crate in the dependency tree is GPL-only. To regenerate the full list, run
`cargo metadata --format-version 1` and read each package's `license`, or install `cargo-license`. Binary releases
must include these licenses' notices; the release workflow is the place to add an auto-generated
`THIRD-PARTY-LICENSES` file.

### FFmpeg (optional, external, not bundled)

Background movies are decoded by running an external `ffmpeg` executable that the user installs separately
(`WACCA_FFMPEG`, `PATH`, or winget's install folder). FFmpeg is not part of this repository or of its releases. FFmpeg
is licensed under the LGPL 2.1 or later, or the GPL, depending on how the build you install was configured. Without
FFmpeg, Lily runs without background movies.

### 7-Zip (portable Windows build only)

The portable Windows release bundles a 7-Zip command-line binary so first-run setup can unpack the game's update
archives. 7-Zip is © Igor Pavlov and is licensed under the GNU LGPL 2.1 (most of the code), with parts under the
BSD 3-clause license and the unRAR license restriction. It is used unmodified as a separate program; its license file
ships next to it in the release, and its source code is available from the 7-Zip project. 7-Zip is not part of
this repository.
