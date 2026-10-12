//! Starting, finding and stopping the real game without user interaction.
//!
//! [`launch`] runs the install's own `launch.bat` (amdaemon + game through segatools `inject`) detached, with stdin
//! closed so its final `pause` returns at once. The game window is found by class + title (UE4 `UnrealWindow`,
//! title "Mercury", doc 07 §2). [`stop`] kills the game, amdaemon and inject. Nothing under the install directory
//! is modified; Waccon.Server / OverlayWebview are never started.

use serde::Serialize;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct GameConfig {
    /// Directory containing `launch.bat`; used as the working directory.
    pub bin_dir: PathBuf,
    pub launch_bat: String,
    pub window_class: String,
    /// Substring the window title must contain.
    pub window_title: String,
    /// Image names killed by [`stop`].
    pub processes: Vec<String>,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            bin_dir: PathBuf::from(r"C:\Games\WACCA\bin"),
            launch_bat: "launch.bat".into(),
            window_class: "UnrealWindow".into(),
            window_title: "Mercury".into(),
            processes: [
                "Mercury-Win64-Shipping.exe",
                "amdaemon.exe",
                "inject.exe",
                "OverlayWebview.exe",
                "Waccon.Server.exe",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Debug, Error)]
pub enum GameError {
    #[error("{} not found", .0.display())]
    MissingLauncher(PathBuf),
    #[error("starting {}: {source}", path.display())]
    Spawn {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("no game window (class {class:?}, title containing {title:?}) after {waited:?}")]
    WindowTimeout {
        class: String,
        title: String,
        waited: Duration,
    },
    #[error("window query failed: {0}")]
    Window(std::io::Error),
}

/// Client area in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Whether a top-level window is the game's.
pub fn is_game_window(cfg: &GameConfig, class: &str, title: &str, visible: bool) -> bool {
    visible && class == cfg.window_class && title.contains(&cfg.window_title)
}

#[derive(Debug)]
pub struct GameWindow {
    pub hwnd: isize,
    pub title: String,
    pub rect: Rect,
}

/// The process started by [`launch`] (`cmd /c launch.bat`); ends by itself once the game is stopped.
#[derive(Debug)]
pub struct Launched {
    child: std::process::Child,
}

impl Launched {
    /// Waits briefly for the launcher to exit after [`stop`], killing it if it lingers.
    pub fn reap(mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Err(e) = self.child.kill() {
            log::warn!("killing launcher: {e}");
        }
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
pub fn launch(cfg: &GameConfig) -> Result<Launched, GameError> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let bat = cfg.bin_dir.join(&cfg.launch_bat);
    if !bat.is_file() {
        return Err(GameError::MissingLauncher(bat));
    }
    log::info!("launching {}", bat.display());
    let child = Command::new("cmd")
        .arg("/c")
        .arg(&bat)
        .current_dir(&cfg.bin_dir)
        // launch.bat runs `inject` from its own directory; with this set (some shells/agents do) cmd would not
        // look in the current directory and the game would never start.
        .env_remove("NoDefaultCurrentDirectoryInExePath")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|source| GameError::Spawn { path: bat, source })?;
    Ok(Launched { child })
}

/// Kills the game processes (`taskkill /f /im`); missing processes are not an error. Returns those killed.
#[cfg(windows)]
pub fn stop(cfg: &GameConfig) -> Vec<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut killed = Vec::new();
    for name in &cfg.processes {
        let out = std::process::Command::new("taskkill")
            .args(["/f", "/im", name])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match out {
            Ok(o) if o.status.success() => killed.push(name.clone()),
            Ok(_) => log::debug!("{name} not running"),
            Err(e) => log::warn!("taskkill {name}: {e}"),
        }
    }
    log::info!("stopped (killed): {killed:?}");
    killed
}

/// Polls for the game window until `timeout`.
#[cfg(windows)]
pub fn wait_for_window(cfg: &GameConfig, timeout: Duration) -> Result<GameWindow, GameError> {
    let start = Instant::now();
    loop {
        if let Some(w) = find_window(cfg)? {
            log::info!(
                "game window {:?} after {:?}: {:?}",
                w.title,
                start.elapsed(),
                w.rect
            );
            return Ok(w);
        }
        if start.elapsed() >= timeout {
            return Err(GameError::WindowTimeout {
                class: cfg.window_class.clone(),
                title: cfg.window_title.clone(),
                waited: timeout,
            });
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(windows)]
mod ffi {
    #[repr(C)]
    #[derive(Default)]
    pub struct WinRect {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }
    #[repr(C)]
    #[derive(Default)]
    pub struct WinPoint {
        pub x: i32,
        pub y: i32,
    }
    pub type EnumProc = unsafe extern "system" fn(hwnd: isize, lparam: isize) -> i32;
    pub const HWND_TOPMOST: isize = -1;
    pub const SWP_NOSIZE: u32 = 0x1;
    pub const SWP_NOMOVE: u32 = 0x2;
    pub const SWP_NOACTIVATE: u32 = 0x10;
    /// `DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2`.
    pub const PER_MONITOR_AWARE_V2: isize = -4;
    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn EnumWindows(cb: EnumProc, lparam: isize) -> i32;
        pub fn IsWindowVisible(hwnd: isize) -> i32;
        pub fn GetWindowTextW(hwnd: isize, buf: *mut u16, len: i32) -> i32;
        pub fn GetClassNameW(hwnd: isize, buf: *mut u16, len: i32) -> i32;
        pub fn GetClientRect(hwnd: isize, rect: *mut WinRect) -> i32;
        pub fn ClientToScreen(hwnd: isize, point: *mut WinPoint) -> i32;
        pub fn SetWindowPos(
            hwnd: isize,
            after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        pub fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
    }
}

/// Makes window/rect queries return physical pixels (the coordinates ffmpeg captures). Call once at start-up.
#[cfg(windows)]
pub fn set_dpi_aware() {
    // SAFETY: plain Win32 call; fails harmlessly if awareness was already set.
    if unsafe { ffi::SetProcessDpiAwarenessContext(ffi::PER_MONITOR_AWARE_V2) } == 0 {
        log::debug!(
            "SetProcessDpiAwarenessContext: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Keeps the game above other windows so desktop capture sees it (cleared when the game exits).
#[cfg(windows)]
pub fn make_topmost(w: &GameWindow) {
    let flags = ffi::SWP_NOMOVE | ffi::SWP_NOSIZE | ffi::SWP_NOACTIVATE;
    // SAFETY: plain Win32 call on a window handle; failure only logged.
    if unsafe { ffi::SetWindowPos(w.hwnd, ffi::HWND_TOPMOST, 0, 0, 0, 0, flags) } == 0 {
        log::warn!("SetWindowPos: {}", std::io::Error::last_os_error());
    }
}

#[cfg(windows)]
fn wide_text(hwnd: isize, get: unsafe extern "system" fn(isize, *mut u16, i32) -> i32) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: buffer and its length match.
    let n = unsafe { get(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.clamp(0, 256) as usize])
}

/// The first visible top-level window matching [`is_game_window`], with its client rect in screen coordinates.
#[cfg(windows)]
pub fn find_window(cfg: &GameConfig) -> Result<Option<GameWindow>, GameError> {
    unsafe extern "system" fn collect(hwnd: isize, lparam: isize) -> i32 {
        // SAFETY: lparam is the &mut Vec passed to EnumWindows below, alive for the call.
        let out = unsafe { &mut *(lparam as *mut Vec<isize>) };
        out.push(hwnd);
        1
    }
    let mut hwnds: Vec<isize> = Vec::new();
    // SAFETY: the callback only pushes into the Vec behind lparam.
    if unsafe { ffi::EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize) } == 0 {
        return Err(GameError::Window(std::io::Error::last_os_error()));
    }
    for hwnd in hwnds {
        // SAFETY: plain query on a handle EnumWindows returned.
        let visible = unsafe { ffi::IsWindowVisible(hwnd) } != 0;
        let class = wide_text(hwnd, ffi::GetClassNameW);
        let title = wide_text(hwnd, ffi::GetWindowTextW);
        if !is_game_window(cfg, &class, &title, visible) {
            continue;
        }
        let mut r = ffi::WinRect::default();
        let mut p = ffi::WinPoint::default();
        // SAFETY: out-pointers to live locals.
        let ok = unsafe {
            ffi::GetClientRect(hwnd, &mut r) != 0 && ffi::ClientToScreen(hwnd, &mut p) != 0
        };
        if !ok {
            return Err(GameError::Window(std::io::Error::last_os_error()));
        }
        let rect = Rect {
            x: p.x,
            y: p.y,
            width: (r.right - r.left).max(0) as u32,
            height: (r.bottom - r.top).max(0) as u32,
        };
        return Ok(Some(GameWindow { hwnd, title, rect }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_list_covers_every_harness_process() {
        let p = GameConfig::default().processes;
        for name in [
            "Mercury-Win64-Shipping.exe",
            "amdaemon.exe",
            "inject.exe",
            "OverlayWebview.exe",
            "Waccon.Server.exe",
        ] {
            assert!(p.iter().any(|n| n == name), "{name}");
        }
    }

    #[test]
    fn game_window_matching() {
        let cfg = GameConfig::default();
        assert!(is_game_window(&cfg, "UnrealWindow", "Mercury", true));
        assert!(is_game_window(
            &cfg,
            "UnrealWindow",
            "Mercury (64-bit)",
            true
        ));
        assert!(!is_game_window(&cfg, "UnrealWindow", "Mercury", false));
        // An Explorer window opened on WindowsNoEditor\Mercury has the same title.
        assert!(!is_game_window(&cfg, "CabinetWClass", "Mercury", true));
        assert!(!is_game_window(&cfg, "UnrealWindow", "Other", true));
    }

    #[cfg(windows)]
    #[test]
    fn missing_launcher_is_reported() {
        let cfg = GameConfig {
            bin_dir: std::env::temp_dir().join("wacca-realgame-no-such-dir"),
            ..GameConfig::default()
        };
        assert!(matches!(launch(&cfg), Err(GameError::MissingLauncher(_))));
    }

    #[cfg(windows)]
    #[test]
    fn find_window_with_unmatched_title_is_none() {
        let cfg = GameConfig {
            window_title: "wacca-realgame no such window \u{1F600}".into(),
            ..GameConfig::default()
        };
        assert!(find_window(&cfg).unwrap().is_none());
    }
}
