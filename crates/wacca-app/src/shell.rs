//! winit window + wgpu surface: runs the screen state machine, draws screens, routes input.
//! All decisions live in the pure modules (`screen`, `menu`, `select`, `options`, `results`,
//! `hud`); this file only wires them to the window, audio and save file.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use wacca_audio::{BgmBank, DEFAULT_SE_SET, HitSounds, Player, SeBank};
use wacca_game::grade::PlayResult;
use wacca_game::rating;
use wacca_game::save::{self, SaveData};
use wacca_render::layout::{Rect, letterbox, playfield_viewport};
use wacca_render::text::{Fonts, TextRenderer};
use wacca_render::{JudgeSprite, Renderer, Sprite, SpriteSheet, Vertex};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{
    ElementState, KeyEvent, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::canvas::{Canvas, ease};
use crate::gesture::{Gesture, Pointer, Scroller};
use crate::menu::Menu;
use crate::pacer::Pacer;
use crate::perf::{Drift, Perf};
use crate::play::{PlayOptions, PlaySpec, PlayState, VideoSetup, load_chart, movie_path};
use crate::results::Summary;
use crate::screen::{self, Event, Next, Screen};
use crate::ui::{Action, Item, View, WHITE};
use crate::{
    category, hud, jacket, mode, options, play, preview, record, results, select_ui, title, wipe,
};

/// Contact id used for the mouse (touch ids come from the OS).
const MOUSE_ID: u64 = u64::MAX;
const SPEED_STEP: f64 = 0.5;
/// Time constant of the wheel scroll easing (s).
const WHEEL_TAU: f32 = 0.07;

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("event loop: {0}")]
    EventLoop(#[from] winit::error::EventLoopError),
    #[error("graphics: {0}")]
    Graphics(String),
}

fn ms(d: std::time::Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

fn gfx_err(e: impl std::fmt::Display) -> ShellError {
    ShellError::Graphics(e.to_string())
}

/// Everything the game needs, prepared by `main`.
#[derive(Debug)]
pub struct Setup {
    pub assets: PathBuf,
    pub data: PathBuf,
    pub bgm: BgmBank,
    /// The one output device, opened for the app's lifetime (preview and play share it).
    pub player: Arc<Player>,
    pub se: Option<SeBank>,
    pub save_path: Option<PathBuf>,
    pub save: SaveData,
    pub menu: Menu,
    /// Play this right away (`--chart`).
    pub start: Option<PlaySpec>,
    pub windowed: bool,
    /// External decoder for background movies (`None` = no movies).
    pub ffmpeg: Option<PathBuf>,
    /// Process start (startup time).
    pub started: Instant,
}

pub fn run(setup: Setup, fonts: Fonts) -> Result<(), ShellError> {
    let mut app = App {
        s: setup,
        fonts: Some(fonts),
        gfx: None,
        screen: Screen::Title,
        entered: Instant::now(),
        wipe: None,
        play: None,
        summary: None,
        results_at: Instant::now(),
        last_spec: None,
        hits: None,
        preview: PreviewPlayer::default(),
        pointers: HashMap::new(),
        scroller: Scroller::default(),
        cursor: None,
        mouse_down: false,
        alt: false,
        jackets_tried: HashSet::new(),
        ui_tried: HashSet::new(),
        wheel: 0.0,
        last_frame: None,
        error: None,
        perf: Perf::default(),
        show_perf: false,
        drift: Drift::default(),
        input_at: None,
        first_frame: false,
        pacer: Pacer::new(std::time::Duration::ZERO),
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.error.map_or(Ok(()), Err)
}

struct Gfx {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    text: TextRenderer,
    /// Present mode and frame latency (for the perf overlay).
    mode: String,
    /// Seconds per refresh of the window's monitor (0 = unknown).
    period: f64,
}

/// Where one [`Gfx::draw`] spent its time.
#[derive(Clone, Copy, Debug)]
struct DrawTimes {
    submit: std::time::Duration,
    present: std::time::Duration,
}

impl Gfx {
    /// `ui_dir`: exported UI/Vfx textures (`data/ui`) for the playfield sprites.
    fn new(window: Arc<Window>, fonts: Fonts, ui_dir: &Path) -> Result<Self, ShellError> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance.create_surface(window.clone()).map_err(gfx_err)?;
        let power_preference = wgpu::PowerPreference::from_env().unwrap_or_default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference,
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
        let caps = surface.get_capabilities(&adapter);
        log::info!("present modes: {:?}", caps.present_modes);
        config.present_mode = match std::env::var("WACCA_PRESENT").as_deref() {
            Ok("mailbox") => wgpu::PresentMode::Mailbox,
            Ok("immediate") => wgpu::PresentMode::Immediate,
            _ => wgpu::PresentMode::AutoVsync,
        };
        if let Some(n) = std::env::var("WACCA_FRAME_LATENCY")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            config.desired_maximum_frame_latency = n;
        }
        surface.configure(&device, &config);
        let mode = format!(
            "{:?}, latency {}",
            config.present_mode, config.desired_maximum_frame_latency
        );
        log::info!(
            "surface: {:?} {}x{}, {mode}",
            config.format,
            config.width,
            config.height
        );
        let period = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .map_or(0.0, |mhz| 1000.0 / f64::from(mhz));
        let mut renderer = Renderer::new(&device, &queue, config.format);
        renderer.load_sprites(ui_dir);
        let text = TextRenderer::new(&device, &queue, config.format, fonts);
        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            text,
            mode,
            period,
        })
    }

    fn size(&self) -> [f32; 2] {
        [self.config.width as f32, self.config.height as f32]
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// The next swap-chain image; with vsync this waits for the display.
    fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        self.surface
            .get_current_texture()
            .map_err(|e| {
                log::warn!("surface: {e}; reconfiguring");
                self.surface.configure(&self.device, &self.config);
            })
            .ok()
    }

    /// Playfield `scene` (may be empty), then the `view` overlay, onto `frame`.
    fn draw(
        &mut self,
        frame: wgpu::SurfaceTexture,
        scene: &[Vertex],
        view: &View,
        jackets: &mut Images,
        ui: &mut Images,
    ) -> DrawTimes {
        let t1 = Instant::now();
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let size = [self.config.width, self.config.height];
        self.renderer.render(&mut encoder, &target, size, scene);
        self.text.begin(size);
        for item in &view.items {
            match item {
                Item::Fill { rect, color } => self.text.fill(*rect, *color),
                Item::Text {
                    text,
                    at,
                    px,
                    color,
                    align,
                    max_width,
                    face,
                } => self
                    .text
                    .text(text, *at, *px, *color, *align, *max_width, *face),
                Item::TurnedText {
                    text,
                    at,
                    px,
                    color,
                    align,
                    angle,
                } => self
                    .text
                    .text_turned(text, *at, *px, *color, *align, *angle),
                Item::Image {
                    key,
                    rect,
                    uv,
                    angle,
                    tint,
                } => {
                    ui.ensure(&mut self.text, key);
                    self.text.image_region(key, *rect, *uv, *angle, *tint);
                }
                Item::Jacket { rect, key } => {
                    jackets.ensure(&mut self.text, key);
                    if self.text.has_image(key) {
                        self.text.image(key, *rect, WHITE);
                    } else {
                        self.text.fill(*rect, jacket::placeholder_color(key));
                    }
                }
                Item::Poly { points, colors } => self.text.fill_poly(points, colors),
                Item::ImagePoly {
                    key,
                    points,
                    uvs,
                    tint,
                } => {
                    ui.ensure(&mut self.text, key);
                    self.text.image_poly(key, points, uvs, *tint);
                }
            }
        }
        self.text.render(&mut encoder, &target);
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        let t2 = Instant::now();
        frame.present();
        DrawTimes {
            submit: t2 - t1,
            present: t2.elapsed(),
        }
    }
}

/// Loads each image (`<dir>/<key>.png`) once, on first use. Jacket keys (`S03/uT_J_S03_30`)
/// and UI texture keys (`UI/Textures/...`) never collide.
struct Images<'a> {
    dir: PathBuf,
    tried: &'a mut HashSet<String>,
    /// Files loaded this frame and the time spent.
    loads: u64,
    spent: std::time::Duration,
}

impl<'a> Images<'a> {
    fn new(dir: PathBuf, tried: &'a mut HashSet<String>) -> Self {
        Self {
            dir,
            tried,
            loads: 0,
            spent: std::time::Duration::ZERO,
        }
    }

    fn ensure(&mut self, text: &mut TextRenderer, key: &str) {
        if key.is_empty() || !self.tried.insert(key.to_string()) {
            return;
        }
        let t = Instant::now();
        let path = self.dir.join(format!("{key}.png"));
        match jacket::load(&path) {
            Some((size, rgba)) => text.set_image(key, size, &rgba),
            None => log::debug!("no image {}", path.display()),
        }
        self.loads += 1;
        self.spent += t.elapsed();
    }
}

/// Song-select preview through the shared [`Player`], started after the cursor rests.
#[derive(Debug, Default)]
struct PreviewPlayer {
    song: Option<u32>,
    since: Option<Instant>,
    /// The preview loaded into the player.
    window: Option<preview::Window>,
    /// Starting failed for this song; do not retry every frame.
    failed: bool,
    /// Song id → BGM cue (from its chart header).
    cues: HashMap<u32, String>,
}

impl PreviewPlayer {
    fn stop(&mut self, player: &Player) {
        if self.window.take().is_some() {
            player.pause();
        }
        self.song = None;
    }

    fn update(&mut self, s: &Setup) {
        let song = s.menu.list.current();
        let id = song.map(|s| s.id);
        if id != self.song {
            self.stop(&s.player);
            self.song = id;
            self.since = Some(Instant::now());
            self.failed = false;
        }
        let Some(song) = song else { return };
        let rested = self
            .since
            .is_some_and(|t| t.elapsed().as_secs_f64() >= preview::DEBOUNCE_SECS);
        if self.window.is_none() && !self.failed && rested {
            let w = preview::Window::new(song.preview_begin, song.preview_seconds);
            match self.start(s, song, w) {
                Ok(()) => self.window = Some(w),
                Err(e) => {
                    log::warn!("preview of {}: {e}", song.folder);
                    self.failed = true;
                }
            }
        }
        if let Some(w) = &self.window {
            let p = &s.player;
            let pos = p.position();
            if w.over(pos)
                && let Err(e) = p.seek(w.begin)
            {
                log::warn!("preview seek: {e}");
            }
            p.set_volume(w.gain(pos));
        }
    }

    fn start(
        &mut self,
        s: &Setup,
        song: &wacca_game::catalog::Song,
        w: preview::Window,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let cue = match self.cues.get(&song.id) {
            Some(c) => c.clone(),
            None => {
                let entry = song.charts.first().ok_or("no chart")?;
                let chart = load_chart(&s.assets.join(&entry.chart_path))?;
                let cue = chart.header.music_file_path;
                self.cues.insert(song.id, cue.clone());
                cue
            }
        };
        let player = &s.player;
        player.load(Box::new(s.bgm.open_cue(&cue)?))?;
        player.seek(w.begin)?;
        player.set_volume(0.0);
        player.play();
        log::debug!("preview {cue} from {:.1} s for {:.1} s", w.begin, w.len);
        Ok(())
    }
}

/// Hit sounds at `rate`, decoded on first use (and again if the rate changes). A bank that
/// fails to decode is dropped so the game plays silently instead of retrying.
fn hit_sounds(
    cache: &mut Option<HitSounds>,
    se: &mut Option<SeBank>,
    rate: u32,
) -> Option<HitSounds> {
    let stale = cache
        .as_ref()
        .is_none_or(|h| h.marvelous.first().is_some_and(|s| s.rate != rate));
    if stale && let Some(bank) = se {
        match HitSounds::load(bank, &DEFAULT_SE_SET, rate) {
            Ok(h) => *cache = Some(h),
            Err(e) => {
                log::warn!("hit sounds: {e}; playing without them");
                *se = None;
            }
        }
    }
    cache
        .clone()
        .filter(|h| h.marvelous.first().is_none_or(|s| s.rate == rate))
}

struct App {
    s: Setup,
    /// Moved into the text renderer once the window exists.
    fonts: Option<Fonts>,
    gfx: Option<Gfx>,
    screen: Screen,
    /// When the current screen appeared (title / mode select animations).
    entered: Instant,
    /// Screen being wiped away and when the wipe started ([`wipe`]).
    wipe: Option<(Screen, Instant)>,
    play: Option<PlayState>,
    summary: Option<Summary>,
    /// When the results screen appeared (its entry animation).
    results_at: Instant,
    last_spec: Option<PlaySpec>,
    /// Decoded once, at the first play.
    hits: Option<HitSounds>,
    preview: PreviewPlayer,
    /// Menu gestures per contact.
    pointers: HashMap<u64, Pointer>,
    scroller: Scroller,
    /// Last cursor position in window pixels.
    cursor: Option<[f32; 2]>,
    mouse_down: bool,
    alt: bool,
    jackets_tried: HashSet<String>,
    ui_tried: HashSet<String>,
    /// Visual offset of the category / song wheel in panels; eases to 0 after a move.
    wheel: f32,
    last_frame: Option<Instant>,
    error: Option<ShellError>,
    perf: Perf,
    /// F3: the perf overlay.
    show_perf: bool,
    drift: Drift,
    /// First play input not yet on screen.
    input_at: Option<Instant>,
    /// The first frame was presented (startup time logged).
    first_frame: bool,
    pacer: Pacer,
}

impl App {
    fn size(&self) -> [f32; 2] {
        self.gfx.as_ref().map_or([1.0, 1.0], Gfx::size)
    }

    /// The 9:16 menu area.
    fn area(&self) -> Rect {
        let [w, h] = self.size();
        letterbox(w, h)
    }

    fn view(&self) -> View {
        let [w, h] = self.size();
        let Some((from, at)) = self.wipe else {
            return self.screen_view(self.screen);
        };
        let t = at.elapsed().as_secs_f32();
        let shown = if wipe::covering(t) { from } else { self.screen };
        let mut v = self.screen_view(shown);
        v.items.extend(wipe::view(Canvas::fit(w, h), t).items);
        // No input while the screens change.
        v.buttons.clear();
        v
    }

    fn screen_view(&self, screen: Screen) -> View {
        let area = self.area();
        let m = &self.s.menu;
        let [w, h] = self.size();
        let canvas = Canvas::fit(w, h);
        let since = self.entered.elapsed().as_secs_f32();
        match screen {
            Screen::Title => title::view(canvas, since),
            Screen::ModeSelect => mode::view(canvas, since),
            Screen::Category => {
                category::view(canvas, &m.list, m.category, self.wheel, &self.s.save)
            }
            Screen::SongSelect => select_ui::view(canvas, &m.list, self.wheel, &self.s.save),
            Screen::Options => {
                let title = m.list.current().map_or("", |s| s.title.as_str());
                options::view(&m.options, &m.settings, m.autoplay, title, area)
            }
            Screen::Results => self
                .summary
                .as_ref()
                .map(|s| results::view(s, area, self.results_at.elapsed().as_secs_f32()))
                .unwrap_or_default(),
            Screen::Play => {
                let [w, h] = self.size();
                let window = Rect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                };
                let art = self.gfx.as_ref().is_some_and(|g| {
                    let sprites = g.renderer.sprites();
                    [Sprite::Judge(JudgeSprite::Marvelous), Sprite::Digit(0)]
                        .into_iter()
                        .all(|s| sprites.get(s).is_some())
                });
                self.play.as_ref().map_or_else(View::default, |p| {
                    let hud = p.hud(art);
                    hud::view(&hud, &p.spec.title, window, playfield_viewport(w, h))
                })
            }
        }
    }

    fn event(&mut self, event_loop: &ActiveEventLoop, e: Event) {
        match screen::next(self.screen, e) {
            Next::Stay => {}
            Next::Quit => event_loop.exit(),
            Next::Go(Screen::Play) => {
                let spec = if self.screen == Screen::Results {
                    self.last_spec.clone()
                } else {
                    self.spec_from_menu()
                };
                if self.screen == Screen::Options {
                    self.save_settings();
                }
                match spec {
                    Some(spec) => self.start_play(spec),
                    None => log::warn!("no chart selected"),
                }
            }
            Next::Go(s) => {
                if wipe::applies(self.screen, s) {
                    self.wipe = Some((self.screen, Instant::now()));
                }
                self.screen = s;
                self.entered = Instant::now();
            }
        }
    }

    fn spec_from_menu(&self) -> Option<PlaySpec> {
        let (song, c) = self.s.menu.list.chart()?;
        Some(PlaySpec {
            title: song.title.clone(),
            jacket: song.jacket.clone(),
            song_id: Some(song.id),
            difficulty: Some(c.difficulty),
            level: Some(c.level),
            chart_path: self.s.assets.join(&c.chart_path),
            norma: c.norma,
            movie: movie_path(&self.s.assets, c.movie.as_deref()),
        })
    }

    fn start_play(&mut self, spec: PlaySpec) {
        self.preview.stop(&self.s.player);
        let m = &self.s.menu;
        let opts = PlayOptions {
            note_speed: f64::from(m.settings.note_speed),
            offset: options::offset_secs(m.settings.judge_offset),
            autoplay: m.autoplay,
            mirror: m.settings.mirror,
            bonus_effect: m.settings.bonus_effect,
            bar_lines: m.settings.bar_lines,
            touch_guide: m.settings.touch_guide,
        };
        if let Some(gfx) = &mut self.gfx {
            gfx.renderer
                .set_rim_texts(&play::rim_difficulty(&spec), &spec.title);
        }
        self.last_spec = Some(spec.clone());
        let (cache, se) = (&mut self.hits, &mut self.s.se);
        let hits = |rate| hit_sounds(cache, se, rate);
        let [w, h] = self.gfx.as_ref().map_or([1080.0; 2], Gfx::size);
        let video = self
            .s
            .ffmpeg
            .as_deref()
            .filter(|_| m.settings.background_video)
            .map(|ffmpeg| VideoSetup {
                ffmpeg,
                size: w.min(h).round() as u32,
            });
        let player = Arc::clone(&self.s.player);
        let t = Instant::now();
        self.drift.reset();
        match PlayState::new(spec, opts, player, &self.s.bgm, hits, video).and_then(|mut p| {
            p.start()?;
            Ok(p)
        }) {
            Ok(p) => {
                log::info!("song loaded in {:.0} ms", ms(t.elapsed()));
                self.play = Some(p);
                self.screen = Screen::Play;
            }
            Err(e) => {
                log::error!("cannot start play: {e}");
                self.screen = Screen::SongSelect;
            }
        }
    }

    fn end_play(&mut self, event_loop: &ActiveEventLoop, result: PlayResult, completed: bool) {
        let Some(play) = self.play.take() else { return };
        let spec = &play.spec;
        let key = spec.song_id.zip(spec.difficulty);
        let songs = self.s.menu.list.songs();
        let before = rating::save_rating(&self.s.save, songs);
        let (mut new_best, mut not_saved) = (false, None);
        match (key, &self.s.save_path) {
            _ if play.opts.autoplay => not_saved = Some("AUTOPLAY: NOT SAVED".to_string()),
            (None, _) => not_saved = Some("UNKNOWN SONG: NOT SAVED".into()),
            (_, None) => not_saved = Some("NO SAVE LOCATION: NOT SAVED".into()),
            (Some((id, d)), Some(path)) => {
                match record::save_result(path, &mut self.s.save, id, d, &result) {
                    Ok(best) => {
                        new_best = best;
                        log::info!(
                            "saved to {}{}",
                            path.display(),
                            if best { " (new best)" } else { "" }
                        );
                    }
                    Err(e) => {
                        log::error!("save: {e}");
                        not_saved = Some("SAVE FAILED".into());
                    }
                }
            }
        }
        let songs = self.s.menu.list.songs();
        let artist = spec
            .song_id
            .and_then(|id| songs.iter().find(|s| s.id == id))
            .map(|s| s.artist.clone())
            .unwrap_or_default();
        self.summary = Some(Summary {
            title: spec.title.clone(),
            artist,
            jacket: spec.jacket.clone(),
            difficulty: spec.difficulty,
            level: spec.level,
            result,
            completed,
            new_best,
            not_saved,
            player: self.s.save.settings.player_name.clone(),
            rating: results::Rating {
                before,
                after: rating::save_rating(&self.s.save, songs),
            },
        });
        self.results_at = Instant::now();
        self.event(event_loop, Event::PlayOver);
    }

    fn save_settings(&mut self) {
        self.s.save.settings = self.s.menu.settings.clone();
        if let Some(path) = &self.s.save_path
            && let Err(e) = save::save(path, &self.s.save)
        {
            log::error!("save settings: {e}");
        }
    }

    /// Wheel position on the category / song screens: (cursor, length).
    fn wheel_pos(&self) -> Option<(usize, usize)> {
        let m = &self.s.menu;
        match self.screen {
            Screen::Category => Some((m.category, m.list.genres().len())),
            Screen::SongSelect => Some((m.list.cursor(), m.list.len())),
            _ => None,
        }
    }

    fn action(&mut self, event_loop: &ActiveEventLoop, a: Action) {
        if self.wipe.is_some() {
            return;
        }
        let (screen, before) = (self.screen, self.wheel_pos());
        if let Some(e) = self.s.menu.act(self.screen, a) {
            self.event(event_loop, e);
        }
        if self.screen != screen {
            self.wheel = 0.0;
        } else if let (Some((b, len)), Some((c, _))) = (before, self.wheel_pos()) {
            self.wheel += select_ui::wrap_delta(b, c, len) as f32;
        }
    }

    fn toggle_fullscreen(&self) {
        if let Some(g) = &self.gfx {
            let full = g.window.fullscreen().is_some();
            g.window
                .set_fullscreen((!full).then_some(Fullscreen::Borderless(None)));
        }
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, code: KeyCode) {
        if code == KeyCode::F11 || (code == KeyCode::Enter && self.alt) {
            self.toggle_fullscreen();
            return;
        }
        if code == KeyCode::F3 {
            self.show_perf = !self.show_perf;
            return;
        }
        if self.screen == Screen::Play {
            match code {
                KeyCode::Escape => {
                    if let Some(p) = &mut self.play {
                        let r = p.give_up();
                        self.end_play(event_loop, r, false);
                    }
                }
                KeyCode::Space => self.play.iter_mut().for_each(PlayState::toggle_pause),
                KeyCode::ArrowUp => self
                    .play
                    .iter_mut()
                    .for_each(|p| p.change_speed(SPEED_STEP)),
                KeyCode::ArrowDown => self
                    .play
                    .iter_mut()
                    .for_each(|p| p.change_speed(-SPEED_STEP)),
                _ => {}
            }
            return;
        }
        let action = match code {
            KeyCode::ArrowUp => Action::Up,
            KeyCode::ArrowDown => Action::Down,
            KeyCode::ArrowLeft => Action::Left,
            KeyCode::ArrowRight => Action::Right,
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => Action::Confirm,
            KeyCode::Escape | KeyCode::Backspace => Action::Back,
            KeyCode::KeyS => Action::NextSort,
            KeyCode::KeyD => Action::Difficulty,
            KeyCode::PageUp => Action::Genre(-1),
            KeyCode::PageDown => Action::Genre(1),
            KeyCode::KeyF => Action::ToggleFilter,
            KeyCode::KeyR => Action::Retry,
            _ => return,
        };
        self.action(event_loop, action);
    }

    /// Pointer `id` pressed (`Some(true)`), moved (`None`) or released (`Some(false)`) at `p`.
    fn pointer(&mut self, event_loop: &ActiveEventLoop, id: u64, p: [f32; 2], down: Option<bool>) {
        if self.screen == Screen::Play {
            let [w, h] = self.size();
            let at = (down != Some(false)).then_some(p);
            self.input_at.get_or_insert_with(Instant::now);
            if let Some(play) = &mut self.play {
                play.pointer(id, at, playfield_viewport(w, h));
            }
            return;
        }
        let g = self.pointers.entry(id).or_default();
        let gesture = match down {
            Some(true) => {
                g.down(p);
                None
            }
            None => g.moved(p),
            Some(false) => g.up(p),
        };
        match gesture {
            Some(Gesture::Tap(at)) => {
                if let Some(a) = self.view().hit(at) {
                    self.action(event_loop, a);
                }
            }
            Some(Gesture::Drag([dx, _])) if self.wheel_pos().is_some() => {
                let step = Canvas::fit(self.size()[0], self.size()[1]).scale * 250.0;
                let n = self.scroller.steps(dx, step);
                if n != 0 {
                    self.action(event_loop, Action::Scroll(n));
                }
            }
            _ => {}
        }
    }

    fn touch(&mut self, event_loop: &ActiveEventLoop, t: Touch) {
        let p = [t.location.x as f32, t.location.y as f32];
        let down = match t.phase {
            TouchPhase::Started => Some(true),
            TouchPhase::Moved => None,
            TouchPhase::Ended | TouchPhase::Cancelled => Some(false),
        };
        self.pointer(event_loop, t.id, p, down);
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        let start = Instant::now();
        // Acquire first: with vsync it waits for the display, and the frame should show the
        // song time after that wait, not before it.
        let Some(target) = self.gfx.as_mut().and_then(Gfx::acquire) else {
            return;
        };
        let acquired = Instant::now();
        self.pacer.acquired(start, acquired);
        self.perf.acquire.push(ms(acquired - start));
        let mut scene = Vec::new();
        match self.screen {
            Screen::Play => {
                let over = self.play.as_mut().and_then(PlayState::frame);
                if let Some(r) = over {
                    self.end_play(event_loop, r, true);
                } else if let Some(p) = &mut self.play {
                    let video = p.video_frame();
                    if let Some(gfx) = &mut self.gfx {
                        gfx.renderer.show_video(video.is_some());
                        if let Some((f, true)) = video
                            && let Some(planes) = f.planes()
                        {
                            gfx.renderer.set_video_frame([f.width, f.height], planes);
                        }
                    }
                    let empty = SpriteSheet::default();
                    let sprites = self.gfx.as_ref().map_or(&empty, |g| g.renderer.sprites());
                    scene = p.scene(sprites);
                    if p.paused {
                        self.drift.reset();
                    } else {
                        let wall = (start - self.s.started).as_secs_f64();
                        let (drift, step) = self.drift.sample(wall, p.now());
                        self.perf.drift = drift;
                        self.perf.jitter.push(step.abs());
                    }
                }
            }
            Screen::SongSelect | Screen::Options => self.preview.update(&self.s),
            Screen::Title | Screen::ModeSelect | Screen::Category => {
                self.preview.stop(&self.s.player)
            }
            Screen::Results => {}
        }
        let dt = self
            .last_frame
            .replace(start)
            .map_or(0.0, |t| (start - t).as_secs_f32());
        if dt > 0.0 {
            self.perf.frame(f64::from(dt));
        }
        self.wheel = ease(self.wheel, dt, WHEEL_TAU);
        if self
            .wipe
            .is_some_and(|(_, at)| wipe::done(at.elapsed().as_secs_f32()))
        {
            self.wipe = None;
        }
        if self.screen != Screen::Play
            && let Some(gfx) = &mut self.gfx
        {
            gfx.renderer.show_video(false);
        }
        let mut view = self.view();
        if self.show_perf {
            let [w, h] = self.size();
            let window = Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            };
            view.items.extend(self.perf.view(window).items);
        }
        self.perf.build.push(ms(acquired.elapsed()));
        let mut jackets = Images::new(self.s.data.join("jackets"), &mut self.jackets_tried);
        let mut ui = Images::new(self.s.data.join("ui"), &mut self.ui_tried);
        let Some(gfx) = &mut self.gfx else { return };
        let t = gfx.draw(target, &scene, &view, &mut jackets, &mut ui);
        let p = &mut self.perf;
        p.image_loads += jackets.loads + ui.loads;
        p.image_ms += ms(jackets.spent + ui.spent);
        p.submit.push(ms(t.submit));
        p.present.push(ms(t.present));
        if let Some(at) = self.input_at.take() {
            p.input.push(ms(at.elapsed()));
        }
        (p.period, p.mode) = (gfx.period, gfx.mode.clone());
        p.audio = self
            .s
            .player
            .output_stats()
            .map(|(frames, latency)| (frames, ms(latency)));
        if p.frames % 240 == 0 {
            p.log();
        }
        if !self.first_frame {
            self.first_frame = true;
            log::info!(
                "first frame {:.0} ms after start",
                ms(self.s.started.elapsed())
            );
        }
    }

    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<(), ShellError> {
        let mut attrs = Window::default_attributes()
            .with_title("WACCA_slim")
            .with_inner_size(LogicalSize::new(450.0, 800.0));
        if !self.s.windowed {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let window = event_loop.create_window(attrs).map_err(gfx_err)?;
        let fonts = self
            .fonts
            .take()
            .ok_or_else(|| gfx_err("window created twice"))?;
        let ui_dir = self.s.data.join("ui");
        let t = Instant::now();
        let gfx = Gfx::new(Arc::new(window), fonts, &ui_dir)?;
        let vsync = matches!(
            gfx.config.present_mode,
            wgpu::PresentMode::Fifo | wgpu::PresentMode::AutoVsync
        );
        if vsync && std::env::var("WACCA_PACE").as_deref() != Ok("0") {
            self.pacer = Pacer::new(std::time::Duration::from_secs_f64(gfx.period));
        }
        self.gfx = Some(gfx);
        log::info!(
            "graphics ready in {:.0} ms ({:.0} ms after start)",
            ms(t.elapsed()),
            ms(self.s.started.elapsed())
        );
        if let Some(spec) = self.s.start.take() {
            self.start_play(spec);
        }
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
                if let Some(p) = &mut self.play {
                    let r = p.give_up();
                    self.end_play(event_loop, r, false);
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.resize(size.width, size.height);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.alt = m.state().alt_key(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state: ElementState::Pressed,
                        repeat,
                        ..
                    },
                ..
            } => {
                // Menus scroll on key repeat; play ignores it.
                if !repeat || self.screen != Screen::Play {
                    self.key(event_loop, code);
                }
            }
            WindowEvent::Touch(t) => self.touch(event_loop, t),
            WindowEvent::CursorMoved {
                position: PhysicalPosition { x, y },
                ..
            } => {
                let p = [x as f32, y as f32];
                self.cursor = Some(p);
                if self.mouse_down {
                    self.pointer(event_loop, MOUSE_ID, p, None);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(p) = self.cursor.filter(|_| self.mouse_down) {
                    self.mouse_down = false;
                    self.pointer(event_loop, MOUSE_ID, p, Some(false));
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.mouse_down = state == ElementState::Pressed;
                if let Some(p) = self.cursor {
                    self.pointer(event_loop, MOUSE_ID, p, Some(self.mouse_down));
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.wheel_pos().is_some() => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if dy.abs() >= 1.0 {
                    self.action(event_loop, Action::Scroll(-dy.round() as i32));
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(gfx) = &self.gfx else { return };
        match self.pacer.next_start().filter(|&t| t > Instant::now()) {
            Some(t) => event_loop.set_control_flow(ControlFlow::WaitUntil(t)),
            None => {
                event_loop.set_control_flow(ControlFlow::Wait);
                gfx.window.request_redraw();
            }
        }
    }
}
