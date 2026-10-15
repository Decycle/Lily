//! iOS specifics: where the game files live, the audio session, keeping the screen on.

use std::path::PathBuf;

/// The app's `Documents` folder: `assets/`, `data/` and `save/` go here (the Files app shows it,
/// and it survives app updates).
pub fn documents() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents"))
}

/// Process start: a playback audio session (plays with the silent switch on, like a game should)
/// with a short I/O buffer for low latency.
pub fn start() {
    use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayback};
    // SAFETY: plain AVAudioSession calls with valid arguments; the category constant is an
    // AVFAudio static that exists on every supported iOS.
    unsafe {
        let session = AVAudioSession::sharedInstance();
        if let Some(cat) = AVAudioSessionCategoryPlayback
            && let Err(e) = session.setCategory_error(cat)
        {
            log::warn!("audio session category: {e:?}");
        }
        // UNVERIFIED: 5 ms requested; iOS rounds it to what the hardware allows.
        if let Err(e) = session.setPreferredIOBufferDuration_error(0.005) {
            log::warn!("audio buffer duration: {e:?}");
        }
        if let Err(e) = session.setActive_error(true) {
            log::warn!("audio session: {e:?}");
        }
    }
}

/// Turns the idle timer (auto-lock) off or back on. Safe to call from any thread: the change is
/// made on the main thread.
pub fn keep_awake(on: bool) {
    use objc2::MainThreadMarker;
    use objc2_ui_kit::UIApplication;
    let apply = move |mtm: MainThreadMarker| {
        UIApplication::sharedApplication(mtm).setIdleTimerDisabled(on);
    };
    match MainThreadMarker::new() {
        Some(mtm) => apply(mtm),
        None => dispatch_main(move || {
            if let Some(mtm) = MainThreadMarker::new() {
                apply(mtm);
            }
        }),
    }
}

/// Runs `f` on the main thread (libdispatch's main queue).
fn dispatch_main(f: impl FnOnce() + Send + 'static) {
    use std::ffi::c_void;
    #[repr(C)]
    struct Queue(c_void);
    unsafe extern "C" {
        static _dispatch_main_q: Queue;
        fn dispatch_async_f(
            queue: *const Queue,
            context: *mut c_void,
            work: unsafe extern "C" fn(*mut c_void),
        );
    }
    unsafe extern "C" fn trampoline(ctx: *mut c_void) {
        // SAFETY: `ctx` is the box leaked below, run exactly once.
        let f = unsafe { Box::from_raw(ctx.cast::<Box<dyn FnOnce()>>()) };
        f();
    }
    let boxed: Box<Box<dyn FnOnce()>> = Box::new(Box::new(f));
    // SAFETY: `_dispatch_main_q` is libdispatch's main queue; the context is a leaked box that
    // the trampoline frees.
    unsafe {
        dispatch_async_f(
            &raw const _dispatch_main_q,
            Box::into_raw(boxed).cast(),
            trampoline,
        );
    }
}
