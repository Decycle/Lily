# 07 — Controlling and recording the real game (verification harness)

Scope: how to drive the real WACCA build on this PC (segatools + Waccon-IO) from code with exact, scripted
touch input, record it at 60 fps with audio, align it with our timeline, read its judgements, and compare it frame
by frame with WACCA_slim fed the same input. Ends with a harness design (`tools/realgame`) and a first joint
experiment.

Nothing here was run against the live game. All statements about the running game are from source code, config
files, binary strings and docs, and are marked **UNVERIFIED** where they need a live test.

Confidence legend: **CONFIRMED** (read in source/config/binary on this PC or in the cited source), **UNVERIFIED**
(inference or single weak source; to be checked by the experiments in §7).

Sources
- [W1] Waccon-IO source, HEAD fetched 2026-10-12: https://github.com/samnyan/Waccon-IO —
  `waccon-io/include/waccon/shared_memory.h`, `waccon-io/src/mercuryio.c`, `waccon-io/src/waccon_state.c`,
  `waccon-io/README.md`, `IO_REFACTOR.md`, `Waccon-Server/WconProtocol.cs`, `SharedMemoryBridge.cs`,
  `WacconServer.cs`, `Waccon-Server/README.md`.
- [W2] Installed copies: `C:\Games\WACCA\Waccon\web\controller.js` (bundled controller, WCON encoder),
  `Waccon\appsettings.json`, `Waccon\overlay_config.json`, `C:\Games\WACCA\bin\waccon_io.dll` (strings).
- [S1] segatools source (develop): https://gitea.tendokyu.moe/TeamTofuShop/segatools —
  `games/mercuryhook/touch.c`, `touch.h`, `io4.c`, `mercury-dll.c`, `games/mercuryio/mercuryio.c`, `config.c`,
  `common/util/fg-detect.c`, `common/board/io4.c`.
- [S2] capnhook `iobuf_write` (used by segatools for the emulated serial buffers):
  https://github.com/decafcode/capnhook/blob/master/hook/iobuf.c (segatools pins the fork
  https://gitea.tendokyu.moe/TeamTofuShop/capnhook).
- [G1] `C:\Games\WACCA\bin\segatools.ini`, `launch.bat`, `config.json`; `C:\Users\<user>\OneDrive\Desktop\Play WACCA.bat`.
- [G2] `C:\Games\WACCA\WindowsNoEditor\Mercury\Config\Default{Engine,Game,GameUserSettings,Hardware,Input}.ini`
  (and the diffs against `C:\Games\WACCA\_original_backups\`).
- [G3] Strings in `Mercury-Win64-Shipping.exe` (via `tools/research/grep_strings.py`): `ReplayManager.cpp`,
  `UReplayData{InputDataArray,FrameNum}`, `%sReplayData/%s/%02d/`, `bReplaySave`, `RhythmGameUserOption`
  fields (`bAutoPlay`, `AutoPlayJudgeType`, `StartBarNum`, `DebugPlaySpeed`, `bDebugMetronome`,
  `JudgeRangeDisplayType`), `TITLE_HUD_STATE_DEMOPLAY`, `bAgingMode`, `PlayLog/Header.csv` and its columns,
  imports `timeBeginPeriod`/`timeEndPeriod`.
- [G4] `Content/Message/TestModeMessage.uexp` (test-menu strings), `Content/Table/DemoMusicTable` (parsed with
  `tools/research/ue4_datatable.py`), `Content/Map/*.umap` and `Content/Tools/BP_DevelopmentMenu.uasset` listing.
- [G5] Game-written files under `C:\Games\WACCA\bin\appdata\SDFE\Saved\` (`Logs\MercuryLogs\...`, `Config\...`).
- [M1] timeBeginPeriod (per-process since Windows 10 2004):
  https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod
- [M2] Windows.Graphics.Capture `Direct3D11CaptureFrame.SystemRelativeTime` ("QPC time at which the compositor
  rendered the frame"): https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.direct3d11captureframe.systemrelativetime
- [M3] DXGI Desktop Duplication `DXGI_OUTDUPL_FRAME_INFO.LastPresentTime`:
  https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_frame_info
- [M4] `IAudioCaptureClient::GetBuffer` (`pu64QPCPosition`):
  https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer ;
  loopback recording: https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording
- [M5] Process loopback (`AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS`), minimum client "Windows 10 Build 20348":
  https://learn.microsoft.com/en-us/windows/win32/api/audioclientactivationparams/ns-audioclientactivationparams-audioclient_process_loopback_params
- [M6] `InjectTouchInput`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-injecttouchinput ;
  `SendInput`: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
- [F1] FFmpeg `ddagrab`, `ssim`, `psnr`, `hstack` filters: https://ffmpeg.org/ffmpeg-filters.html ; `gdigrab`:
  https://ffmpeg.org/ffmpeg-devices.html#gdigrab
- [O1] obs-websocket v5 protocol: https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md
- [A1] ARTEMiS server (WACCA title handler): https://gitea.tendokyu.moe/Hay1tsme/artemis

---

## 0. Summary of recommendations

| Item | Recommendation |
|---|---|
| 1 Input | **Write the Waccon shared buffer directly** (`Local\WACCON_SHARED_BUFFER`, ABI 1.1) from our Rust harness, using the same sequence-lock protocol as Waccon.Server. No change to the game install, Server can keep running; precise QPC scheduling; consumption is acknowledged by the DLL. Taps held ≥ 2 game frames (34 ms) until §7 E1 measures the minimum. Fallback: WCON over TCP 52468 (same record, +0.1–1 ms, .NET jitter). |
| 2 Navigation | Manual navigation for the first experiments; then closed-loop automation: coin/test/service via shared-buffer `opbtn` bits, touches via the same buffer, screen-state detection by template matching captured frames (jackets, difficulty plates, fixed UI) — never blind timed macros. Free play (`[system] freeplay=1`) and "songs for 1-play = 1" make sessions short (user change). |
| 3 Time sync | Song clock from **game audio**: WASAPI loopback with per-packet QPC stamps, cross-correlated with our decoded BGM (expected ±1 ms). Video frames get QPC stamps from Windows.Graphics.Capture. The game's internal judge offset vs audible audio is then measured, not assumed (E2 sweep). |
| 4 Recording | Long term: in-harness **Windows.Graphics.Capture** of the game window (QPC per frame) + loopback audio, lossless frames. Bootstrap (zero code, already installed): **OBS 32.0.4** (window capture + audio, obs-websocket for start/stop). Ours: headless wgpu render at the exact song time of each captured real frame → per-frame SSIM/PSNR; side-by-side via FFmpeg `hstack` (FFmpeg already on the plan's approval list). |
| 5 Judgements | Least invasive and reliable: (a) the game's own **PlayLog CSV** (per-song MARVELOUS/GREAT/GOOD/MISS columns; offline writing UNVERIFIED), (b) result-screen template OCR (counts + FAST/LATE), (c) per-note: isolated probe notes + judgement-sprite template matching in the video. Memory reading and a local ARTEMiS server are not recommended. Optional: `bReplaySave` (per-frame input replays) if the user agrees to flip one setting. |
| 6 Harness | `tools/realgame` (crate `wacca-realgame`, prototype in this commit: cell mapping, WCON framing, shared-buffer layout + seqlock writer, tap-script format). Next: `inject`, `capture`, `sync`, `compare`, `judge` binaries; file formats in §6. |

---

## 1. Input injection

### 1.1 The input path (CONFIRMED from source/config unless marked)

```
our harness ──(a) WCON/TCP 52468──► Waccon.Server ─┐
            ──(b) direct write ───────────────────►├─► Local\WACCON_SHARED_BUFFER (seqlock, lease)
browser/overlay (WebSocket 52469) ─► Waccon.Server ┘                 │ polled by
                                                                     ▼
Mercury-Win64-Shipping.exe [injected: mercuryhook.dll → waccon_io.dll (mercuryio API 0x0100)]
  touch thread (loop: read shm + local WinTouch/mouse, OR them, call segatools callback, wait 1 ms)
  └─► mercuryhook touch_res_auto_scan(): packs 240 bools into two 36-byte "0x81" frames
        (cells 0..119 → COM3 "R" board, 120..239 → COM4 "L" board, 5 cells per byte, 24 bytes)
        appended to a 520-byte emulated UART buffer per side
  └─► game's ElizabethPlugin serial reader (COM3/COM4, 115200 baud, BufferSize 4096)
  └─► game logic (fixed 60 fps tick) → judgement
```

- Waccon is configured as segatools' `mercuryio` provider: `[mercuryio] path=waccon_io.dll` [G1]. The DLL ORs
  the shared-buffer cells with its own WinTouch/mouse cells on every callback [W1 `mercuryio.c` `collect_touch`].
- The touch thread loops `collect_touch → callback → WaitForSingleObject(stop, 1)` [W1]; the installed DLL is an
  older build (PE timestamp 2026-09-04, imports `Sleep`, lacks the refactor's `mousePollMs`/`mappingMode` keys) but
  has the same ABI-1.1 strings (`shared input accepted seq=%lu source=%lu lease=%llums`) [W2]. A 1 ms wait only
  gives ~1 ms if the process has raised the timer resolution; since Windows 10 2004 that is per-process [M1].
  The game imports `timeBeginPeriod` [G3], so ~1 kHz is likely — **UNVERIFIED**, and measurable (§1.4).
- mercuryhook writes one frame per callback into a fixed 520-byte buffer; `iobuf_write` refuses (drops) a frame
  that does not fit [S1 `touch.c`, S2]. With 36-byte frames at ~1 kHz the buffer is full after ~14 ms unless the
  game drains it continuously. If the game drains it once per 16.7 ms frame, the frames it reads are the **oldest**
  ones and up to ~14 ms stale, and newer ones are lost. **UNVERIFIED**: the game's read cadence. This is the main
  unknown latency term and the reason to measure it (E1) instead of assuming it.
- Real hardware streams a 240-sensor frame roughly every 8 ms (docs/research/04 §1, community). The game's
  `TouchDeviceSettings` has analogue thresholds (On/Off 17/12) and `bCareChattering=true`, `bCareKeyGap=true`
  [G2 DefaultHardware.ini]. The thresholds cannot apply to segatools' on/off bitmask, but a chattering filter
  could require a cell to stay on for more than one scan or frame. **UNVERIFIED**; E1 includes 5/17/34/50 ms taps.
- Foreground: segatools freezes touch input while the game is unfocused only if `[io4] foreground=1`
  (`fgdet_init` is only called then) [S1 `io4.c`, `fg-detect.c`]. Here `foreground=0` [G1], so injection works
  while the harness or OBS has focus. Waccon's own mouse source uses `GetForegroundWindow` [W2 imports], which does
  not matter for injected cells.
- Game tick: `bUseFixedFrameRate=True`, `FixedFrameRate=60`, `bUseVSync=True` [G2]. The replay system stores
  `FrameNum` plus `InputDataArray` [G3], a strong hint that game logic consumes **one sensor snapshot per frame**.
  **UNVERIFIED**, but consistent with the judgement windows being whole frames (docs/research/04 §1).

### 1.2 Shared-buffer protocol (CONFIRMED [W1])

Layout (packed little-endian, 2282 bytes): `magic "WCON" u32 @0, major u16 @4 (=1), minor u16 @6 (=1),
struct_size u32 @8 (=2282), capabilities u32 @12, input_sequence u32 @16, output_sequence u32 @20,
input @24 (262 B), LED output @286 (u32 unit_count, 480×RGBA, u64 timestamp_us), server endpoint @2218,
io endpoint @2250` (endpoint = `u32 pid, u16 major, u16 minor, u64 started_ms, u64 heartbeat_ms,
u64 input_sequence`, times = `GetTickCount64`).

Input record (262 B, identical to the WCON `InputSnapshot` payload): `opbtn u8, gamebtn u8, cells[240] u8 (0/1),
source_id u32, timestamp_us u64, lease_ms u64`.

Write protocol (Waccon.Server `PublishInput`): `seq = (seq+1)|1` (odd) → copy record → `seq = odd+1` (even).
waccon_io.dll accepts a record when the sequence is even, differs from the last one it took, did not change during
the copy, and `0 < lease_ms ≤ 60000`; it then holds the cells for `lease_ms` (cleared afterwards) and writes the
accepted sequence to `io.input_sequence` [W1 `waccon_state.c`]. So:
- every state change is one publish; a held state must be **re-published before its lease runs out**
  (re-publishing identical content still advances the sequence);
- `io.input_sequence == our sequence` is an **acknowledgement** with the DLL's own timing: the harness can measure
  write→consume latency per edge;
- `opbtn` bits 0x01 TEST, 0x02 SERVICE, 0x04 COIN and `gamebtn` 0x01/0x02 volume work through the same record
  [W1 `mercuryio.h`], so coins and test-menu navigation need no keyboard focus.

Coexistence: Waccon.Server only writes the input record when a client sends a snapshot or disconnects (it then
publishes an all-zero clear with lease 1) [W1 `WacconServer.cs`]; its heartbeat touches only the server endpoint.
So the harness can write while the Server runs, provided **no controller client is active**: close the
OverlayWebview (its page sends `ClearInput` on hide and snapshots every 100 ms while touched [W2 `controller.js`])
and do not touch the screen or click the game window (WinTouch/mouse cells are ORed in).

Cell numbering [W1 README, W2 `controller.js`]: `cell = side*120 + ring*30 + sector`; side 0 = visual right
(`x ≥ centre`), side 1 = visual left; ring 0 = inner (radius 0.6) … 3 = outer (1.0); sector 0 at the top of each
half, increasing downwards. With our lane convention (lane 0 = segment just above 3 o'clock, counter-clockwise,
docs/research/04 §2): right side `lane = (14 − s) mod 60`, left side `lane = 15 + s`; `depth = ring`.
Implemented and unit-tested in `tools/realgame/src/cells.rs`. **UNVERIFIED** that the game's lane 0 and ring
order match; this is E1's first check (players use this mapping daily, so it is at least approximately right;
a reversed ring order would invert snap directions and is easy to see in the test menu).

### 1.3 Options compared

| | (a) WCON over TCP to Waccon.Server | (b) direct shared-buffer write | (c) own `mercuryio` DLL with a timeline | (d) keyboard / WinTouch injection |
|---|---|---|---|---|
| Changes to game install | none | none | replace `[mercuryio] path` (segatools.ini) | none / none |
| Extra hops | TCP loopback + .NET async + shm | shm only | none (inside game process) | OS input stack → WM_TOUCH → Waccon mapping |
| Latency added before the DLL sees it | ~0.1–1 ms, .NET thread-pool jitter | ~0 (next 1 ms DLL loop) | 0 | several ms, variable |
| Timestamp accuracy | our QPC at send | our QPC at write + DLL ack sequence | QPC inside the game process | poor |
| Determinism | lease forced to server's 500 ms; fine | full control of lease and edges | best possible (could align to the callback) | geometric mapping error, 10-contact practical limit, needs focus |
| 240 independent cells | yes | yes | yes | (d-key) waccon has no per-cell keyboard map; (d-touch) only through screen geometry |
| Effort | small | small (prototype here) | medium (C/Rust DLL, chaining to waccon for normal play) | small, but useless for exact cells |

Recommendation: **(b)**. It is the same record the Server writes, adds nothing between us and the DLL, gives a
per-edge acknowledgement, and needs no change under `C:\Games\WACCA`. The remaining latency and jitter (UART
buffer, game read cadence, 16.7 ms frame quantisation) are identical for (a)–(c) because they live downstream of
the DLL, so (c) only helps if E1 shows the DLL loop itself is slow or jittery (> 2 ms). (a) is the fallback
if a direct mapping is ever refused (e.g. an ABI change; the harness checks magic/version/size first, as the DLL does).

Scheduling in the injector: a dedicated thread with `timeBeginPeriod(1)`, sleeping to ~1 ms before each edge, then
spinning on `QueryPerformanceCounter`; publish each edge; renew the lease every 50 ms with `lease_ms = 100` (short,
so a crashed harness releases all cells within 0.1 s); log `{qpc_write, seq, qpc_ack}`.

### 1.4 How the game samples touch (summary)

CONFIRMED: game logic ticks at a fixed 60 fps with vsync; the DLL loop is ~1 ms if timer resolution is raised;
mercuryhook writes a full 240-cell frame per callback into a 14-frame buffer. UNVERIFIED and measured by E1:
the game's serial read cadence, any debounce (`bCareChattering`), and end-to-end tap → visible-effect latency. Until
then: hold every scripted tap **≥ 34 ms (2 frames)** and expect each edge to land in one of two adjacent game
frames. For exact comparisons we quantise our own engine's input the same way (§5.3) instead of hoping the game is
continuous-time.

---

## 2. Navigation automation

### 2.1 Inputs available without touching the game install

- Coin / test / service: shared-buffer `opbtn` (§1.2), or keyboard F3/F1/F2 (segatools `[io4]`, read with
  `GetAsyncKeyState`, works unfocused because `foreground=0`) [G1, S1].
- Card: hold Enter (`[aime] scan=0x0D`) [G1]. `DEVICE\aime.txt` does not exist, so what card (if any) is emulated
  is UNVERIFIED; the game is in `OfflineMode=True` [G2] and its only network log is `ApiUserStatusLogout … Timeout`
  [G5], so **guest play** is the reproducible path (the PlayLog writer has a `GUEST` string [G3]). Enter is also the
  UE action `CursorOK` (arrow keys, Enter, Backspace are mapped in DefaultInput.ini) — UNVERIFIED whether any
  shipping screen listens to them; do not rely on keyboard menu navigation.
- Touch: the shared buffer (§1).

### 2.2 Making a session reproducible (user-side settings, suggestions only)

- Free play: `segatools.ini [system] freeplay=1` removes coins. "SONGS FOR 1-PLAY" (`CanPlayMusicNumber` 1–3,
  test menu) set to 1 makes every credit one song → result → title. Both are optional and reversible.
- Guest play uses default options (speed, mirror, judge offset, effects). Record which options the game shows and
  configure WACCA_slim identically; options navigation can be added once the defaults are known.
- `bValidCulture_Offline` exists per song in MusicParameterTable: in offline mode only some songs may be listed —
  pick experiment songs from those (UNVERIFIED filter semantics).

### 2.3 State detection (closed loop, not blind timing)

- **Screen templates** (recommended): capture the window (§4) and match small regions against references: title
  logo, mode select, song-select jacket (we have all 395 jackets as BGRA from the assets), difficulty plate,
  "ready" overlay, result layout. Navigation becomes "act → wait until template matches (timeout) → act". Song
  choice: scroll one step, compare the centred jacket with the target jacket, repeat.
- **LED output** in the shared buffer (480 RGBA units, sequence-locked, with timestamp): the game drives the ring
  LEDs per scene; a cheap secondary state/tempo signal (UNVERIFIED patterns; record them during E1).
- **Files** [G5]: `MercuryLogs\AppendingLog\CreditLog.csv` gets a line per credit, `PlayLog\` and `DailyPlayLog\`
  are written by `MercuryLog.cpp` (empty so far), `ApiErrorLog.csv` for network errors. `amdaemon.exe.log`
  holds only daemon exceptions. No UE log file is written (shipping build). Window title is constant ("Mercury").
- **Teach by demonstration**: the harness can sample the shared buffer at 1 kHz while the user navigates with the
  Waccon web controller, and save that as a macro of touches; the macro is then replayed step by step, gated by the
  screen templates. WinTouch/mouse touches are *not* visible in the buffer (the DLL ORs them locally), so teaching
  must use the web controller.

The exact touch zones of each menu (WACCA menus are operated on the ring) are UNVERIFIED; learn them from the
first manual sessions (video + taught macro) rather than guessing.

### 2.4 Built-in autoplay / demo / reference modes

| Feature | Evidence | Usable? |
|---|---|---|
| Attract **demo play** | `TITLE_HUD_STATE_DEMOPLAY`, `Map/Demo.umap`, `DemoMusicTable` [G3, G4]: 5 rows, all `Difficulty 0` (Normal), `PlayTime 30.0`: 3011 *Ouvertüre*, 32 *Let you DIVE!*, 3009 *Datamania (tpz Overheat Remix)*, 2012 *with U*, 3016 *Meta-Mysteria* | Yes, as a **no-input visual reference** (notes, playfield, BGA) with zero setup: record the attract loop. Whether it shows notes/autoplay hits and which 30 s window it plays is UNVERIFIED — check in the first recording. |
| Debug **autoplay** | `RhythmGameUserOption.bAutoPlay`, `AutoPlayJudgeType`, `StartBarNum`, `DebugPlaySpeed`, `bDebugMetronome`, `JudgeRangeDisplayType`, `EDebugItemType::DEBUG_ITEM_TYPE_AUTO_PLAY(_JUDGE)` [G3]; `Map/DevelopmentMenu.umap`, `Map/ScoreEdit.umap`, `Tools/BP_DevelopmentMenu` are cooked into the build [G4] | Potentially ideal (autoplay any chart, jump to a bar, metronome), but only reachable through the development menu (e.g. starting the game with a map URL argument in a *copied* launcher) or memory poking. UNVERIFIED that it works in this build; may break the boot sequence. Optional experiment with the user's OK. |
| Test-menu "AUTO PLAY" / "AUTO REWIND" | `TestModeMessage` strings, next to the LED tests [G4] | Probably LED/sound test playback; UNVERIFIED. Look at it during E1. |
| Aging mode | `bAgingMode`, `AgingElapsedSeconds`, Elizabeth `bAutoAging=false`, `AutoAgingMode=random` [G2, G3] | Random burn-in input; not a chart reference. |
| **Replay save** | `bReplaySave=false` in DefaultGameUserSettings.ini, `ReplayManager.cpp`, `UReplayData{InputDataArray, FrameNum}`, path `%sReplayData/%s/%02d/` [G2, G3] | If the user sets `bReplaySave=True` (in `appdata\SDFE\Saved\Config\WindowsNoEditor\GameUserSettings.ini`, section `[/Script/Mercury.MercuryGameUserSettings]`), the game may write per-frame input replays — **the exact frame the game assigned each injected edge to**. Format (UE SaveGame?) and whether it is active in shipping are UNVERIFIED. High value, low risk, user decision. |

---

## 3. Time synchronisation

Clocks involved: our QPC (inject log, capture stamps), the game's audio output, the game's frame tick, and the
recording's media time. Everything is mapped to **song time** (chart time, seconds, the same `time` as
`InputFrame`).

1. **Audio → song time (primary).** WASAPI loopback on the default render device; every packet carries the QPC
   time of its first frame [M4]. Cross-correlate the captured mix with the BGM decoded by `wacca-audio` (HCA,
   48 kHz, the same cue the chart's `#MUSIC_FILE_PATH` names) over a 1–2 s window after the first non-silent part.
   Result: `qpc_at(song_t = 0)` with sub-millisecond resolution; a second window near the end gives clock drift
   (ppm). Notes: per-process loopback needs build 20348 [M5] and this PC is 19045, so capture the whole device and
   mute other apps; the game outputs 6-channel shared-mode WASAPI (`WASAPI_NumChannels=6`) [G2], loopback gives the
   device mix. Hit sounds are mixed in but do not hurt correlation. Expected accuracy ±1 ms (UNVERIFIED until E1;
   the main error is the endpoint's constant render latency, which cancels out because ours is measured the same way).
2. **Live use.** The injector needs song time *during* the song: run the correlator in real time on the first
   ~1 s of BGM, then schedule all taps in QPC. Scripts must keep the first tap ≥ 2 s after the BGM starts.
3. **Video → song time.** WGC frames carry `SystemRelativeTime` = QPC when the compositor rendered the frame [M2],
   so each captured frame gets `song_t = (qpc − qpc0)/f`. That includes a constant present→compose delay (0–1
   frame). Refine it by **visual phase lock**: render ours at `song_t + δ` for δ ∈ ±20 ms (1 ms steps) and pick δ
   maximising SSIM over the moving notes; with notes moving every frame this pins the game's render time per frame
   to ~1 ms and measures its visual offset vs audio. With OBS recordings the same is done using the recorded audio
   track instead of QPC (OBS A/V sync is within a frame; phase lock removes the rest).
4. **Judge clock vs audible audio.** Unknown by design; measured in E2 by sweeping tap offsets against real notes
   and finding the Marvelous/Great boundaries (±3 / ±5 frames, docs/research/04 §4.1): the boundary midpoint gives the
   game's effective input offset to within one frame, the boundary sharpness tells whether judging is frame-quantised.
5. **Fallback markers.** First-note detection (the first note's spawn on screen) and touch-effect flashes from an
   injected tap give coarse (±1 frame) cross-checks.

---

## 4. Recording

| Method | Frame timing | Audio | Effort here | Verdict |
|---|---|---|---|---|
| Windows.Graphics.Capture (window) | QPC per composed frame [M2]; only frames that changed | separate WASAPI loopback with QPC [M4] | Rust code (`windows` crate) | **Recommended long-term**: exact stamps, no extra software, works while the game is unfocused (not while minimised) |
| DXGI Desktop Duplication | `LastPresentTime` QPC [M3] | separate | Rust code; whole output, crop the window | Fine alternative; needs the window unobstructed |
| FFmpeg `ddagrab`/`gdigrab` + WASAPI loopback [F1] | gdigrab: GDI polling, drops/duplicates; ddagrab better | via dshow/loopback device | FFmpeg not installed (needs approval) | Good for previews, weaker stamps |
| **OBS 32.0.4** (installed: `obs64.exe`, `win-capture`, `win-wasapi`, `obs-websocket`; websocket config exists) [O1] | 60 fps CFR output; duplicates/drops are hidden | application or desktop audio in the same file | none: a scene + websocket `StartRecord`/`StopRecord` | **Bootstrap for E1/E2**; align by audio (§3.3) |

Settings for the game: keep the current 675×1200 window (`-ResX=675 -ResY=1200` [G1]); record lossless or near
lossless (OBS: MKV, x264 CRF 0 or "Lossless" preset; WGC harness: QOI/PNG per frame plus `frames.csv`).
675×1200×4 B × 60 fps ≈ 194 MB/s raw, so compress on worker threads or keep clips short (30 s ≈ 1–2 GB as QOI).

### 4.1 Our side: deterministic offscreen rendering

Requirement for `wacca-render` (when it grows a real renderer): a headless path
`render_frame(&Scene, song_t, size) -> RGBA8` (wgpu texture → `copy_texture_to_buffer` → map) whose output is a pure
function of chart, options, input-derived judge events up to `song_t`, and `song_t` (no wall clock, no frame-delta
accumulation; effect animations keyed to their trigger time). Two modes:
- **Matched**: render at the exact `song_t` of every captured real frame (after phase lock) → 1:1 pairs even when
  the game drops or repeats a frame.
- **Fixed**: `song_t = t0 + n/60` for clean side-by-side videos.
Same viewport geometry as the game: 675×1200 portrait; the game's own split is in DefaultGameUserSettings
(`ViewportTopRate2D=0`, `ViewportBottomRate2D=0.94`, `ViewportTopRate3D=0.190625`, `ViewportBottomRate3D=0.5625`)
[G2] — useful for docs/research/06 layout work.

### 4.2 Comparison outputs

- Per frame: SSIM (8×8 windows, luma) and PSNR over the whole frame and over masks (playfield ring, notes band,
  HUD); the masks keep the BGA video from dominating. Implement SSIM/PSNR in Rust (~100 lines) so no new
  dependency is needed; FFmpeg's `ssim`/`psnr` filters [F1] are a cross-check once FFmpeg is approved.
- Side by side: `ffmpeg -i real.mkv -i ours_%05d.png -filter_complex hstack` [F1] (needs FFmpeg), plus a diff
  panel (|real − ours| ×4). Without FFmpeg: write PNG strips of chosen frames and a CSV/HTML report.

---

## 5. Judgement comparison

### 5.1 Options

| Source | Granularity | Invasiveness | Reliability | Verdict |
|---|---|---|---|---|
| **PlayLog CSV** (`MercuryLogs\PlayLog\…_%Y%m%d%H%M%S.csv`, `Header.csv`; columns include `No.`, `ID`, `Lv.`, `MARVELOUS`, `GREAT`, `GOOD`, `MISS` per song, `GUEST`) [G3, G5] | per song counts | none (game writes it) | high, if written offline (UNVERIFIED: folder empty so far) | **Use first** |
| **Result screen** template OCR (digits from extracted UI textures) | per song counts + FAST/LATE, score | none | high (fixed layout) | **Use**, also confirms PlayLog |
| **Per-note sprites in video** (MARVELOUS/GREAT/GOOD/MISS and fast/late markers near the note, templates from UI textures) | per note | none | good if probe notes are isolated (≥ 300 ms apart, distinct lanes) | **Use for E2+** |
| `bReplaySave` replay files | per frame input (maybe judgements) | one setting changed by the user | unknown format | optional bonus |
| Memory reading (score/judge structs) | per note | attach/read process memory; needs RE (Ghidra, not installed) | brittle | not recommended |
| Local ARTEMiS server [A1] | per song (the music-update request carries judgement counts; UNVERIFIED field set) | install a Python server, switch off `OfflineMode`, DNS config | medium | not recommended (more invasive than PlayLog/OCR for the same data) |
| LED output | coarse feedback | none | unknown mapping | latency probe only |

### 5.2 Test charts

We cannot add charts to the game (no changes to the install), so probes use existing charts: pick a slow Normal chart
whose notes are isolated plain touches; script one tap per note at `note_time + δ_i`. For E2 choose δ from a
pattern like −83…+83 ms in 8.3 ms steps repeated across notes, so judgement transitions bracket the window edges.

### 5.3 Feeding our engine the same input

Convert the inject log (`qpc_write` per edge, acknowledged) to song time with §3, then optionally quantise to the
game's frame grid (frame phase from the capture stamps/phase lock, plus the latency measured in E1), and feed the
resulting `InputFrame` list to `wacca-game::judge`. Compare per note (sprite detection) and per song (PlayLog/OCR).
Differences that disappear with quantisation enabled point at frame-based judging in the original.

---

## 6. Harness design: `tools/realgame`

Crate `wacca-realgame` (workspace member). Library parts are pure and unit-tested; binaries do the IO.

```
              taps.json ─┐                          ┌─ wacca-audio (decode BGM)
                         ▼                          ▼
 realgame inject ── shm writer (QPC, ack) ──► game ──► audio ──► realgame sync (loopback + xcorr)
      │  inject.jsonl                         │                      │ align.json (qpc0, drift)
      │                                       └─ window ─► realgame capture (WGC) ─► frames/, frames.csv
      ▼                                                                │
 realgame judge ◄── PlayLog CSV, result frames, per-note sprites ◄────┤
      │ judge.csv (real vs wacca-game on the same InputFrames)         ▼
      └──────────────────────────────────────────────► realgame compare (headless wacca-render, SSIM/PSNR)
                                                            compare.csv, side-by-side PNG/MP4
```

Implemented in this commit (`tools/realgame/src`):
- `cells.rs` — lane/depth ↔ Waccon cell index (§1.2), `SensorState` ↔ 240 bytes.
- `wcon.rs` — WCON v1 frame encode/decode, 262-byte input record (matches `controller.js` byte for byte).
- `shm.rs` — shared-buffer layout, header check, seqlock `publish_input`, DLL ack (`io.input_sequence`),
  heartbeat, LED snapshot read; `SharedBuffer::open_existing` (Win32 `OpenFileMappingW`, never creates the
  mapping). Tests run against an in-memory fake that mirrors `waccon_shm_read_input`; nothing touches the live
  buffer.
- `timeline.rs` — tap-script format (below) → `InputFrame` edge list shared by the injector and `wacca-game`.
- `scripts/e1-lanes.taps.json` — the E1 script.

To build next (in order): `inject` (scheduler thread, lease renewal, ack logging, `--buttons coin|test|service`,
`--teach` recorder), `capture` (WGC + loopback, QPC stamps), `sync` (real-time BGM correlation), `judge`, `compare`.

File formats (all in one run directory `runs/<date>-<name>/`, gitignored, never committed):
- `*.taps.json` — `{"taps":[{"at_ms","dur_ms","lane","width","depths"}]}`; `at_ms` in song time (or run time
  when no song is playing, e.g. test menu); `depths` bit 0 = innermost ring.
- `run.json` — game version, window size, song id, difficulty, options shown in game, script name, harness
  version, measured offsets.
- `inject.jsonl` — one line per publish: `{"seq","qpc_write","qpc_ack","t_script_ms","cells":"<hex 240 bits>","kind":"edge|renew|clear"}`.
- `frames.csv` — `idx,qpc,file` (QPC in 100 ns units as WGC reports); frames as `frames/%06d.qoi|png`.
- `audio.wav` + `audio_qpc.csv` (`sample_index,qpc` per packet).
- `align.json` — `{"qpc_song0","drift_ppm","video_offset_ms","frame_phase_ms","input_latency_ms"}`.
- `compare.csv` — `idx,song_t,ssim,psnr,ssim_ring,ssim_notes`; `judge.csv` — `note,song_t,real,ours,real_fl,ours_fl`.

Safety rules for the injector: refuse to write unless the header matches ABI 1.1 and the DLL heartbeat is fresh
(< 3 s); short leases (100 ms) so a crash releases input; an explicit `--live` flag; clear input on exit.

---

## 7. First joint experiments

### E1 — mapping, latency, minimum tap (no song needed)

Needs from the user:
1. OK to run the harness's `inject` against the live game (it writes only to `Local\WACCON_SHARED_BUFFER`).
2. Start the game as usual (`Play WACCA.bat`), then **close OverlayWebview** (tray) and do not touch the screen.
3. OBS: one scene with Window Capture of "Mercury" (capture method Windows 10 (1903+)) and desktop audio,
   60 fps; either press Record manually or give the harness the obs-websocket password (it stays local).
4. Navigate with F1 into the test menu → "TEST OF INPUT MACHINE" (or whichever screen shows the touch cells;
   to be found together).

Steps:
1. Harness checks header/heartbeat, then plays `scripts/e1-lanes.taps.json` (run time, not song time): lanes 0–9
   (all rings, 200 ms each, 0.5 s apart), lane 0 rings 0→3 one at a time, lanes 15/30/45, then 17/34/50/5 ms taps.
2. Look at the recording: which cells light up for lane 0 (expect just above 3 o'clock), direction of lanes 1–9
   (expect counter-clockwise), ring order (expect inner → outer), whether the 5 ms / 17 ms taps register.
3. Latency: ack time − write time (DLL loop), and first frame showing the cell − write time (end-to-end) from the
   OBS/WGC timestamps; repeat the run 3× for jitter.
4. Repeat steps 1–3 during attract mode or a song to check that the in-game touch effect agrees.

Outcome: confirmed (or corrected) `cells.rs`, minimum tap duration, latency distribution → `align.json` defaults.

### E2 — clock alignment and judgement offset (needs `sync`)

Guest play of one slow Normal chart; the script hits each isolated touch note at `note_time + δ` with the δ
pattern of §5.2; record; read PlayLog/results/sprites; fit the Marvelous window edges → the game's effective input
offset and whether judging is frame-quantised. Then run the same `InputFrame`s through `wacca-game` and diff.

### E0 (free, any time) — attract demo

Record ~3 minutes of the title/attract loop with OBS. This shows whether demo play renders notes, which 30 s
window it uses, and gives the first visual references for M3/M6 with no input and no harness.

---

## 8. Open questions (UNVERIFIED items)

1. Game's serial read cadence and the 520-byte buffer behaviour (stale/dropped frames) → E1 latency histogram.
2. Debounce (`bCareChattering`) / minimum tap length → E1 short taps.
3. Lane 0 position, lane direction, ring order of Waccon cells vs game lanes → E1.
4. Frame-quantised vs continuous judging; judge clock vs audible audio → E2.
5. PlayLog CSV written in offline guest play; its exact columns → after the first play.
6. Demo play content and timing → E0.
7. `bReplaySave` output location/format; development-menu autoplay reachability → optional, user decision.
8. Menu touch zones and guest flow → first manual session + teach mode.
9. `timeBeginPeriod` effective inside the game (DLL loop ≈ 1 ms) → E1 ack latency.
