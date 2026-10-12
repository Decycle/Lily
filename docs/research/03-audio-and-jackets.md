# 03 – Audio (ACB/AWB/HCA/ADX) and jackets (UE4 Texture2D)

Research date 2026-10-12. Everything below was checked against the files in `assets/` unless it is
marked **UNVERIFIED**. All prototypes use only the Python stdlib and open the game files read-only.
Run them with `python -I` and pass paths as arguments:

| Script (`tools/research/`) | Purpose |
|---|---|
| `cri_utf.py` | Library: CRI `@UTF` table reader and AFS2 header parser |
| `utf_dump.py <file> [rows]` | Dumps the schema and first rows of an ACB, ACF or UTF file, including nested tables |
| `acb_map.py <acb.uexp> [awb_dir] [--charts MusicData] [--csv out] [--quiet]` | Resolves every cue → waveform → AWB file, entry and byte range, then checks the HCA/ADX signature |
| `jacket_extract.py survey <JACKET dir>` / `info <uexp>` / `png <uexp> <out.png>` | Parses the cooked Texture2D and writes a PNG (zlib + hand-written PNG) |

Generated outputs are in `docs/research/samples/`:
- `bgm_cue_map.csv`: 832 rows, one per cue × waveform for MER_BGM.
- `se_cue_map.csv`: the same for MER_SE.
- `uT_J_S00_003.png`: an extracted jacket ("Rainbow Dream", 256×256). It was checked visually.

---

## 1. Chart → audio bytes

### 1.1 Files involved

| File | What it is |
|---|---|
| `Sound/Bgm/MER_BGM.uasset/.uexp` | UE4 `SoundAtomCueSheet` named `MER_BGM`. The `.uexp` holds the whole **ACB** (`@UTF`, "ACB Format/PC Ver.1.30.00") as bulk data. Use the first `@UTF` in the file and its size field. |
| `Sound/Bgm/MER_BGM.awb` + `MER_BGM_V3_01..07.awb` | Stream AWBs (AFS2, version 2, 4-byte offsets, 2-byte IDs, align 32, subkey 0). |
| `Sound/Bgm/MER_BGM_S00_003.uasset/.uexp` etc. | UE4 `SoundAtomCue` objects. Each holds only `CueSheet` (an import of `/Game/Sound/Bgm/MER_BGM`) and `CueName` (an FString, for example `MER_BGM_S00_003`). The `.uexp` always contains the same name as the file, so **a Rust loader can skip these files** and look the chart's `#MUSIC_FILE_PATH` up directly in the ACB CueName table. |
| `Sound/Bgm/bgmpool.*` | A tiny second ACB with one cue (`Cue_0`, 90 s mono). The `bgmpool.awb` has 1 entry. 19 legacy `MER_BGM_S01_xxx` SoundAtomCue assets point their CueSheet at `bgmpool`, but the cue names they request exist only in MER_BGM (or nowhere). Ignore bgmpool. |
| `Sound/Bgm/blank`, `bpm100/150/200` | Assets for the silent and metronome cues (also in the MER_BGM ACB). |

### 1.2 Resolution chain (verified)

```
chart .mer "#MUSIC_FILE_PATH MER_BGM_S00_003"
  -> ACB Header.CueNameTable  : row where CueName == name  -> CueIndex (u16)
  -> ACB Header.CueTable[CueIndex] : ReferenceType (const 3 = Sequence), ReferenceIndex, Length (ms; 0xFFFFFFFF = looping)
  -> SequenceTable[ReferenceIndex] : NumTracks, TrackIndex (data: u16 BE array)
  -> TrackTable[t] : EventIndex, CommandIndex
       TrackEventTable[EventIndex].Command  = TLV list (u16 code, u8 len, payload)
          code 0x07D0 "noteOn": payload u16 refType (2 = Synth), u16 refIndex
       TrackCommandTable[CommandIndex].Command: code 0x006F len 4 = bus send (u16 StringValueTable idx, u16 level/10000)
  -> SynthTable[i].ReferenceItems : (u16 type, u16 index)* ; type 1 = Waveform
  -> WaveformTable[w] : Streaming (1 = stream, 0 = memory), StreamAwbPortNo, StreamAwbId,
                        MemoryAwbId, EncodeType (2 = HCA, 0 = ADX), NumChannels, SamplingRate, NumSamples,
                        LoopFlag, ExtensionData (-> WaveformExtensionDataTable LoopStart/LoopEnd, or 0xFFFF)
  -> Header.StreamAwbHash[StreamAwbPortNo].Name  (+ ".awb")    e.g. port 0 = MER_BGM, 1..7 = MER_BGM_V3_01..07
  -> AFS2 header of that .awb: find StreamAwbId in the ID table -> entry index i
     start = align_up(offset[i], 32), end = offset[i+1]   (offsets are absolute in the .awb, LE)
  -> bytes [start, end) are one complete HCA file ("HCA\0", v2.0)
```

Generic ReferenceType values (from vgmstream `acb.c`): 1 = Waveform, 2 = Synth, 3 = Sequence,
8 = BlockSequence. WACCA only uses 3. The embedded `StreamAwbAfs2Header` table holds a copy of every
AWB's AFS2 header. It matched the on-disk headers for all 8 AWBs, so the AWBs could be indexed
without reading them at all. `StreamAwbHash.Hash` is an MD5-like 16-byte value; it was not checked.

### 1.3 Two tracks per song: speaker and headphone mixes

Every song cue is a Sequence with **2 tracks**. Each track plays a different waveform:
- Track A sends to bus `BGM_SPEAKER` at 1.0 and `BGM_HEADPHONE` at 0.
- Track B does the reverse.

The two HCA streams are different encodes. For S00_003 they are the same size, but the bytes and
MD5 differ. In S00_004 even the sample counts differ (6214911 vs 6220800). Bus names come from
`StringValueTable` (`MasterOut, SPEAKER, HEADPHONE, SE, VOICE, BGM_SPEAKER, BGM_HEADPHONE, JINGLE`).
**Recommendation:** for desktop play, use the track whose send is `BGM_SPEAKER`. The AFS2 IDs
usually follow the pattern headphone = n, speaker = n+1, but resolve through the tables rather than
relying on that.

Sequence commands contain `0x0041 len 4` (value 1, probably the cue category "BGM" from
AcfReference `Type 3 Name BGM Id 1`) and `0x0057 len 2` with values 70–240. The second is probably
a per-cue volume in percent. **UNVERIFIED**: it could be applied as gain = value/100. Compare against
the in-game loudness.

### 1.4 Counts (output of `acb_map.py`)

- MER_BGM ACB: 417 cues and cue names, 417 sequences, 832 tracks/synths, 824 waveforms.
- All 8 stream AWBs are present and their headers match:
  - MER_BGM: 698 entries
  - V3_01: 12
  - V3_02: 16
  - V3_03: 6
  - V3_04: 20
  - V3_05: 16
  - V3_06: 8
  - V3_07: 48
- Every AWB entry is referenced (0 orphans). All 832 waveform refs start with `HCA\0` (none masked).
  - 820 are stereo 48 kHz.
  - 12 are mono 48 kHz (bpm and metronome cues).
- Cues per AWB:
  - MER_BGM: 353
  - V3_01: 6
  - V3_02: 8
  - V3_03: 3
  - V3_04: 10
  - V3_05: 8
  - V3_06: 4
  - V3_07: 24
- Unresolved cues: only `blank` (cue 0, a sequence with 0 tracks, meaning intentional silence).
- Charts: 403 distinct `#MUSIC_FILE_PATH` values. 397 resolve.
  - **6 do not exist in the ACB**: `MER_BGM_S01_007, _025, _028, _053, _096, _277`.
  - Each of these belongs to a chart folder that is **not in MusicParameterTable**, so these are removed songs. Treat them as "no audio" and hide them.
- Each of the 395 MusicParameterTable songs resolves to an existing cue.
  - Watch out: in 3 songs **the Inferno chart (`_03.mer`) uses a different cue** from the other difficulties:
    - S00-032 → `S00_033`
    - S02-021 → `S02_022`
    - S03-011 → `S03_070`
  - So resolve audio **per chart file**, not per song.
- The tutorial chart `S99-001_0x.mer` is **UTF-16LE with BOM** (all others are UTF-8) and uses `MER_BGM_SYS_004`.
- 20 cues are not used by any chart:
  - `MER_BGM_SYS_*` (menu/system BGM)
  - `TEST_Lch/Rch`
  - `bpm100/150/200`
  - `blank`
  - `S01_234`
- Cue `Length` is in ms and matches NumSamples/48000 (S00_003: 116507 ms vs 5592383 samples).

### 1.5 Sound effects (`Sound/Se`)

- `MER_SE.uexp` (14 MB) is a self-contained ACB with a **memory AWB** in the header column `AwbFile`. All waveforms have `Streaming = 0`, and `MemoryAwbId` is the AFS2 ID inside that blob. Offsets are relative to the blob.
  - 381 cues, 393 waveforms; everything resolves except `blank`.
- Every other `Se/*.uasset/.uexp` (e.g. `MER_SE_SYS_001`, `0001-02-00`) is just a SoundAtomCue (CueSheet `MER_SE` + CueName). **There are no raw HCA/ADX files outside the ACB.**
- Codecs inside MER_SE:
  - **HCA**: 373 waveforms (109 mono 48 k, 274 stereo 48 k, 3 stereo 44.1 k, 1 mono 24 k).
  - **ADX**: 20 waveforms (`EncodeType 0`, header `80 00 00 24`, encoding type 3 = standard ADX, block 18, version 4, flags 0 = not encrypted, 44.1 kHz mono or stereo, one at 48 kHz). These are exactly the 20 `00xx-02-xx` and `9999-02-xx` cues (1–3.6 s each; probably navigator/character voice lines – **UNVERIFIED**).
- So an SE player needs an **ADX decoder too** unless SEs are pre-converted. ADX is a trivial 4-bit ADPCM, about 100 lines.
- `Sound/MerSoundDATA.acf` is the global ACF (buses, categories). It is not needed to play audio. Apply bus volumes yourself.

### 1.6 Rust loading recipe

1. Read `MER_BGM.uexp`, find `@UTF`, parse the tables. A tiny parser is enough; the `criware-utf` crate is for CPK and is not needed.
2. Build `HashMap<cue_name, Vec<(bus, awb_name, awb_id)>>`.
3. Open each `.awb` lazily. Read the 16-byte AFS2 header plus the ID and offset tables (or use the ACB's embedded copy) and `mmap` or seek to `[start, end)`.
4. Feed the HCA bytes to a decoder (section 2). The 2.8 GB `MER_BGM.awb` must never be read whole; per-song slices are 1–9 MB.

---

## 2. HCA

### 2.1 Format summary

Everything is big-endian. Chunk IDs may have bit 7 set on each byte ("masked"), so compare with `& 0x7F`.
The sources are vgmstream `clhca.c` (layout) and FFmpeg `hcadec.c`. The fields marked *seen* were read
from WACCA files by the survey in this research.

| Chunk | Layout | WACCA values (seen) |
|---|---|---|
| base | `"HCA\0"`, u16 version (0x0101..0x0103, 0x0200, 0x0300), u16 header_size (= offset of first frame) | v0x0200, header 0x60 |
| `fmt\0` | u8 channels, u24 sample_rate, u32 frame_count, u16 encoder_delay, u16 end_padding. Samples = frames×1024 − delay − padding | delay 128; padding varies; e.g. 5462 frames → 5592383 samples, matches ACB NumSamples |
| `comp` (v2+) | u16 frame_size, u8 min_resolution, max_resolution, track_count, channel_config, total_band_count, base_band_count, stereo_band_count, bands_per_hfr_group, u16 reserved | frame 682 (stereo, about 256 kbps), `01 0f 01 00 80 80 00 00` |
| `dec\0` (v1.x) | u16 frame_size, u8 min_res, max_res, total_bands−1, base_bands−1, u8 (track_count:4, channel_config:4), u8 stereo_type | – |
| `vbr\0` | u16 max_frame_size, u16 noise_level | absent |
| `ath\0` | u16 ath_type (if absent: 1 for v<2.0, 0 for v≥2.0) | absent |
| `loop` | u32 loop_start_frame, u32 loop_end_frame, u16 loop_start_delay, u16 loop_end_padding | 22 BGM (SYS/TEST only) and 74 SE |
| `ciph` | u16 type: 0 = none, 1 = fixed table, 56 = keyed (u64 key, AWB subkey) | **0 everywhere** (BGM and SE) |
| `rva\0` | f32 volume | absent |
| `comm` | u8 len + text | absent |
| `pad\0` | Filler up to header_size. **The last 2 bytes of the header are a CRC-16.** | – |

- **Frames:** `frame_count` blocks of exactly `frame_size` bytes. Each block:
  - Starts with sync `0xFFFF`.
  - Then a bit-packed payload: 9-bit noise level and 7-bit evaluation boundary, then per-channel scale factors (delta coded), intensity/HFR data, and quantized spectra.
  - Ends with a CRC-16.
- **Decoding:** each frame gives 8 subframes × 128 samples per channel = 1024 samples, through dequantization, joint-stereo and HFR reconstruction, and an IMDCT (DCT-IV, 256-sample window, overlap-add).
- **Frame state:** frames depend only on the previous frame's overlap buffer. To seek, decode from frame `floor((t·rate + delay)/1024)` and discard one priming frame (vgmstream does this).
- **CRC-16:** polynomial 0x8005, init 0, not reflected. A CRC over the whole block (or the whole header), including the stored CRC, must equal 0. **Verified** on the S00_003 header and frames 0–2.
- **Cipher:** byte substitution over each block, before bit reading. Not needed for WACCA (type 0).
- **Encoder delay:** drop the first `encoder_delay` (128) decoded samples so that sample 0 matches ACB NumSamples and Length. Whether the game's chart `#OFFSET` assumes this trim is **UNVERIFIED** (CRI's runtime does trim it).

### 2.2 Rust options

| Option | Notes | License |
|---|---|---|
| **`cridecoder`** crate ([crates.io](https://crates.io/crates/cridecoder), [GitHub](https://github.com/seiunx-dev/cridecoder), [docs](https://docs.rs/cridecoder/latest/cridecoder/hca/struct.HcaDecoder.html)) | Pure Rust, no C. `HcaDecoder::from_reader(R: Read+Seek)`, `decode_frame() -> (&[f32], n)`, `decode_frame_i16`, `seek(sample)`, `info()`, key support. It also does ACB/AWB extraction and USM. Based on vgmstream and PyCriCodecs. Young project: v0.1.0 March 2026, v0.3.7 2026-10-08, about 2k downloads. Dependencies include serde, serde_json, encoding_rs and others. ADX support is not mentioned. | MIT |
| Port vgmstream **clHCA** ([clhca.c](https://github.com/vgmstream/vgmstream/blob/master/src/coding/libs/clhca.c)) | About 1835 lines of C including tables. Self-contained: header parse, CRC, cipher tables, ATH, bit reader, IMDCT. A clean Rust port would be about 1,000–1,300 lines, roughly 2–4 days including verification against vgmstream output. History: Nyagamon HCADecoder → kode54 → bnnm (also used VGAudio as reference). | vgmstream ISC-style permissive ([COPYING](https://github.com/vgmstream/vgmstream/blob/master/COPYING)) |
| Port FFmpeg **hcadec** ([hcadec.c](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/hcadec.c) 638 lines + [hca_data.h](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/hca_data.h) 176 lines) | Compact, but relies on FFmpeg's bit reader and its `av_tx` MDCT. | **LGPL-2.1+** (a port is a derivative; avoid for an MIT/permissive codebase) |
| VGAudio ([GitHub](https://github.com/Thealexbarney/VGAudio)) | Readable C# HCA and ADX implementation. A good second reference. | MIT |
| FFI to vgmstream / libcgss | Pulls a C build into the project. | ISC / MIT |

**Recommendation:**
1. Put a small `trait HcaSource` in front of the decoder and start with **`cridecoder::hca::HcaDecoder`** (MIT, pure Rust, frame API plus seek). Wrap each AWB slice in a `Read + Seek` adapter.
2. Validate the output once against vgmstream (`vgmstream-cli` on an extracted `.hca`). Do not install it here; do it when convenient.
3. If the crate turns out to be unmaintained or buggy, port clHCA (permissive license, self-contained) behind the same trait.
4. Do not port FFmpeg (LGPL).
5. For the 20 ADX SEs, write a 100-line ADX decoder:
   - The header gives the coefficients via a highpass cutoff: `u16 @0x10`.
   - Each 18-byte block holds a 2-byte scale followed by 32 4-bit samples per channel.
   - Reference: [vgmstream adx_decoder.c](https://github.com/vgmstream/vgmstream/blob/master/src/coding/adx_decoder.c) and [MultimediaWiki ADX](https://wiki.multimedia.cx/index.php/CRI_ADX_ADPCM).
   - Alternative: pre-convert SEs to WAV/OGG with a one-off tool.
6. **Performance:** HCA decodes far faster than realtime. Decode the whole song to f32 PCM on a background thread at song start (about 45 MB for a 2-minute stereo song). Alternatively stream with a ring buffer. Mix with `cpal` (or kira/rodio).

---

## 3. Song preview

- The ACB has **no preview data for songs**:
  - Song HCAs have no `loop` chunk.
  - Song cues have finite Length.
  - `WaveformExtensionDataTable` (22 rows, LoopStart/LoopEnd in samples) only covers the 11 looping cues (MER_BGM_SYS_002/003, TEST_Lch/Rch, …, whose Length is 0xFFFFFFFF). These are menu loops.
- The preview window comes from **`Table/MusicParameterTable`** (decoded by the other research agent into `docs/research/tables/MusicParameterTable.json`):
  - `PreviewBeginTime` (f32 seconds; 0–126.76, 4 songs use 0)
  - `PreviewSeconds` (f32; 10.0 for 344/395 songs, otherwise 9.5–15)
  - Example: `3030 ヒトガタ PreviewBeginTime 57.3 PreviewSeconds 10.0`
- Other useful fields in the same table:
  - `AssetDirectory` (→ `MusicData/<dir>`)
  - `JacketAssetName` (e.g. `S03/uT_J_S03_30` → `UI/Textures/JACKET/S03/uT_J_S03_30.uasset`; all 395 resolve)
- The audio cue is **not** in MusicParameterTable. Get it from the chart's `#MUSIC_FILE_PATH`.
- The preview fade-in/out duration, and whether the preview loops, are **UNVERIFIED** (not in the tables seen). Suggest a 0.5–1 s fade and looping the window.
- `DemoMusicTable` (attract mode) lists 5 songs with `PlayTime 30.0`.

---

## 4. Jackets (`UI/Textures/JACKET/S00..S03`)

UE4 cooked `Texture2D` (the package names `/Script/Engine`, `PF_B8G8R8A8`, `TEXTUREGROUP_UI`,
`TC_EditorIcon`, `SRGB`). The pixel data is in the `.uexp` after the tagged properties, as
`FTexturePlatformData` (little-endian):

```
i32 SizeX, i32 SizeY, u32 PackedData (NumSlices | flags<<30), FString PixelFormat ("PF_B8G8R8A8\0"),
i32 FirstMipToSerialize, i32 NumMips,
per mip: i32 bCooked, u32 BulkDataFlags, i32 ElementCount, i32 SizeOnDisk, i64 OffsetInFile,
         [payload if inline], i32 SizeX, i32 SizeY          (no SizeZ in this engine version)
... repeated per cooked pixel format, terminated by FName "None"; file ends with tag C1 83 2A 9E
```

- `BulkDataFlags 0x48` (ForceInlinePayload | SingleUse): the payload follows immediately. `OffsetInFile = uasset_size + position_in_uexp` (verified for all files).
- `0x501` (PayloadInSeparateFile | …): the payload is in `.ubulk`. The `.ubulk` position is `OffsetInFile + Summary.BulkDataStartOffset`, where BulkDataStartOffset = uasset + uexp − 4. Verified: mip0 −266792 → 0, mip1 1830360 → 2097152.

Survey of all 497 jackets (`jacket_extract.py survey`):

| Count | Format | Size | Notes |
|---|---|---|---|
| 415 | PF_B8G8R8A8 | 256×256 | 1 mip, inline, 262144 bytes. The normal case. |
| 2 | PF_DXT1 2048² (12 mips, 5 in .ubulk) **and** PF_B8G8R8A8 256² | | `S03_52`, `S03_53` carry two platform datas. Use the BGRA one. |
| 4 | PF_B8G8R8A8 | 2048×2048 | 16 MB inline: `S01_277, S02_233, S02_237, S03_0`. None of these is used by MusicParameterTable. |
| 1 | PF_B8G8R8A8 | 512×512 | `S00_000`, used by the tutorial. |
| 58 + 17 | PF_B8G8R8A8 | 16×16 / 4×4 | Stripped placeholders. None is referenced by MusicParameterTable. |

- **Every jacket a song uses is available as uncompressed BGRA8**, so no BCn decoder is needed.
- Bytes are B, G, R, A, rows top-down, no row padding, sRGB. The alpha channel is meaningful (the corners/edges have alpha < 255).

**Rust options:**
1. **Load directly** (recommended). Locate `PF_B8G8R8A8` → walk the header → slice `w*h*4` bytes → swizzle BGRA→RGBA, or upload as `wgpu::TextureFormat::Bgra8UnormSrgb` with no swizzle. About 60 lines and no crates.
2. If a DXT1/BC7 texture ever has to be decoded on the CPU: [`texpresso`](https://crates.io/crates/texpresso) (MIT, v2.0.2, BC1–BC5) or [`bcdec_rs`](https://crates.io/crates/bcdec_rs) (MIT, BC1–BC7). Otherwise upload as `Bc1RgbaUnormSrgb` directly to the GPU.
3. Pre-converting to PNG is optional. `jacket_extract.py png` does it with stdlib zlib (`docs/research/samples/uT_J_S00_003.png`).

---

## 5. Hit sound effects

Note hit sounds are chosen by the player's **SE set**, in `Table/SESetTable` (decoded in
`docs/research/tables/SESetTable.json`). There are 20 rows (`SESetID` 105001…305003). Each row has
these cue-name fields, which are all cues in MER_SE:

| Field | Meaning (from the name) | Default set 205005 ("Default (WACCA)") | Set 105001 ("Default (WACCA Lily R)") |
|---|---|---|---|
| MarvelousSEName | tap/touch hit, Marvelous | MER_SE_SYS_002 (526 ms) | MER_SE_SYS_186 |
| GoodSEName | tap/touch hit, Great/Good | MER_SE_SYS_001 (531 ms) | MER_SE_SYS_185 |
| SlideMarvelousSEName / SlideGoodSEName | slide notes | SYS_009 / SYS_008 | SYS_188 / SYS_187 |
| SnapMarvelousSEName / SnapGoodSEName | snap (in/out) notes | SYS_111 / SYS_110 | SYS_194 / SYS_193 |
| HoldMarvelousName / HoldSEName | hold start + sustain | SYS_010 / SYS_003 | SYS_190 / SYS_189 |
| ReHoldSEName | re-grab during a hold | SYS_004 | SYS_191 |
| ChainSeName | chain notes | SYS_109 | SYS_192 |

- Both 105001 and 205005 have `bIsInitItem = true`.
- The display names come from `Message/AttackSEMessage`: "Default (WACCA Lily R)" and "Default (WACCA)". Which name belongs to which ID is inferred from row order, so it is **UNVERIFIED**.
- The hold cues (SYS_003/004/010, 189/190/191) have **Length 0xFFFFFFFF**. Each is a 2-track sequence: a one-shot attack waveform plus a waveform with a loop region from WaveformExtensionData (e.g. 80825–202520 samples). Play the attack, loop the sustain while the hold is held, and stop on release.
- **Miss:** no miss SE field exists. It is presumably silent – **UNVERIFIED**.
- Other cue groups:
  - `MER_SE_BON_*`: bonus effects. Probably the Marvelous-bonus/combo effects that `BonusEffectVolumeTable` controls (**UNVERIFIED**).
  - `MER_SE_AIM_*`, `MER_SE_CLK_001`: guide/clap/metronome (**UNVERIFIED**).
  - `MER_SE_UI_*`: menu sounds.
- Per-type user volumes (`TouchNoteVolumeTable`, `HoldNoteVolumeTable`, `SlideNoteVolumeTable`, `SnapNoteVolumeTable`, `ChainNoteVolumeTable`, `SNoteVolumeTable`, `BonusEffectVolumeTable`, `GuideVolumeTable`, `BGMVolumeTable`) are 11-step 0–100 % option lists. Each step is a separate gain.

---

## Sources

- vgmstream ACB parser: https://github.com/vgmstream/vgmstream/blob/master/src/meta/acb.c
- vgmstream @UTF reader: https://github.com/vgmstream/vgmstream/blob/master/src/meta/cri_utf.c
- vgmstream AWB/AFS2: https://github.com/vgmstream/vgmstream/blob/master/src/meta/awb.c
- vgmstream clHCA: https://github.com/vgmstream/vgmstream/blob/master/src/coding/libs/clhca.c
- vgmstream license (ISC-style): https://github.com/vgmstream/vgmstream/blob/master/COPYING
- FFmpeg HCA decoder (LGPL-2.1+): https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/hcadec.c
- cridecoder (MIT): https://crates.io/crates/cridecoder, https://github.com/seiunx-dev/cridecoder
- VGAudio (MIT): https://github.com/Thealexbarney/VGAudio
- Original HCA decoder: https://github.com/Nyagamon/HCADecoder
- criware-utf crate (CPK UTF tables): https://crates.io/crates/criware-utf
- ADX: https://wiki.multimedia.cx/index.php/CRI_ADX_ADPCM
- texpresso: https://crates.io/crates/texpresso ; bcdec_rs: https://crates.io/crates/bcdec_rs
- UE4 FByteBulkData / FTexturePlatformData layout: UE4 source `Engine/Source/Runtime/CoreUObject/Private/Serialization/BulkData.cpp`, `Engine/Source/Runtime/Engine/Private/TextureDerivedData.cpp` (requires Epic GitHub access); also CUE4Parse https://github.com/FabianFG/CUE4Parse
