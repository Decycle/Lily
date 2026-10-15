//! Asking for a Lily pack's web address (setup by URL, [`crate::setup_flow::Flow::by_url`]).
//! iOS: a native alert with a text field over the game's view (winit has no on-screen keyboard
//! support). macOS: an AppleScript dialog. Elsewhere there is no prompt: pass `--setup-url`.

/// Called once with the entered address (`None`: cancelled).
pub type Answer = Box<dyn FnOnce(Option<String>) + Send>;

#[cfg(any(target_os = "ios", target_os = "macos"))]
const TITLE: &str = "Lily pack address";
#[cfg(any(target_os = "ios", target_os = "macos"))]
const MESSAGE: &str = "The web address of a Lily pack (lily-pack.tar), for example one shared \
                       from your computer with tools/serve-pack.sh.";
#[cfg(any(target_os = "ios", target_os = "macos"))]
const EXAMPLE: &str = "http://192.168.1.20:8000/lily-pack.tar";

/// Shows the prompt; `owner` is the game's `UIView` on iOS (ignored elsewhere). Call on the UI
/// thread; returns at once and answers later.
#[cfg(target_os = "ios")]
pub fn ask(owner: isize, prefill: Option<&str>, answer: Answer) {
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::rc::Rc;

    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::Weak;
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
        UIKeyboardType, UITextAutocapitalizationType, UITextAutocorrectionType, UITextField,
        UITextInputTraits, UIView,
    };

    let Some(mtm) = MainThreadMarker::new() else {
        log::error!("address prompt: not on the main thread");
        answer(None);
        return;
    };
    // SAFETY: `owner` is the `UIView` winit created for the game window (from its raw window
    // handle); the window lives as long as the app.
    let view: Option<&UIView> = unsafe { (owner as *const UIView).as_ref() };
    // The top-most presented controller (a prompt can only be presented from it).
    let Some(mut vc) = view
        .and_then(|v| v.window())
        .and_then(|w| w.rootViewController())
    else {
        log::error!("address prompt: no view controller to present from");
        answer(None);
        return;
    };
    while let Some(top) = vc.presentedViewController() {
        vc = top;
    }

    let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
        Some(&NSString::from_str(TITLE)),
        Some(&NSString::from_str(MESSAGE)),
        UIAlertControllerStyle::Alert,
        mtm,
    );
    let prefill = prefill.map(NSString::from_str);
    let configure = RcBlock::new(move |field: NonNull<UITextField>| {
        // SAFETY: UIKit passes the alert's live text field.
        let field = unsafe { field.as_ref() };
        field.setPlaceholder(Some(&NSString::from_str(EXAMPLE)));
        field.setText(prefill.as_deref());
        field.setKeyboardType(UIKeyboardType::URL);
        field.setAutocapitalizationType(UITextAutocapitalizationType::None);
        field.setAutocorrectionType(UITextAutocorrectionType::No);
    });
    alert.addTextFieldWithConfigurationHandler(Some(&configure));

    // Whichever button is pressed first answers.
    let answer = Rc::new(RefCell::new(Some(answer)));
    let weak = Weak::from_retained(&alert);
    let ok_answer = Rc::clone(&answer);
    let ok = RcBlock::new(move |_: NonNull<UIAlertAction>| {
        let text = weak
            .load()
            .and_then(|a| a.textFields())
            .and_then(|f| f.firstObject())
            .and_then(|f| f.text())
            .map(|t| t.to_string());
        if let Some(f) = ok_answer.borrow_mut().take() {
            f(text);
        }
    });
    let cancel = RcBlock::new(move |_: NonNull<UIAlertAction>| {
        if let Some(f) = answer.borrow_mut().take() {
            f(None);
        }
    });
    let download = UIAlertAction::actionWithTitle_style_handler(
        Some(&NSString::from_str("Download")),
        UIAlertActionStyle::Default,
        Some(&ok),
        mtm,
    );
    alert.addAction(&UIAlertAction::actionWithTitle_style_handler(
        Some(&NSString::from_str("Cancel")),
        UIAlertActionStyle::Cancel,
        Some(&cancel),
        mtm,
    ));
    alert.addAction(&download);
    alert.setPreferredAction(Some(&download));
    vc.presentViewController_animated_completion(&alert, true, None);
}

/// macOS: `osascript`'s `display dialog` with an answer field, on a worker thread.
#[cfg(target_os = "macos")]
pub fn ask(_owner: isize, prefill: Option<&str>, answer: Answer) {
    // AppleScript string literal.
    let quote = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
    let script = format!(
        "text returned of (display dialog {} with title {} default answer {} \
         buttons {{\"Cancel\", \"Download\"}} default button \"Download\")",
        quote(MESSAGE),
        quote(TITLE),
        quote(prefill.unwrap_or(EXAMPLE)),
    );
    std::thread::spawn(move || {
        let out = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output();
        let text = match out {
            Ok(o) if o.status.success() => {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            }
            Ok(_) => None, // Cancel
            Err(e) => {
                log::error!("address prompt: osascript: {e}");
                None
            }
        };
        answer(text);
    });
}

#[cfg(not(any(target_os = "ios", target_os = "macos")))]
pub fn ask(_owner: isize, _prefill: Option<&str>, answer: Answer) {
    log::error!("no address prompt on this platform: start Lily with --setup-url <address>");
    answer(None);
}

/// Keeps the screen on while it lives (iOS would otherwise lock and suspend a long download).
pub struct KeepAwake(());

impl KeepAwake {
    pub fn new() -> Self {
        #[cfg(target_os = "ios")]
        crate::ios::keep_awake(true);
        KeepAwake(())
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(target_os = "ios")]
        crate::ios::keep_awake(false);
    }
}
