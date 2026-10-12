# Contributing

## Layout

| Crate | Owns | Depends on |
|---|---|---|
| `wacca-chart` | `.mer` model, parser, tempo/scroll timing | — |
| `wacca-game` | rules, judgement engine, score/gauge/grade/rating, save data, song catalog types | chart |
| `wacca-ue4` | UE 4.19 cooked package reader (DataTables, textures, widgets) | — |
| `wacca-audio` | CRI @UTF/ACB/AFS2, HCA/ADX decode, audio output + clock | — |
| `wacca-render` | wgpu playfield/notes/effects | chart |
| `wacca-convert` | `assets/` → `data/` CLI | ue4, game |

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
  the assets path from the `WACCA_ASSETS` env var (e.g. `C:\Users\<user>\OneDrive\Documents\WACCA\WACCA_slim\assets`)
  and are `#[ignore]` so `cargo test` passes on a clean checkout; run them with `cargo test -- --ignored`.
- **Never commit game data.** `assets/`, `data/`, extracted samples and table dumps stay out of git. Fixtures must be
  tiny, hand-written, or synthetic.
- Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

## Specs

Research docs in `docs/research/` are the source of truth; anything marked UNVERIFIED should be implemented as the doc
recommends and noted with a `// UNVERIFIED:` comment so it is easy to find later. Plan: `docs/PLAN.md`.

## Toolchain

Rust stable, `x86_64-pc-windows-gnu`. Cargo lives in `%USERPROFILE%\.cargo\bin` (add to `PATH` in shells that lack it).
