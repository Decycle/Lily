//! Minimal playfield demo: a chart scrolling in real time (no audio).
//!
//! `cargo run -p wacca-render --example playfield -- [path/to/chart.mer | -] [ui dir]`; without a
//! path a synthetic chart is shown. Sprites are read from the ui dir (default `data/ui`).
//! Keys: Up/Down note speed, Space pause, R restart. `RUST_LOG=info` for diagnostics.
//!
//! `WACCA_SHOT=<out.png>@<seconds>` renders that one moment (1024x1024, as after a perfect play
//! up to it: hit effects, judgement, combo, score row) to a PNG without opening a window.
//! `WACCA_SHOT_BARS=1` adds bar lines, `WACCA_SHOT_TITLE` the song title on the rim. Notes are the
//! game's 3D meshes when `data/meshes` exists (`wacca-convert --meshes`); `WACCA_SHOT_2D=1` forces
//! the 2D notes. `WACCA_SHOT_SIZE` sets the side (px), `WACCA_SHOT_MOVIE=<dir>` shows a movie
//! frame from `<dir>/{y,u,v}.png` (8-bit grey planes), `WACCA_SHOT_3D=1` leaves out the 2D layer
//! (compare with the game's main view before its HUD). Without a movie the background scene
//! shows: `WACCA_SHOT_BG=normal` (default), `good` or `off`;
//! `WACCA_SHOT_BG_CLOCK=<time>,<bar>,<beat>` sets its material clocks, `WACCA_SHOT_BG_CAPTURE=<png>`
//! also saves its capture target.

use std::error::Error;
use std::sync::Arc as Shared;
use std::time::Instant;

use wacca_chart::{
    Arc, BeatPos, Bonus, Chart, ChartHeader, HoldNode, MaskDir, MaskEvent, Note, NoteBody,
    ScrollSegment, SlideDir, SnapDir,
};
use wacca_render::{
    Background, BgState, Effects, JudgeSprite, LiveState, Renderer, RimState, SceneOptions, Stage,
    build_mesh_notes, build_scene_into, build_stage_into,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const LEAD_IN: f64 = -1.0;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let chart = match std::env::args_os().nth(1).filter(|p| p != "-") {
        Some(path) => wacca_chart::parse(&std::fs::read(path)?)?,
        None => demo_chart(),
    };
    let ui = std::env::args_os()
        .nth(2)
        .map_or_else(|| "data/ui".into(), std::path::PathBuf::from);
    log::info!("{} notes, {:.1} s", chart.notes.len(), chart.end_time);
    if let Some(shot) = std::env::var_os("WACCA_SHOT") {
        let shot = shot.to_string_lossy().into_owned();
        let (out, at) = shot
            .rsplit_once('@')
            .ok_or("WACCA_SHOT=<out.png>@<seconds>")?;
        return snapshot(&chart, &ui, at.parse()?, std::path::Path::new(out));
    }
    let mut app = App {
        gfx: None,
        ui,
        chart,
        clock: Clock::start(LEAD_IN),
        opts: SceneOptions {
            note_speed: 2.0,
            ..Default::default()
        },
    };
    EventLoop::new()?.run_app(&mut app)?;
    Ok(())
}

struct Gfx {
    window: Shared<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

impl Gfx {
    fn new(window: Shared<Window>, ui: &std::path::Path) -> Result<Self, Box<dyn Error>> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;
        log::info!("adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface not supported by adapter")?;
        surface.configure(&device, &config);
        let mut renderer = Renderer::new(&device, &queue, config.format);
        renderer.load_sprites(ui);
        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    fn draw(&mut self, chart: &Chart, now: f64, opts: &SceneOptions) {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(e) => {
                log::warn!("surface: {e}; reconfiguring");
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let size = [self.config.width, self.config.height];
        let (mut scene, mut notes) = (Vec::new(), Vec::new());
        let live = LiveState::default();
        let opts = SceneOptions {
            mesh_notes: self.renderer.note_meshes().is_some(),
            ..*opts
        };
        if let Some(meshes) = self.renderer.note_meshes() {
            build_mesh_notes(&mut notes, meshes, chart, now, &opts, &live);
        }
        build_scene_into(
            &mut scene,
            chart,
            now,
            &opts,
            &live,
            self.renderer.sprites(),
        );
        let mut stage = Stage::default();
        build_stage_into(
            &mut stage,
            chart,
            now,
            &opts,
            &live,
            self.renderer.note_meshes(),
        );
        self.renderer
            .render_with_notes(&mut encoder, &view, size, &scene, &notes, &stage);
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        frame.present();
    }
}

/// Wall-clock song time with pause.
struct Clock {
    base: f64,
    running_since: Option<Instant>,
}

impl Clock {
    fn start(at: f64) -> Self {
        Self {
            base: at,
            running_since: Some(Instant::now()),
        }
    }

    fn now(&self) -> f64 {
        self.base
            + self
                .running_since
                .map_or(0.0, |s| s.elapsed().as_secs_f64())
    }

    fn toggle_pause(&mut self) {
        self.base = self.now();
        self.running_since = match self.running_since {
            Some(_) => None,
            None => Some(Instant::now()),
        };
    }
}

struct App {
    gfx: Option<Gfx>,
    ui: std::path::PathBuf,
    chart: Chart,
    clock: Clock,
    opts: SceneOptions,
}

impl App {
    fn key(&mut self, code: KeyCode) {
        match code {
            KeyCode::ArrowUp => self.opts.note_speed = (self.opts.note_speed + 0.5).min(6.0),
            KeyCode::ArrowDown => self.opts.note_speed = (self.opts.note_speed - 0.5).max(1.0),
            KeyCode::Space => self.clock.toggle_pause(),
            KeyCode::KeyR => self.clock = Clock::start(LEAD_IN),
            _ => return,
        }
        log::info!(
            "note speed {:.1}, t = {:.2}",
            self.opts.note_speed,
            self.clock.now()
        );
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("wacca-render playfield")
            .with_inner_size(LogicalSize::new(450.0, 800.0));
        let gfx = event_loop
            .create_window(attrs)
            .map_err(Box::from)
            .and_then(|w| Gfx::new(Shared::new(w), &self.ui));
        match gfx {
            Ok(gfx) => self.gfx = Some(gfx),
            Err(e) => {
                log::error!("graphics init failed: {e}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => self.key(code),
            WindowEvent::RedrawRequested => {
                if self.clock.now() > self.chart.end_time + 1.0 {
                    self.clock = Clock::start(LEAD_IN);
                }
                if let Some(gfx) = &mut self.gfx {
                    gfx.draw(&self.chart, self.clock.now(), &self.opts);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }
}

/// Render the chart at `at` seconds offscreen and save it as a PNG.
fn snapshot(
    chart: &Chart,
    ui: &std::path::Path,
    at: f64,
    out: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    let side: u32 = std::env::var("WACCA_SHOT_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = Renderer::new(&device, &queue, format);
    renderer.load_sprites(ui);
    let movie = std::env::var_os("WACCA_SHOT_MOVIE").map(std::path::PathBuf::from);
    if let Some(dir) = &movie {
        let plane = |name| -> Result<image::GrayImage, Box<dyn Error>> {
            Ok(image::open(dir.join(name))?.to_luma8())
        };
        let (y, u, v) = (plane("y.png")?, plane("u.png")?, plane("v.png")?);
        renderer.set_video_frame([y.width(), y.height()], [&y, &u, &v]);
        renderer.show_video(true);
    }
    // Perfect play up to `at` (WACCA_SHOT_JUDGE=great|good for other judgements): every
    // note judged on time, effects stepped at 120 Hz. WACCA_SHOT_REPLAY=<json> replays a
    // recorded play instead (see `Replay`).
    let kind = match std::env::var("WACCA_SHOT_JUDGE").as_deref() {
        Ok("great") => JudgeSprite::Great,
        Ok("good") => JudgeSprite::Good,
        Ok("miss") => JudgeSprite::Miss,
        _ => JudgeSprite::Marvelous,
    };
    let replay = match std::env::var_os("WACCA_SHOT_REPLAY") {
        Some(path) => serde_json::from_slice::<Replay>(&std::fs::read(path)?)?.complete(chart),
        None => Replay::perfect(chart, kind),
    };
    let mut fx = Effects::default();
    fx.set_library(renderer.effect_library());
    // Without a movie: the background scene, Normal or Good (WACCA_SHOT_BG=good), warmed up as
    // the game starts it; WACCA_SHOT_BG=off leaves it out.
    let mut background = match std::env::var("WACCA_SHOT_BG").as_deref() {
        _ if movie.is_some() => None,
        Ok("off") => None,
        state => renderer.background_library().map(|lib| {
            let mut bg = Background::new(lib);
            bg.set_state(if state == Ok("good") {
                BgState::Good
            } else {
                BgState::Normal
            });
            bg
        }),
    };
    let dt = 1.0 / 120.0;
    let mut t = at - 0.5;
    while t < at {
        let now = |x: f64| x > t - dt && x <= t;
        for h in &replay.hits {
            let Some(n) = chart.notes.get(h.note) else {
                continue;
            };
            if now(h.hit) {
                fx.hit_note(n.arc.lane, n.arc.width, h.kind.sprite(), n.bonus);
            }
            // Holds pop again over their end arc when judged.
            if let NoteBody::Hold { nodes } = &n.body
                && let Some(end) = nodes.last().filter(|_| now(h.shown))
            {
                fx.hit_note(end.arc.lane, end.arc.width, h.kind.sprite(), n.bonus);
            }
        }
        fx.touch(replay.touched_at(t));
        let holds = replay
            .held_at(chart, t)
            .filter_map(|i| match &chart.notes[i].body {
                NoteBody::Hold { nodes } => wacca_render::effects::hold_arc_at(nodes, t),
                _ => None,
            });
        fx.set_holds(holds.collect());
        fx.update(dt as f32);
        if let Some(bg) = &mut background {
            bg.update(dt as f32);
        }
        t += dt;
    }
    let shown = replay.shown(at);
    let judged: Vec<bool> = (0..chart.notes.len())
        .map(|i| replay.hits.iter().any(|h| h.note == i && h.shown <= at))
        .collect();
    let holding: Vec<usize> = replay.held_at(chart, at).collect();
    let last = shown
        .last()
        .map(|h| (h.kind.sprite(), (at - h.shown) as f32));
    let mut combo = (0u32, f64::NEG_INFINITY);
    for h in &shown {
        combo = match h.kind {
            ReplayJudgement::Miss if combo.0 > 0 => (0, h.shown),
            ReplayJudgement::Miss => combo,
            _ => (combo.0 + 1, h.shown),
        };
    }
    // Rim texts (WACCA_SHOT_LEVEL, default NORMAL/Lv.1; WACCA_SHOT_TITLE names the song) and a
    // score from the judgements (Marvelous full, Great 70 %, Good 50 %).
    let title = std::env::var("WACCA_SHOT_TITLE").unwrap_or_default();
    let level = std::env::var("WACCA_SHOT_LEVEL").unwrap_or_else(|_| "NORMAL/Lv.1".into());
    renderer.set_rim_texts(&level, &title);
    let n = chart.notes.len().max(1) as f32;
    let share = shown.iter().map(|h| h.kind.weight()).sum::<f32>() / n;
    let live = LiveState {
        judged: &judged,
        holding: &holding,
        effects: Some(&fx),
        background: background.as_ref(),
        judgement: last,
        combo: (combo.0, (at - combo.1).min(1e3) as f32),
        rim: Some(RimState {
            score: (share * 1_000_000.0) as u32,
            progress: (at / chart.end_time) as f32,
            difficulty_color: [0.20, 0.70, 1.0, 1.0],
        }),
        gauge: Some((share, 0.5)),
        ..LiveState::default()
    };
    let flag = |name| std::env::var_os(name).is_some();
    let opts = SceneOptions {
        note_speed: std::env::var("WACCA_SHOT_SPEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2.0),
        movie: movie.is_some(),
        bar_lines: flag("WACCA_SHOT_BARS"),
        touch_guide: false,
        mesh_notes: renderer.note_meshes().is_some() && !flag("WACCA_SHOT_2D"),
    };
    let mut notes = Vec::new();
    if let Some(meshes) = renderer.note_meshes().filter(|_| opts.mesh_notes) {
        build_mesh_notes(&mut notes, meshes, chart, at, &opts, &live);
    }
    let mut scene = Vec::new();
    if !flag("WACCA_SHOT_3D") {
        build_scene_into(&mut scene, chart, at, &opts, &live, renderer.sprites());
    }
    let mut stage = Stage::default();
    let meshes = renderer.note_meshes().filter(|_| opts.mesh_notes);
    build_stage_into(&mut stage, chart, at, &opts, &live, meshes);
    // WACCA_SHOT_BG_CLOCK=<GameTime>,<Bar>,<Beat> sets the background material clocks (e.g. to a
    // RenderDoc frame's values).
    if let Ok(clock) = std::env::var("WACCA_SHOT_BG_CLOCK") {
        let v: Vec<f32> = clock.split(',').filter_map(|x| x.parse().ok()).collect();
        if let [time, bar, beat] = v[..] {
            (
                stage.background.time,
                stage.background.bar,
                stage.background.beat,
            ) = (time, bar, beat);
        }
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    let view = target.create_view(&Default::default());
    renderer.render_with_notes(&mut encoder, &view, [side, side], &scene, &notes, &stage);
    queue.submit([encoder.finish()]);
    save_texture(&device, &queue, &target, out)?;
    // WACCA_SHOT_BG_CAPTURE=<png>: the background capture too (linear colour as stored).
    if let Some(path) = std::env::var_os("WACCA_SHOT_BG_CAPTURE") {
        save_texture(
            &device,
            &queue,
            renderer.background_capture(),
            std::path::Path::new(&path),
        )?;
    }
    Ok(())
}

/// Copies an RGBA8 `texture` to the PNG `out`.
fn save_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    out: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    let [w, h] = [texture.width(), texture.height()];
    // Readback rows are 256-byte aligned.
    let row = (w * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely())?;
    let padded = buffer.slice(..).get_mapped_range().to_vec();
    let pixels: Vec<u8> = padded
        .chunks(row as usize)
        .flat_map(|r| &r[..w as usize * 4])
        .copied()
        .collect();
    image::save_buffer(out, &pixels, w, h, image::ExtendedColorType::Rgba8)?;
    log::info!("wrote {}", out.display());
    Ok(())
}

/// A play to render (`WACCA_SHOT_REPLAY`, JSON): judgements and pressed sensors in chart time,
/// e.g. from a recording of the real game (`tools/research/parity_audit.py`).
#[derive(Debug, Default, serde::Deserialize)]
struct Replay {
    hits: Vec<ReplayHit>,
    /// `(time, lanes)`: from `time` on, the lanes with a set bit are touched (any depth).
    /// Empty: the perfect play's taps.
    #[serde(default)]
    touches: Vec<(f64, u64)>,
    /// Judgement of the notes without a hit (on time).
    #[serde(default)]
    default: Option<ReplayJudgement>,
}

#[derive(Debug, serde::Deserialize)]
struct ReplayHit {
    note: usize,
    kind: ReplayJudgement,
    /// Hit effect onset (hold: the head).
    hit: f64,
    /// Judgement art shown and the note removed (hold: its end).
    shown: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
enum ReplayJudgement {
    Marvelous,
    Great,
    Good,
    Miss,
}

impl ReplayJudgement {
    fn sprite(self) -> JudgeSprite {
        match self {
            Self::Marvelous => JudgeSprite::Marvelous,
            Self::Great => JudgeSprite::Great,
            Self::Good => JudgeSprite::Good,
            Self::Miss => JudgeSprite::Miss,
        }
    }

    fn weight(self) -> f32 {
        match self {
            Self::Marvelous => 1.0,
            Self::Great => 0.7,
            Self::Good => 0.5,
            Self::Miss => 0.0,
        }
    }
}

impl Replay {
    /// Every note judged `kind` on time; each note's sensors pressed for a 34 ms tap (or the
    /// hold), as the E3 taps.
    fn perfect(chart: &Chart, kind: JudgeSprite) -> Self {
        let kind = match kind {
            JudgeSprite::Marvelous => ReplayJudgement::Marvelous,
            JudgeSprite::Great => ReplayJudgement::Great,
            JudgeSprite::Good => ReplayJudgement::Good,
            JudgeSprite::Miss => ReplayJudgement::Miss,
        };
        let end = |n: &Note| match &n.body {
            NoteBody::Hold { nodes } => nodes.last().map_or(n.time, |e| e.time),
            _ => n.time + 0.034,
        };
        let hits = chart.notes.iter().enumerate().map(|(note, n)| ReplayHit {
            note,
            kind,
            hit: n.time,
            shown: match &n.body {
                NoteBody::Hold { .. } => end(n),
                _ => n.time,
            },
        });
        let mut edges: Vec<f64> = chart.notes.iter().flat_map(|n| [n.time, end(n)]).collect();
        edges.sort_by(f64::total_cmp);
        edges.dedup();
        let touches = edges.iter().map(|&t| {
            let lanes = chart.notes.iter().filter(|n| n.time <= t && t < end(n));
            let mask = lanes.fold(0, |m, n| {
                (0..n.arc.width).fold(m, |m, i| m | 1u64 << ((n.arc.lane + i) % 60))
            });
            (t, mask)
        });
        Self {
            hits: hits.collect(),
            touches: touches.collect(),
            default: None,
        }
    }

    /// Fills in the notes without a hit (as `default`, on time) and missing touches.
    fn complete(mut self, chart: &Chart) -> Self {
        let kind = self.default.unwrap_or(ReplayJudgement::Marvelous);
        let perfect = Self::perfect(chart, kind.sprite());
        let missing: Vec<ReplayHit> = perfect
            .hits
            .into_iter()
            .filter(|p| !self.hits.iter().any(|h| h.note == p.note))
            .collect();
        self.hits.extend(missing);
        if self.touches.is_empty() {
            self.touches = perfect.touches;
        }
        self
    }

    fn touched_at(&self, t: f64) -> u64 {
        self.touches
            .iter()
            .take_while(|e| e.0 <= t)
            .last()
            .map_or(0, |e| e.1)
    }

    /// Hits shown by `t`, in order.
    fn shown(&self, t: f64) -> Vec<&ReplayHit> {
        let mut out: Vec<&ReplayHit> = self.hits.iter().filter(|h| h.shown <= t).collect();
        out.sort_by(|a, b| a.shown.total_cmp(&b.shown));
        out
    }

    /// Holds whose head was hit (not missed) and that have not ended at `t`.
    fn held_at<'a>(&'a self, chart: &'a Chart, t: f64) -> impl Iterator<Item = usize> + 'a {
        self.hits
            .iter()
            .filter(move |h| {
                h.kind != ReplayJudgement::Miss
                    && h.hit <= t
                    && matches!(&chart.notes.get(h.note).map(|n| &n.body),
                        Some(NoteBody::Hold { nodes }) if nodes.last().is_some_and(|e| e.time > t))
            })
            .map(|h| h.note)
    }
}

/// ~30 s of every note kind, a wrapping hold, mask sweeps, hi-speed, a stop and a negative hi-speed.
fn demo_chart() -> Chart {
    let note = |time: f64, lane: u8, width: u8, bonus: Bonus, body: NoteBody| Note {
        pos: BeatPos::default(),
        time,
        arc: Arc {
            lane: lane % 60,
            width,
        },
        bonus,
        body,
        file_index: 0,
    };
    let mut notes = Vec::new();
    for i in 0..112u8 {
        let t = 2.0 + f64::from(i) * 0.25;
        let (lane, width) = (i.wrapping_mul(7) % 60, 3 + i % 6);
        let (bonus, body) = match i % 8 {
            0 => (Bonus::None, NoteBody::Touch),
            1 => (Bonus::None, NoteBody::Chain),
            2 => (Bonus::None, NoteBody::Slide(SlideDir::Cw)),
            3 => (Bonus::None, NoteBody::Slide(SlideDir::Ccw)),
            4 => (Bonus::None, NoteBody::Snap(SnapDir::In)),
            5 => (Bonus::None, NoteBody::Snap(SnapDir::Out)),
            6 => (Bonus::R, NoteBody::Touch),
            _ => (Bonus::Bonus, NoteBody::Touch),
        };
        notes.push(note(t, lane, width, bonus, body));
    }
    notes.push(note(10.0, 0, 60, Bonus::None, NoteBody::Chain));
    for start in [3.0, 11.0, 19.0, 27.0] {
        // Sweeps counter-clockwise across lane 59 -> 0 (unwrapped 54..66).
        let nodes: Vec<HoldNode> = (0..=6)
            .map(|k| {
                let lane_unwrapped = 54.0 + 2.0 * k as f32;
                HoldNode {
                    pos: BeatPos::default(),
                    time: start + 0.4 * f64::from(k),
                    arc: Arc {
                        lane: (lane_unwrapped as u8) % 60,
                        width: 6,
                    },
                    lane_unwrapped,
                    control: true,
                }
            })
            .collect();
        notes.push(note(start, 54, 6, Bonus::None, NoteBody::Hold { nodes }));
    }
    notes.sort_by(|a, b| a.time.total_cmp(&b.time));

    let mask = |time: f64, lane: u8, width: u8, add: bool, dir: MaskDir| MaskEvent {
        pos: BeatPos::default(),
        time,
        arc: Arc { lane, width },
        add,
        dir,
    };
    let masks = vec![
        mask(0.0, 0, 60, true, MaskDir::Center),
        mask(16.0, 30, 30, false, MaskDir::Minus),
        mask(18.0, 30, 30, true, MaskDir::Plus),
    ];

    // (start, speed): hi-speed x2, stop, negative hi-speed.
    let mut scroll: Vec<ScrollSegment> = Vec::new();
    for (start_sec, speed) in [
        (0.0, 1.0),
        (12.0, 2.0),
        (13.0, 0.0),
        (14.0, 1.0),
        (24.0, -0.5),
        (25.0, 1.0),
    ] {
        let start_dist = scroll
            .last()
            .map_or(0.0, |s| s.start_dist + (start_sec - s.start_sec) * s.speed);
        scroll.push(ScrollSegment {
            start_sec,
            start_dist,
            speed,
        });
    }

    Chart {
        header: ChartHeader::default(),
        notes,
        masks,
        tempo: vec![],
        scroll,
        reverses: vec![],
        tutorial_tags: vec![],
        end: BeatPos::default(),
        end_time: 31.0,
    }
}
