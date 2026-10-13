# Contributing

## Layout

| Crate | Owns | Depends on |
|---|---|---|
| `wacca-chart` | `.mer` model, parser, tempo/scroll timing | — |
| `wacca-game` | rules, judgement engine, score/gauge/grade/rating, save data, song catalog types | chart |
| `wacca-ue4` | UE 4.19 cooked package reader (DataTables, textures) | — |
| `wacca-audio` | CRI @UTF/ACB/AFS2, HCA/ADX decode, audio output + clock | — |
| `wacca-render` | wgpu playfield/notes/effects | chart |
| `wacca-video` | `.usm` demux, background-movie decode via an external `ffmpeg` process (`WACCA_FFMPEG`, `PATH` or winget) | — |
| `wacca-convert` | `assets/` → `data/` library + CLI | ue4, game |
| `wacca-setup` | first-run import: Reverse `.7z` archives (via 7-Zip's CLI) or an installed game → portable `assets/` (runtime subset) + `data/`; the read set is documented in `readset.rs` | convert, audio, chart, game |

Shared vocabulary lives in `wacca-chart::model` and `wacca-game::{rules, input, catalog}`. Change those only when
necessary, keep the change minimal, and say so in the PR.

## Rules

- **Small and focused.** No speculative abstractions, no dead code, no unused dependencies. Prefer std.
- **Pure core, thin IO.** Parsing/logic functions take `&[u8]`/`&str`/structs and return values; file and device IO
  stay at the edges so everything important is unit-testable without assets or hardware.
- **Errors:** library code returns `Result` with a `thiserror` enum; no `unwrap`/`expect`/`panic!` on data paths
  (tests may unwrap). Malformed-but-recoverable data → `log::warn!` and continue.
- **Debuggable:** `Debug` on all public types; `log` for diagnostics (no `println!` in libs).
- **Tests:** every behaviour gets a unit test with a small inline fixture. Tests that need the real game files read
  the assets path from the `WACCA_ASSETS` env var (e.g. `WACCA_ASSETS=assets` when the game's `Content` folder is copied to `assets/`)
  and are `#[ignore]` so `cargo test` passes on a clean checkout; run them with `cargo test -- --ignored`.
- **Never commit game data.** `assets/`, `data/`, extracted samples and table dumps stay out of git. Fixtures must be
  tiny, hand-written, or synthetic.
- Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## Specs

Research docs in `docs/research/` are the source of truth; anything marked UNVERIFIED should be implemented as the doc
recommends and noted with a `// UNVERIFIED:` comment so it is easy to find later. Plan: `docs/PLAN.md`.

## Toolchain

Rust stable, `x86_64-pc-windows-gnu`. Cargo lives in `%USERPROFILE%\.cargo\bin` (add to `PATH` in shells that lack it).

**Pinned dependencies.** Crates that depend on `windows-link` 0.2 import Windows DLLs via `raw-dylib`, which on
`windows-gnu` needs MinGW's `dlltool` *and* assembler (`as`); the rustup GNU toolchain ships only `dlltool`, so the
build fails. Until a full MinGW-w64 is installed we stay below that line: `wgpu` 27 (28+ uses `windows` 0.62), and in
`Cargo.lock` `libloading` 0.8.8, `parking_lot` 0.12.4 / `parking_lot_core` 0.9.11; `env_logger` is used without
default features (its colour support pulls `windows-sys` 0.61). Do not run a bare `cargo update`; update single
crates with `cargo update -p <crate>` and check that `cargo tree --target x86_64-pc-windows-gnu -i windows-link`
prints nothing.

**Other platforms.** The workspace also builds on Linux (needs `libasound2-dev` / `alsa-lib-devel` and
`pkg-config` for `cpal`) and macOS (wgpu's Metal backend). Windows-only code (the real-game harness's Win32 FFI,
process creation flags, the setup's folder picker) is behind `#[cfg(windows)]` with a non-Windows fallback. Check a
change on other targets without their linkers using `rustup target add x86_64-unknown-linux-gnu aarch64-apple-darwin`
and `cargo clippy --workspace --all-targets --target <triple> -- -D warnings` (on Windows, the Linux check needs
`PKG_CONFIG_ALLOW_CROSS=1` and a `PKG_CONFIG` stub that prints `-lasound`, because `alsa-sys` asks `pkg-config`).

## CI

`.github/workflows/ci.yml` runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo test --workspace` (non-ignored tests) on `windows-latest`, `ubuntu-latest` and `macos-latest`. Windows CI uses
the same `stable-x86_64-pc-windows-gnu` toolchain as local development and fails if `windows-link` enters the
dependency tree: the rustup GNU toolchain has `dlltool` but no assembler, so the pins above are what keeps it
building without a full MinGW-w64. `.github/workflows/release.yml` (manual) builds the Windows portable zip.
