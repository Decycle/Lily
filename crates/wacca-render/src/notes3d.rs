//! Notes as the game's real 3D meshes (pure, no GPU). Spec: `docs/research/11-shaders-and-meshes.md`
//! §1.3–1.4 (meshes, materials), §2.3–2.4 (camera, note transforms from RenderDoc captures).
//!
//! Placement: [`crate::camera`]. Drawn here: note bodies, hold end caps, bar lines and the R-note
//! aura. The slide chevron and snap arrow sections (mesh sections at or above the note width that
//! are not aura) are left out (ported separately).
//!
//! R notes (`Bonus::R`) use the `…NoteS` meshes: the same body sections plus one wider section
//! per lane (r 158.6–181.1, UV0 u −1…0) drawn with `vMX_2_NoteAura_AUS_m_00` (additive,
//! `vT_SNote00_00_m`, NewNoteColor (2.5, 2.31, 1.04)) — the only material sampling that texture;
//! the KING reference frames show the yellow-white glow around R notes. Simultaneous notes get
//! `SyncNote` = 1 (cyan outline: the band sections at UV0 v < 0.4, KING frames).

use std::collections::HashMap;
use std::path::Path;

use wacca_chart::{Bonus, Chart, LANES, NoteBody, SlideDir, SnapDir};

use crate::camera::{note_scale, note_to_world, world_to_view};
use crate::layout::{last_drawable_time, raw_depth};
use crate::scene::{LiveState, SceneOptions, bar_line_times};

/// The note meshes used for each note kind (`vMsh_<stem>_<width>_s`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MeshKind {
    /// Touch, chain (`DefaultNote00`).
    Default,
    HoldStart,
    /// Body (plus chevron sections, not drawn here).
    Slide,
    /// Body (plus arrow section, not drawn here).
    Snap,
    /// Measure line (`vMsh_BarLine00_60_s`, one full circle; width ignored).
    BarLine,
    /// Hold end cap (`vMsh_HoldEndNote00`).
    HoldEnd,
    /// The R-note ("S") versions with aura sections.
    DefaultS,
    HoldStartS,
    SlideS,
    SnapS,
}

impl MeshKind {
    pub const ALL: [Self; 10] = [
        Self::Default,
        Self::HoldStart,
        Self::Slide,
        Self::Snap,
        Self::BarLine,
        Self::HoldEnd,
        Self::DefaultS,
        Self::HoldStartS,
        Self::SlideS,
        Self::SnapS,
    ];

    fn stem(self) -> &'static str {
        match self {
            Self::Default => "vMsh_DefaultNote00",
            Self::HoldStart => "vMsh_HoldStartNote00",
            Self::Slide => "vMsh_SlideNote00",
            Self::Snap => "vMsh_SnapNote00",
            Self::BarLine => "vMsh_BarLine00",
            Self::HoldEnd => "vMsh_HoldEndNote00",
            Self::DefaultS => "vMsh_DefaultNoteS_00",
            Self::HoldStartS => "vMsh_HoldStartNoteS00",
            Self::SlideS => "vMsh_SlideNoteS00",
            Self::SnapS => "vMsh_SnapNoteS00",
        }
    }

    /// The kind to use when this one's mesh is missing.
    fn fallback(self) -> Option<Self> {
        match self {
            Self::DefaultS => Some(Self::Default),
            Self::HoldStartS => Some(Self::HoldStart),
            Self::SlideS => Some(Self::Slide),
            Self::SnapS => Some(Self::Snap),
            Self::HoldStart | Self::Slide | Self::Snap => Some(Self::Default),
            Self::Default | Self::BarLine | Self::HoldEnd => None,
        }
    }

    /// The R-note version of a note kind.
    fn with_aura(self) -> Self {
        match self {
            Self::Default => Self::DefaultS,
            Self::HoldStart => Self::HoldStartS,
            Self::Slide => Self::SlideS,
            Self::Snap => Self::SnapS,
            other => other,
        }
    }

    /// File name of the mesh for a note `width` lanes wide.
    pub fn file_name(self, width: u8) -> String {
        let width = if self == Self::BarLine { LANES } else { width };
        format!("{}_{width:02}_s.obj", self.stem())
    }
}

/// Material table index (see [`MATERIALS`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum MaterialId {
    Normal,
    Chain,
    Hold,
    SlideL,
    SlideR,
    SnapUp,
    SnapDown,
    BonusN,
    BonusL,
    BonusR,
    /// `vMX_2_BarLine_000`.
    BarLine,
    /// `vMX_2_Hold_003` (hold end cap).
    HoldEnd,
    /// `vMX_2_NoteAura_AUS_m_00` (R-note glow, additive).
    Aura,
}

/// Flag in [`MeshVertex::material`]: the note shares its time with another (`SyncNote` 1).
pub const SYNC_BIT: u32 = 1 << 8;

/// Shader branch of a material (`kind` in `notes.wgsl`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shader {
    /// `vMX_2_NTap_000`.
    Tap,
    BarLine,
    /// `vMX_2_Hold_003`.
    HoldEnd,
    /// `vMX_2_NoteAura_000`.
    Aura,
}

/// Material constants (linear RGB colour and the NTap scalars) from the cooked material
/// instances (doc 11 §1.3; `runs/shaders/materials/*.txt`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub color: [f32; 3],
    /// `Dark`, `HighLight`, `Line` (NTap).
    pub dark: f32,
    pub highlight: f32,
    pub line: f32,
    pub chain: f32,
    pub bonus: f32,
    pub bonus_flip: f32,
    pub shader: Shader,
}

const fn tap(color: [f32; 3], line: f32, chain: f32, bonus: f32, bonus_flip: f32) -> Material {
    Material {
        color,
        dark: 0.8,
        highlight: 0.35,
        line,
        chain,
        bonus,
        bonus_flip,
        shader: Shader::Tap,
    }
}

const fn other(shader: Shader, color: [f32; 3]) -> Material {
    Material {
        shader,
        ..tap(color, 0.0, 0.0, 0.0, 0.0)
    }
}

/// Indexed by [`MaterialId`]. MI values override the master's defaults (`Line` 0.08,
/// `BornusNoteFlip` 1, `ChainNote`/`BornusNote` 0).
pub const MATERIALS: [Material; 13] = [
    // vMX_2_Note_MUS_m_000_Normal00
    tap([1.0, 0.035601, 0.863157], 0.08, 0.0, 0.0, 1.0),
    // Chain00
    tap([1.0, 0.775822, 0.056128], 0.08, 1.0, 0.0, 0.0),
    // Hold00
    tap([0.328125, 0.258969, 0.0], 0.01, 0.0, 0.0, 0.0),
    // SlideL00, SlideR00
    tap([1.0, 0.324505, 0.0], 0.01, 0.0, 0.0, 0.0),
    tap([0.013866, 0.645833, 0.008863], 0.01, 0.0, 0.0, 0.0),
    // SnpUp00, SnpDwn00
    tap([0.730461, 0.005182, 0.005182], 0.01, 0.0, 0.0, 0.0),
    tap([0.009134, 0.417885, 1.0], 0.01, 0.0, 0.0, 1.0),
    // BonusN00, BonusL00, BonusR00
    tap([1.0, 0.035601, 0.863157], 0.01, 0.0, 1.0, 0.0),
    tap([1.0, 0.323143, 0.0], 0.08, 0.0, 1.0, 0.0),
    tap([0.013702, 0.651406, 0.008568], 0.01, 0.0, 1.0, 0.0),
    // vMX_2_BarLine_000: B_Color_2 (Line_Width 1 is in the shader).
    other(Shader::BarLine, [0.466146, 0.468174, 0.5]),
    // vMX_2_Hold_003: colour from the HoldGrd strip (HoldColorNumber 2, in the shader).
    other(Shader::HoldEnd, [1.0, 1.0, 1.0]),
    // vMX_2_NoteAura_AUS_m_00: NewNoteColor.
    other(Shader::Aura, [2.5, 2.305553, 1.041667]),
];

/// Mesh and body material of a note (R notes: the aura mesh, UNVERIFIED for holds/slides/snaps).
pub fn note_style(body: &NoteBody, bonus: Bonus) -> (MeshKind, MaterialId) {
    let (kind, material) = plain_style(body, bonus == Bonus::Bonus);
    if bonus == Bonus::R {
        (kind.with_aura(), material)
    } else {
        (kind, material)
    }
}

fn plain_style(body: &NoteBody, b: bool) -> (MeshKind, MaterialId) {
    use MaterialId as M;
    match body {
        NoteBody::Touch => (MeshKind::Default, if b { M::BonusN } else { M::Normal }),
        NoteBody::Chain => (MeshKind::Default, M::Chain),
        NoteBody::Hold { .. } => (MeshKind::HoldStart, M::Hold),
        NoteBody::Slide(SlideDir::Cw) => (MeshKind::Slide, if b { M::BonusL } else { M::SlideL }),
        NoteBody::Slide(SlideDir::Ccw) => (MeshKind::Slide, if b { M::BonusR } else { M::SlideR }),
        NoteBody::Snap(SnapDir::In) => (MeshKind::Snap, M::SnapUp),
        NoteBody::Snap(SnapDir::Out) => (MeshKind::Snap, M::SnapDown),
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MeshError {
    #[error("line {0}: malformed OBJ statement")]
    Malformed(usize),
    #[error("line {0}: index out of range")]
    BadIndex(usize),
}

/// Triangle soup of one note mesh: per corner position (cm), UV0 and material slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteMesh {
    pub corners: Vec<([f32; 3], [f32; 2], u32)>,
}

/// Parses the OBJ written by `wacca-convert --meshes` (`v`, `vt` as stored, `g section<N>`,
/// triangular `f v/vt`).
pub fn parse_obj(text: &str) -> Result<NoteMesh, MeshError> {
    let (mut v, mut vt) = (Vec::new(), Vec::new());
    let mut mesh = NoteMesh::default();
    let mut slot = 0;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        let mut it = line.split_whitespace();
        let floats = |it: std::str::SplitWhitespace, k: usize| {
            let f: Vec<f32> = it.map(str::parse).collect::<Result<_, _>>().ok()?;
            (f.len() == k).then_some(f)
        };
        match it.next() {
            Some("v") => {
                let f = floats(it, 3).ok_or(MeshError::Malformed(n))?;
                v.push([f[0], f[1], f[2]]);
            }
            Some("vt") => {
                let f = floats(it, 2).ok_or(MeshError::Malformed(n))?;
                vt.push([f[0], f[1]]);
            }
            Some("g") => {
                slot = it
                    .next()
                    .and_then(|g| g.strip_prefix("section"))
                    .and_then(|s| s.parse().ok())
                    .ok_or(MeshError::Malformed(n))?;
            }
            Some("f") => {
                let refs: Vec<&str> = it.collect();
                if refs.len() != 3 {
                    return Err(MeshError::Malformed(n));
                }
                for r in refs {
                    let (a, b) = r.split_once('/').ok_or(MeshError::Malformed(n))?;
                    let index = |s: &str, len: usize| {
                        s.parse::<usize>()
                            .ok()
                            .filter(|&k| (1..=len).contains(&k))
                            .ok_or(MeshError::BadIndex(n))
                    };
                    let (a, b) = (index(a, v.len())?, index(b, vt.len())?);
                    mesh.corners.push((v[a - 1], vt[b - 1], slot));
                }
            }
            _ => {}
        }
    }
    Ok(mesh)
}

/// The loaded note meshes, by kind and width.
#[derive(Clone, Debug, Default)]
pub struct NoteMeshes {
    meshes: HashMap<(MeshKind, u8), NoteMesh>,
}

impl NoteMeshes {
    /// Reads every `<kind>_<width>_s.obj` in `dir`; `None` (draw 2D notes) unless all widths of
    /// the plain note mesh are there. Unreadable files are logged and skipped.
    pub fn load(dir: &Path) -> Option<Self> {
        let mut meshes = HashMap::new();
        for kind in MeshKind::ALL {
            let widths = if kind == MeshKind::BarLine {
                LANES..=LANES
            } else {
                1..=LANES
            };
            for width in widths {
                let path = dir.join(kind.file_name(width));
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                match parse_obj(&text) {
                    Ok(mesh) => {
                        meshes.insert((kind, width), mesh);
                    }
                    Err(e) => log::warn!("{}: {e}", path.display()),
                }
            }
        }
        let me = Self { meshes };
        let complete = (1..=LANES).all(|w| me.get(MeshKind::Default, w).is_some());
        if !complete {
            log::info!("{}: note meshes missing, drawing 2D notes", dir.display());
        }
        complete.then_some(me)
    }

    pub fn insert(&mut self, kind: MeshKind, width: u8, mesh: NoteMesh) {
        self.meshes.insert((kind, width), mesh);
    }

    /// Mesh for `kind` (R-note kinds falling back to the plain kind, note kinds to the plain
    /// note mesh of that width).
    pub fn get(&self, kind: MeshKind, width: u8) -> Option<&NoteMesh> {
        let width = if kind == MeshKind::BarLine {
            LANES
        } else {
            width
        };
        self.meshes
            .get(&(kind, width))
            .or_else(|| self.get(kind.fallback()?, width))
    }
}

/// One corner of a placed note triangle.
#[derive(Clone, Copy, Debug, PartialEq, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct MeshVertex {
    /// `[X, Z, w]` from [`world_to_view`].
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// [`MaterialId`], plus [`SYNC_BIT`].
    pub material: u32,
}

/// Appends note `mesh` at `depth` from `lane`: body sections (material slot below `width`) to
/// `body` with `material` (a [`MaterialId`] plus flags), aura sections (UV0 u < 0) to `aura`;
/// chevron/arrow sections are skipped.
pub fn push_mesh(
    (body, aura): (&mut Vec<MeshVertex>, &mut Vec<MeshVertex>),
    mesh: &NoteMesh,
    depth: f32,
    lane: f32,
    width: u8,
    material: u32,
) {
    let s = note_scale(depth);
    for tri in mesh.corners.as_chunks::<3>().0 {
        let (out, material) = if tri[0].2 < u32::from(width) {
            (&mut *body, material)
        } else if tri.iter().any(|c| c.1[0] < -0.25) {
            (&mut *aura, MaterialId::Aura as u32)
        } else {
            continue;
        };
        out.extend(tri.iter().map(|&(p, uv, _)| MeshVertex {
            pos: world_to_view(note_to_world(p, s, lane)),
            uv,
            material,
        }));
    }
}

/// Note, hold end and (with [`SceneOptions::bar_lines`]) bar line triangles of the chart at
/// `now`, like the notes of [`build_scene`](crate::build_scene): first all aura triangles (drawn
/// additively, then covered by the opaque bodies as the depth test does in the game), then the
/// opaque ones, far first.
pub fn build_mesh_notes(
    out: &mut Vec<MeshVertex>,
    meshes: &NoteMeshes,
    chart: &Chart,
    now: f64,
    opts: &SceneOptions,
    live: &LiveState,
) {
    out.clear();
    let last_time = last_drawable_time(chart, now);
    let now_dist = chart.dist_at(chart.view_time(now));
    let depth = |t: f64| Some(raw_depth(chart, t, now_dist, opts.note_speed) as f32);
    let visible = |d: &f32| (0.0..=1.0).contains(d);
    let judged = |i: usize| live.judged.get(i).copied().unwrap_or(false);
    // Simultaneous note heads (notes are sorted by time).
    let notes = &chart.notes;
    let same_time = |i: usize, j: Option<usize>| {
        j.and_then(|j| notes.get(j))
            .is_some_and(|n| n.time == notes[i].time)
    };
    let sync = |i: usize| same_time(i, i.checked_sub(1)) || same_time(i, Some(i + 1));
    // (depth, mesh, first lane, width, material + flags); no depth buffer: drawn far to near.
    let mut items: Vec<(f32, &NoteMesh, f32, u8, u32)> = Vec::new();
    for (i, note) in notes.iter().enumerate() {
        if judged(i) || last_time.is_some_and(|last| note.time > last) {
            continue;
        }
        if let NoteBody::Hold { nodes } = &note.body
            && let Some(end) = nodes.last().filter(|_| nodes.len() > 1)
            && let Some(d) = depth(end.time).filter(visible)
            && let Some(mesh) = meshes.get(MeshKind::HoldEnd, end.arc.width.clamp(1, LANES))
        {
            let (lane, width) = (f32::from(end.arc.lane), end.arc.width.clamp(1, LANES));
            items.push((d, mesh, lane, width, MaterialId::HoldEnd as u32));
        }
        let Some(d) = depth(note.time).filter(visible) else {
            continue;
        };
        let (kind, material) = note_style(&note.body, note.bonus);
        let width = note.arc.width.clamp(1, LANES);
        let flags = if sync(i) { SYNC_BIT } else { 0 };
        if let Some(mesh) = meshes.get(kind, width) {
            let lane = f32::from(note.arc.lane);
            items.push((d, mesh, lane, width, material as u32 | flags));
        }
    }
    if opts.bar_lines
        && let Some(mesh) = meshes.get(MeshKind::BarLine, LANES)
    {
        for d in bar_line_times(chart, now).filter_map(depth).filter(visible) {
            items.push((d, mesh, 0.0, LANES, MaterialId::BarLine as u32));
        }
    }
    // Far first; at equal depth the bar line under the note (its outer edge is nearer).
    let is_note = |m: u32| m != MaterialId::BarLine as u32;
    items.sort_by(|a, b| b.0.total_cmp(&a.0).then(is_note(a.4).cmp(&is_note(b.4))));
    let mut body = Vec::new();
    for (d, mesh, lane, width, material) in items {
        push_mesh((&mut body, out), mesh, d, lane, width, material);
    }
    out.extend(body);
}

/// Number of leading aura vertices in a [`build_mesh_notes`] list.
pub fn aura_count(notes: &[MeshVertex]) -> usize {
    notes.partition_point(|v| v.material == MaterialId::Aura as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJ: &str = "# test\nv 1 2 3\nv 4 5 6\nv 7 8 9\nvt 0 0.5\nvt 1 0.25\n\
                       g section0\nf 1/1 2/2 3/2\ng section2\nf 3/1 2/1 1/2\n";

    #[test]
    fn parses_exported_obj() {
        let m = parse_obj(OBJ).unwrap();
        assert_eq!(m.corners.len(), 6);
        assert_eq!(m.corners[0], ([1.0, 2.0, 3.0], [0.0, 0.5], 0));
        assert_eq!(m.corners[3], ([7.0, 8.0, 9.0], [0.0, 0.5], 2));
        assert_eq!(parse_obj("v 1 2\n"), Err(MeshError::Malformed(1)));
        assert_eq!(
            parse_obj("v 1 2 3\nvt 0 0\nf 1/1 1/1 2/1\n"),
            Err(MeshError::BadIndex(3))
        );
        assert_eq!(parse_obj("f 1/1 1/1\n"), Err(MeshError::Malformed(1)));
    }

    #[test]
    fn sections_split_into_body_aura_and_skipped() {
        let mut mesh = parse_obj(OBJ).unwrap();
        // A third section with negative u: aura.
        let aura_tri = [([1.0, 0.0, 0.0], [-1.0, 0.2], 3); 3];
        mesh.corners.extend(aura_tri);
        let (mut body, mut aura) = (Vec::new(), Vec::new());
        let m = MaterialId::SlideL as u32 | SYNC_BIT;
        push_mesh((&mut body, &mut aura), &mesh, 0.5, 3.0, 2, m);
        // Section 2 (u >= 0) is a chevron section of a width-2 note: skipped.
        assert_eq!((body.len(), aura.len()), (3, 3));
        assert_eq!(body[0].material, m);
        assert_eq!(aura[0].material, MaterialId::Aura as u32);
        let world = note_to_world([1.0, 2.0, 3.0], 0.5, 3.0);
        assert_eq!(body[0].pos, world_to_view(world));
    }

    #[test]
    fn r_notes_use_the_aura_meshes_with_fallback() {
        assert_eq!(note_style(&NoteBody::Touch, Bonus::R).0, MeshKind::DefaultS);
        let slide = note_style(&NoteBody::Slide(SlideDir::Cw), Bonus::R);
        assert_eq!(slide, (MeshKind::SlideS, MaterialId::SlideL));
        let mut m = NoteMeshes::default();
        m.insert(MeshKind::Default, 3, NoteMesh::default());
        assert!(
            m.get(MeshKind::SlideS, 3).is_some(),
            "S -> plain -> default"
        );
        assert!(m.get(MeshKind::HoldEnd, 3).is_none());
        assert_eq!(MATERIALS[MaterialId::Aura as usize].shader, Shader::Aura);
        assert_eq!(MaterialId::Aura as usize, MATERIALS.len() - 1);
    }

    #[test]
    fn note_kinds_pick_meshes_and_materials() {
        use MaterialId as M;
        let s = |b: NoteBody, bonus| note_style(&b, bonus);
        assert_eq!(s(NoteBody::Touch, Bonus::None).1, M::Normal);
        assert_eq!(s(NoteBody::Touch, Bonus::Bonus).1, M::BonusN);
        assert_eq!(
            s(NoteBody::Slide(SlideDir::Cw), Bonus::None),
            (MeshKind::Slide, M::SlideL)
        );
        assert_eq!(
            s(NoteBody::Snap(SnapDir::Out), Bonus::None),
            (MeshKind::Snap, M::SnapDown)
        );
        assert_eq!(MeshKind::Snap.file_name(5), "vMsh_SnapNote00_05_s.obj");
    }

    #[test]
    fn meshes_fall_back_to_the_plain_note() {
        let mut m = NoteMeshes::default();
        m.insert(MeshKind::Default, 4, NoteMesh::default());
        assert!(m.get(MeshKind::Slide, 4).is_some());
        assert!(m.get(MeshKind::Slide, 5).is_none());
    }

    #[test]
    fn notes_and_bar_lines_are_drawn_far_to_near() {
        use wacca_chart::{Arc, BeatPos, ChartHeader, Note, TempoSegment};
        let note = |time: f64| Note {
            pos: BeatPos::default(),
            time,
            arc: Arc { lane: 10, width: 4 },
            bonus: Bonus::None,
            body: NoteBody::Touch,
            file_index: 0,
        };
        let chart = Chart {
            header: ChartHeader::default(),
            notes: vec![note(1.0), note(2.5), note(2.5), note(30.0)],
            masks: vec![],
            // 120 BPM 4/4: a measure every 2 s.
            tempo: vec![TempoSegment {
                start_tick: 0,
                start_sec: 0.0,
                bpm: 120.0,
                num: 4,
                den: 4,
            }],
            scroll: vec![],
            reverses: vec![],
            tutorial_tags: vec![],
            end: BeatPos::default(),
            end_time: 60.0,
        };
        let one = NoteMesh {
            corners: vec![([170.0, 0.0, 10.0], [0.5, 0.5], 0); 3],
        };
        let mut meshes = NoteMeshes::default();
        meshes.insert(MeshKind::Default, 4, one.clone());
        meshes.insert(MeshKind::BarLine, LANES, one);
        let opts = SceneOptions {
            bar_lines: true,
            ..SceneOptions::default()
        };
        let mut out = Vec::new();
        let judged = [true];
        let live = LiveState {
            judged: &judged,
            ..LiveState::default()
        };
        build_mesh_notes(&mut out, &meshes, &chart, 0.5, &opts, &live);
        let kinds: Vec<u32> = out.iter().step_by(3).map(|v| v.material).collect();
        // Visible window 0.5..3.83 s: bar line at 2 s, two simultaneous (sync) notes at 2.5 s;
        // the 1 s note is judged.
        let (bar, normal) = (MaterialId::BarLine as u32, MaterialId::Normal as u32);
        let sync = normal | SYNC_BIT;
        assert_eq!(kinds, [sync, sync, bar]);
        assert!(out[0].pos[2] > out[6].pos[2], "the far one first");
        assert_eq!(aura_count(&out), 0);
    }
}
