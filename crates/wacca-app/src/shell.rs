//! winit window + wgpu surface around a [`Session`]: audio-clocked rendering and input.
//!
//! Input timing: every touch/mouse event immediately feeds an `InputFrame` stamped with the chart
//! time read from the audio clock at that moment (cheap: one mutex lock), and every rendered frame
//! feeds one more with the current contacts so time-driven judgements (misses, holds, chains)
//! advance even without input. With `--autoplay`, the scripted frames that fell due are fed
//! instead and user input is ignored.

use std::sync::Arc;

use wacca_audio::Player;
use wacca_chart::Chart;
use wacca_game::grade::PlayResult;
use wacca_game::input::InputFrame;
use wacca_render::layout::playfield_viewport;
use wacca_render::{Renderer, SceneOptions, Vertex, build_scene};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::args::SPEED_RANGE;
use crate::session::{Autoplay, Session, chart_now, end_time, start_position};
use crate::touch_mapping::{Contacts, sensor_at_pixel};

/// Contact id used for the mouse (touch ids come from the OS).
const MOUSE_ID: u64 = u64::MAX;
const SPEED_STEP: f64 = 0.5;

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error("graphics: {0}")]
    Graphics(String),
    #[error("audio: {0}")]
    Audio(#[from] wacca_audio::Error),
}

/// How the play ended.
#[derive(Debug)]
pub enum Outcome {
    Completed(PlayResult),
    GaveUp(PlayResult),
}

/// Everything one play needs; consumed by [`run`].
#[derive(Debug)]
pub struct Play {
    pub title: String,
    pub chart: Chart,
    pub player: Player,
    pub autoplay: bool,
    pub speed: f64,
    /// User offset, seconds.
    pub offset: f64,
    pub norma: f32,
    pub bonus_effect: bool,
}

/// Open the window, play the chart to the end (or until Esc / close) and return the result.
pub fn run(play: Play) -> Result<Outcome, ShellError> {
    let end = end_time(&play.chart, play.player.duration(), play.offset);
    log::info!(
        "{} notes, chart end {:.2} s, audio {:.2} s, play ends at chart {end:.2} s",
        play.chart.notes.len(),
        play.chart.end_time,
        play.player.duration()
    );
    let mut app = App {
        gfx: None,
        session: Session::new(&play.chart, play.bonus_effect),
        autoplay: play.autoplay.then(|| Autoplay::new(&play.chart)),
        opts: SceneOptions {
            note_speed: play.speed,
        },
        end,
        play,
        contacts: Contacts::default(),
        cursor: None,
        mouse_down: false,
        paused: false,
        shown_title: String::new(),
        outcome: None,
        error: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(app.outcome.unwrap_or_else(|| {
        // Window closed without an explicit end.
        Outcome::GaveUp(app.session.give_up(app.play.norma))
    }))
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

fn gfx_err(e: impl std::fmt::Display) -> ShellError {
    ShellError::Graphics(e.to_string())
}

impl Gfx {
    fn new(window: Arc<Window>) -> Result<Self, ShellError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone()).map_err(gfx_err)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(gfx_err)?;
        log::info!("adapter: {:?}", adapter.get_info());
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(gfx_err)?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| gfx_err("surface not supported by adapter"))?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
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

    fn draw(&mut self, scene: &[Vertex]) {
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

struct App {
    gfx: Option<Gfx>,
    play: Play,
    session: Session,
    autoplay: Option<Autoplay>,
    opts: SceneOptions,
    /// Chart time at which the play is over.
    end: f64,
    contacts: Contacts,
    /// Last cursor position in window pixels.
    cursor: Option<[f32; 2]>,
    mouse_down: bool,
    paused: bool,
    shown_title: String,
    outcome: Option<Outcome>,
    error: Option<ShellError>,
}

impl App {
    fn now(&self) -> f64 {
        chart_now(
            &self.play.chart,
            self.play.player.position(),
            self.play.offset,
        )
    }

    fn feed_contacts(&mut self) {
        if self.paused || self.autoplay.is_some() {
            return;
        }
        let frame = InputFrame {
            time: self.now(),
            sensors: self.contacts.sensors(),
        };
        self.session.feed(&frame);
    }

    /// Playfield sensor under a window position (physical pixels, like the surface).
    fn pointer(&mut self, id: u64, at: Option<PhysicalPosition<f64>>) {
        let sensor = at.and_then(|p| {
            let gfx = self.gfx.as_ref()?;
            let vp = playfield_viewport(gfx.config.width as f32, gfx.config.height as f32);
            sensor_at_pixel([p.x as f32, p.y as f32], vp)
        });
        self.contacts.set(id, sensor);
        self.feed_contacts();
    }

    fn touch(&mut self, t: Touch) {
        match t.phase {
            TouchPhase::Started | TouchPhase::Moved => self.pointer(t.id, Some(t.location)),
            TouchPhase::Ended | TouchPhase::Cancelled => self.pointer(t.id, None),
        }
    }

    fn mouse_at(&self) -> Option<PhysicalPosition<f64>> {
        let c = self.cursor.filter(|_| self.mouse_down)?;
        Some(PhysicalPosition::new(f64::from(c[0]), f64::from(c[1])))
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode) {
        match code {
            KeyCode::Escape => {
                log::info!("given up");
                self.outcome = Some(Outcome::GaveUp(self.session.give_up(self.play.norma)));
                event_loop.exit();
            }
            KeyCode::Space => {
                self.paused = !self.paused;
                if self.paused {
                    self.play.player.pause();
                } else {
                    self.play.player.play();
                }
                log::info!("paused: {} at chart {:.3}", self.paused, self.now());
            }
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                let step = if code == KeyCode::ArrowUp {
                    SPEED_STEP
                } else {
                    -SPEED_STEP
                };
                self.opts.note_speed =
                    (self.opts.note_speed + step).clamp(*SPEED_RANGE.start(), *SPEED_RANGE.end());
                log::info!("note speed {:.1}", self.opts.note_speed);
            }
            _ => {}
        }
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let now = self.now();
        if !self.paused {
            match &mut self.autoplay {
                Some(auto) => {
                    for f in auto.due(now) {
                        self.session.feed(f);
                    }
                }
                None => self.feed_contacts(),
            }
            if now >= self.end {
                log::info!("song over at chart {now:.3}");
                self.outcome = Some(Outcome::Completed(self.session.finish(self.play.norma)));
                event_loop.exit();
                return;
            }
        }
        let scene = build_scene(&self.play.chart, now, &self.opts);
        let Some(gfx) = &mut self.gfx else { return };
        gfx.draw(&scene);
        let paused = if self.paused { "  [paused]" } else { "" };
        let title = format!(
            "{}  {}  x{:.1}{paused}",
            self.play.title,
            self.session.status(),
            self.opts.note_speed
        );
        if title != self.shown_title {
            gfx.window.set_title(&title);
            self.shown_title = title;
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), ShellError> {
        let attrs = Window::default_attributes()
            .with_title(&self.play.title)
            .with_inner_size(LogicalSize::new(450.0, 800.0));
        let window = event_loop.create_window(attrs).map_err(gfx_err)?;
        self.gfx = Some(Gfx::new(Arc::new(window))?);
        // Start the music only once the window is up so the lead-in is not spent on setup.
        let start = start_position(&self.play.chart, self.play.offset);
        self.play.player.seek(start)?;
        self.play.player.play();
        log::info!("playing from audio {start:.3} s");
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        if let Err(e) = self.start(event_loop) {
            self.error = Some(e);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.outcome = Some(Outcome::GaveUp(self.session.give_up(self.play.norma)));
                event_loop.exit();
            }
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
                        repeat: false,
                        ..
                    },
                ..
            } => self.key(event_loop, code),
            WindowEvent::Touch(t) => self.touch(t),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some([position.x as f32, position.y as f32]);
                if self.mouse_down {
                    self.pointer(MOUSE_ID, self.mouse_at());
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.pointer(MOUSE_ID, None);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.mouse_down = state == ElementState::Pressed;
                self.pointer(MOUSE_ID, self.mouse_at());
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gfx) = &self.gfx {
            gfx.window.request_redraw();
        }
    }
}
