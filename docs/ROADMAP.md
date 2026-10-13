# Roadmap

Open items, taken from the parity audits ([doc 13](research/13-parity-audit.md),
[doc 14](research/14-menu-parity.md)), the judgement experiments ([doc 10](research/10-e3-judge-results.md)) and
the code's `UNVERIFIED` markers. Contributions are welcome; see [CONTRIBUTING.md](../CONTRIBUTING.md).

## Play area: visuals

- Background scene: the state-change bursts (`vp_bgcup000_00` / `vp_bgcdown000_00`), the end-of-song states
  (ClimaxEffect, Full Combo, Missless, All Marvelous, Clear), the Inferno set, and FXAA (doc 13 §4.1).
- Good-state background triangles are drawn about 1.8× the captured size (GPU sprite size, UNVERIFIED; doc 13 §5).
- HUD banners not drawn yet: BONUS GET, end-of-song "Missless!" / "Full Combo" stamps, the COMBO word's two
  faint colour copies (doc 13 §4.2).
- R-note effect timing/size is within ±1 frame (doc 13 §4.3).

## Play area: rules

- Gauge: `NormaPointMax` formula, clamping and bonus handling (UNVERIFIED, `wacca-game/src/score.rs`).
- Score rounding (final floor vs per-note accumulation) and B/C grade hit-count semantics (UNVERIFIED).
- TIMING option unit (8.33 ms per step) and sign are inferred, not measured on the real game.
- Not measured: overlapping simultaneous touches, Expert/Inferno-specific behaviour (locked for guests in the
  real game; doc 10 §10.5).
- Rating: which songs count as "new", and rounding of the per-chart product (UNVERIFIED, `rating.rs`).

## Menus

- Title: halo brightness and the ghost's colours (run-time material parameters; doc 14 §5.3).
- Wheel easing and the menu-to-menu wipe timings are UNVERIFIED; final check → play should be a zoom tunnel.
- Final-check monitor: ring, lane separators and the demo note; movie charts show a static stand-in.
- Results: Missless / Full Combo / All Marvelous / Failed stamps; ring colours per grade.
- Mode select: the `BP_Select_level` rings.
- Content not in our catalog: FOR BEGINNERS / Favorite / New Songs folders, Notes Designer line.

## Platforms

- Linux and macOS build in CI but are untested on real hardware (touch input, audio at 48 kHz, fonts).
- Audio output needs a 48 kHz device (there is no resampler).
- Android / iPad: see [GUIDE.md § Porting notes](GUIDE.md#porting-notes).
