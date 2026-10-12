//! Minimal playfield demo: a chart scrolling in real time (no audio).
//!
//! `cargo run -p wacca-render --example playfield -- [path/to/chart.mer]`; without a path a
//! synthetic chart is shown. Keys: Up/Down note speed, Space pause, R restart. `RUST_LOG=info` for diagnostics.

use std::error::Error;
use std::sync::Arc as Shared;
use std::time::Instant;

use wacca_chart::{
    Arc, BeatPos, Bonus, Chart, ChartHeader, HoldNode, MaskDir, MaskEvent, Note, NoteBody,
    ScrollSegment, SlideDir, SnapDir,
};
use wacca_render::{Renderer, SceneOptions, build_scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

const LEAD_IN: f64 = -1.0;

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let chart = match std::env::args_os().nth(1) {
        Some(path) => wacca_chart::parse(&std::fs::read(path)?)?,
        None => demo_chart(),
    };
    log::info!("{} notes, {:.1} s", chart.notes.len(), chart.end_time);
    let mut app = App {
        gfx: None,
        chart,
        clock: Clock::start(LEAD_IN),
        opts: SceneOptions { note_speed: 2.0 },
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
    fn new(window: Shared<Window>) -> Result<Self, Box<dyn Error>> {
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
        let renderer = Renderer::new(&device, &queue, config.format);
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
            .and_then(|w| Gfx::new(Shared::new(w)));
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
                let scene = build_scene(&self.chart, self.clock.now(), &self.opts);
                if let Some(gfx) = &mut self.gfx {
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
