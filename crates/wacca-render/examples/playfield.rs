//! Minimal playfield demo: a chart scrolling in real time (no audio).
//!
//! `cargo run -p wacca-render --example playfield -- [path/to/chart.mer | -] [ui dir]`; without a
//! path a synthetic chart is shown. Sprites are read from the ui dir (default `data/ui`).
//! Keys: Up/Down note speed, Space pause, R restart. `RUST_LOG=info` for diagnostics.
//!
//! `WACCA_SHOT=<out.png>@<seconds>` renders that one moment (1024x1024, as after a perfect play
//! up to it: hit effects, judgement, combo, score row) to a PNG without opening a window.
//! `WACCA_SHOT_BARS=1` adds bar lines, `WACCA_SHOT_TITLE` the song title on the rim.

use std::error::Error;
use std::sync::Arc as Shared;
use std::time::Instant;

use wacca_chart::{
    Arc, BeatPos, Bonus, Chart, ChartHeader, HoldNode, MaskDir, MaskEvent, Note, NoteBody,
    ScrollSegment, SlideDir, SnapDir,
};
use wacca_render::{
    Effects, JudgeSprite, LiveState, Renderer, RimState, SceneOptions, build_scene,
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

    fn draw(&mut self, scene: &[wacca_render::Vertex]) {
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
        self.renderer.render(&mut encoder, &view, size, scene);
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
                    let scene = build_scene(
                        &self.chart,
                        self.clock.now(),
                        &self.opts,
                        &LiveState::default(),
                        gfx.renderer.sprites(),
                    );
                    gfx.draw(&scene);
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
    // Rows must be 256-byte aligned for the readback.
    const SIDE: u32 = 1024;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
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
    // Perfect play up to `at` (WACCA_SHOT_JUDGE=great|good for other judgements): every
    // note judged on time, effects stepped at 120 Hz.
    let kind = match std::env::var("WACCA_SHOT_JUDGE").as_deref() {
        Ok("great") => JudgeSprite::Great,
        Ok("good") => JudgeSprite::Good,
        _ => JudgeSprite::Marvelous,
    };
    let done: Vec<&Note> = chart.notes.iter().filter(|n| n.time <= at).collect();
    let mut fx = Effects::default();
    let dt = 1.0 / 120.0;
    let mut t = at - 0.5;
    while t < at {
        for n in done.iter().filter(|n| n.time > t - dt && n.time <= t) {
            fx.hit(n.arc.lane, n.arc.width, kind);
        }
        let holds = chart.notes.iter().filter_map(|n| match &n.body {
            NoteBody::Hold { nodes } if n.time <= t && nodes.last()?.time > t => {
                wacca_render::effects::hold_arc_at(nodes, t)
            }
            _ => None,
        });
        fx.set_holds(holds.collect());
        fx.update(dt as f32);
        t += dt;
    }
    let last = done.last().map(|n| (at - n.time) as f32);
    // Rim texts and score as for a perfect play (WACCA_SHOT_TITLE names the song).
    let title = std::env::var("WACCA_SHOT_TITLE").unwrap_or_default();
    renderer.set_rim_texts("NORMAL/Lv.1", &title);
    let share = done.len() as f32 / chart.notes.len().max(1) as f32;
    let live = LiveState {
        effects: Some(&fx),
        judgement: last.map(|age| (kind, age)),
        combo: (done.len() as u32, last.unwrap_or(0.0)),
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
        movie: false,
        bar_lines: flag("WACCA_SHOT_BARS"),
        touch_guide: false,
    };
    let scene = build_scene(chart, at, &opts, &live, renderer.sprites());
    let mut encoder = device.create_command_encoder(&Default::default());
    let view = target.create_view(&Default::default());
    renderer.render(&mut encoder, &view, [SIDE, SIDE], &scene);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("snapshot readback"),
        size: u64::from(SIDE * SIDE * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: Some(SIDE),
            },
        },
        target.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely())?;
    let pixels = buffer.slice(..).get_mapped_range().to_vec();
    image::save_buffer(out, &pixels, SIDE, SIDE, image::ExtendedColorType::Rgba8)?;
    log::info!("wrote {}", out.display());
    Ok(())
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
