//! macOS: the built-in trackpad as a small touch ring. The private MultitouchSupport framework
//! (what tools like BetterTouchTool use) reports every finger with its position on the surface;
//! it is opened at run time with `dlopen`, so nothing links against it and other platforms are
//! unaffected. The shell maps the fingers onto the ring during play
//! ([`crate::touch_mapping::trackpad_to_playfield`]) and locks the cursor meanwhile.
//!
//! `LILY_TRACKPAD_DEBUG=1` logs the contacts (to tune the palm filter).

/// One finger on the trackpad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// Stable while the finger stays down.
    pub id: i32,
    /// Position on the surface, 0..1, x to the right, y up.
    pub pos: [f32; 2],
}

/// Contact size (MultitouchSupport's `size`, roughly the finger's area) above which a contact
/// is taken for a palm or a resting thumb and ignored.
// UNVERIFIED: a fingertip measured ~0.4 on a 2026 MacBook; palms read well above.
#[cfg(target_os = "macos")]
const PALM_SIZE: f32 = 2.5;

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_char, c_int, c_void};
    use std::sync::{Mutex, OnceLock};

    use super::{Contact, PALM_SIZE};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f32,
        y: f32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Readout {
        pos: Point,
        vel: Point,
    }

    /// MultitouchSupport's per-finger record (layout from the community headers; 96 bytes).
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Finger {
        frame: c_int,
        timestamp: f64,
        identifier: c_int,
        state: c_int,
        foo3: c_int,
        foo4: c_int,
        normalized: Readout,
        size: f32,
        zero1: c_int,
        angle: f32,
        major_axis: f32,
        minor_axis: f32,
        mm: Readout,
        zero2: [c_int; 2],
        unk2: f32,
    }

    type Callback = extern "C" fn(*mut c_void, *const Finger, c_int, f64, c_int) -> c_int;

    /// The device, once started (`Err`: why the trackpad is unavailable).
    static DEVICE: OnceLock<Result<Device, String>> = OnceLock::new();
    static CONTACTS: Mutex<Vec<Contact>> = Mutex::new(Vec::new());

    struct Device {
        /// Width / height of the sensor surface.
        aspect: f32,
    }

    /// States of a finger that is on the surface (3 = making contact, 4 = touching); 5 and
    /// later are lifting or hovering.
    fn touching(state: c_int) -> bool {
        matches!(state, 3 | 4)
    }

    extern "C" fn on_frame(
        _device: *mut c_void,
        fingers: *const Finger,
        count: c_int,
        _timestamp: f64,
        _frame: c_int,
    ) -> c_int {
        let fingers = if fingers.is_null() || count <= 0 {
            &[][..]
        } else {
            // SAFETY: MultitouchSupport passes `count` valid records for this callback's duration.
            unsafe { std::slice::from_raw_parts(fingers, count as usize) }
        };
        let debug = std::env::var_os("LILY_TRACKPAD_DEBUG").is_some();
        let mut now = Vec::with_capacity(fingers.len());
        for f in fingers {
            if debug {
                log::info!(
                    "trackpad: id {} state {} pos ({:.3}, {:.3}) size {:.2} axes {:.1}/{:.1}",
                    f.identifier,
                    f.state,
                    f.normalized.pos.x,
                    f.normalized.pos.y,
                    f.size,
                    f.major_axis,
                    f.minor_axis
                );
            }
            if touching(f.state) && f.size < PALM_SIZE {
                now.push(Contact {
                    id: f.identifier,
                    pos: [f.normalized.pos.x, f.normalized.pos.y],
                });
            }
        }
        if let Ok(mut c) = CONTACTS.lock() {
            *c = now;
        }
        0
    }

    fn open() -> Result<Device, String> {
        const PATH: &[u8] =
            b"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport\0";
        // SAFETY: dlopen/dlsym with NUL-terminated names; the symbols are cast to the signatures
        // MultitouchSupport has had since macOS 10.5 (community headers).
        unsafe {
            let lib = libc::dlopen(PATH.as_ptr().cast(), libc::RTLD_NOW);
            if lib.is_null() {
                return Err("MultitouchSupport is not available".into());
            }
            let sym = |name: &[u8]| {
                let p = libc::dlsym(lib, name.as_ptr().cast::<c_char>());
                (!p.is_null()).then_some(p)
            };
            let create = sym(b"MTDeviceCreateDefault\0").ok_or("MTDeviceCreateDefault missing")?;
            let register = sym(b"MTRegisterContactFrameCallback\0")
                .ok_or("MTRegisterContactFrameCallback missing")?;
            let start = sym(b"MTDeviceStart\0").ok_or("MTDeviceStart missing")?;
            let create: extern "C" fn() -> *mut c_void = std::mem::transmute(create);
            let register: extern "C" fn(*mut c_void, Callback) = std::mem::transmute(register);
            let start: extern "C" fn(*mut c_void, c_int) -> c_int = std::mem::transmute(start);
            let device = create();
            if device.is_null() {
                return Err("no trackpad found".into());
            }
            // Surface size in hundredths of a millimetre (absent on some versions: assume 16:10).
            let mut aspect = 1.6;
            if let Some(dims) = sym(b"MTDeviceGetSensorSurfaceDimensions\0") {
                let dims: extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_int =
                    std::mem::transmute(dims);
                let (mut w, mut h) = (0, 0);
                dims(device, &mut w, &mut h);
                if w > 0 && h > 0 {
                    aspect = w as f32 / h as f32;
                    log::info!(
                        "trackpad: {:.1} × {:.1} mm",
                        w as f32 / 100.0,
                        h as f32 / 100.0
                    );
                }
            }
            register(device, on_frame);
            start(device, 0);
            // The device lives for the rest of the process (never stopped or released).
            Ok(Device { aspect })
        }
    }

    pub fn start() -> Result<(), String> {
        match DEVICE.get_or_init(|| {
            let d = open();
            if let Err(e) = &d {
                log::warn!("trackpad: {e}");
            }
            d
        }) {
            Ok(_) => Ok(()),
            Err(e) => Err(e.clone()),
        }
    }

    pub fn contacts() -> Vec<Contact> {
        CONTACTS.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn aspect() -> f32 {
        match DEVICE.get() {
            Some(Ok(d)) => d.aspect,
            _ => 1.6,
        }
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayHideCursor(display: u32) -> i32;
        fn CGDisplayShowCursor(display: u32) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    }

    static CAPTURED: Mutex<bool> = Mutex::new(false);

    pub fn capture_cursor(on: bool) {
        let Ok(mut captured) = CAPTURED.lock() else {
            return;
        };
        if *captured == on {
            return;
        }
        // SAFETY: plain CoreGraphics calls; hide/show are counted, so they are kept paired.
        unsafe {
            let display = CGMainDisplayID();
            if on {
                CGDisplayHideCursor(display);
            } else {
                CGDisplayShowCursor(display);
            }
            CGAssociateMouseAndMouseCursorPosition(u32::from(!on));
        }
        *captured = on;
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::Contact;

    pub fn start() -> Result<(), String> {
        Err("the trackpad mode is only available on macOS".into())
    }

    pub fn contacts() -> Vec<Contact> {
        Vec::new()
    }

    pub fn aspect() -> f32 {
        1.6
    }

    pub fn capture_cursor(_on: bool) {}
}

/// Starts listening to the trackpad (once; later calls only report the result).
pub fn start() -> Result<(), String> {
    imp::start()
}

/// The fingers on the trackpad now (palms left out).
pub fn contacts() -> Vec<Contact> {
    imp::contacts()
}

/// Width / height of the trackpad surface.
pub fn aspect() -> f32 {
    imp::aspect()
}

/// Hides the cursor and stops the trackpad from moving it (`false`: shows and frees it again).
/// macOS' own calls: winit's cursor hiding only applies once the cursor moves over the window,
/// and a locked cursor does not move.
pub fn capture_cursor(on: bool) {
    imp::capture_cursor(on);
}
