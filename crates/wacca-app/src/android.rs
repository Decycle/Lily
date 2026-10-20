//! Android specifics: the activity handle, where the game files live, the address prompt (a
//! native dialog in `LilyActivity.java`, called over JNI) and keeping the screen on.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use jni::JNIEnv;
use jni::objects::{JClass, JObject, JString, JValue};
use winit::platform::android::activity::AndroidApp;

use crate::url_prompt::Answer;

static APP: OnceLock<AndroidApp> = OnceLock::new();
/// The address prompt's answer, waiting for `LilyActivity.nativeUrl`.
static PENDING: Mutex<Option<Answer>> = Mutex::new(None);

/// Process start (from `android_main`).
pub fn init(app: AndroidApp) {
    let _ = APP.set(app);
}

pub fn app() -> Option<AndroidApp> {
    APP.get().cloned()
}

/// The app's external files folder (`Android/data/<package>/files`): `assets/`, `data/` and
/// `save/` go here. It can be reached over USB, and it survives app updates.
pub fn data_dir() -> Option<PathBuf> {
    let app = app()?;
    app.external_data_path()
        .or_else(|| app.internal_data_path())
}

/// Turns the screen's auto-off off or back on, through `LilyActivity.keepScreenOn` (which sets
/// `FLAG_KEEP_SCREEN_ON` on the UI thread). Not `AndroidApp::set_window_flags`: called from a
/// worker thread, that blocked on android-activity's lock while the UI thread waited on it
/// (Android then reported the app as not responding).
pub fn keep_awake(on: bool) {
    if let Err(e) = call_activity("keepScreenOn", "(Z)V", &[JValue::Bool(on.into())]) {
        log::warn!("keep screen on: {e}");
    }
}

/// Shows `LilyActivity.askUrl` (an AlertDialog with a text field); `answer` is called from
/// [`Java_app_lily_LilyActivity_nativeUrl`].
pub fn ask_url(prefill: Option<&str>, answer: Answer) {
    if let Ok(mut p) = PENDING.lock() {
        // A prompt that never answered: give up on it.
        if let Some(old) = p.replace(answer) {
            old(None);
        }
    }
    if let Err(e) = call_ask_url(prefill.unwrap_or("")) {
        log::error!("address prompt: {e}");
        if let Some(a) = PENDING.lock().ok().and_then(|mut p| p.take()) {
            a(None);
        }
    }
}

fn call_ask_url(prefill: &str) -> Result<(), Box<dyn std::error::Error>> {
    with_activity(|env, activity| {
        let text = env.new_string(prefill)?;
        env.call_method(
            activity,
            "askUrl",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&text)],
        )?;
        Ok(())
    })
}

/// Calls a `void` method of the activity with plain arguments (from any thread).
fn call_activity(name: &str, sig: &str, args: &[JValue]) -> Result<(), Box<dyn std::error::Error>> {
    with_activity(|env, activity| {
        env.call_method(activity, name, sig, args)?;
        Ok(())
    })
}

/// Runs `f` with a JNI environment for this thread and the activity object.
fn with_activity(
    f: impl FnOnce(&mut JNIEnv, &JObject) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = app().ok_or("no activity")?;
    // SAFETY: the pointers are the process's JavaVM and the activity object, valid while the
    // app runs (android-activity keeps a global reference to the activity).
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast())? };
    let mut env = vm.attach_current_thread()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
    f(&mut env, &activity)
}

/// `LilyActivity.nativeUrl(String)`: the dialog closed with this address (`null`: cancelled).
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_lily_LilyActivity_nativeUrl(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) {
    let text = if url.is_null() {
        None
    } else {
        env.get_string(&url).ok().map(String::from)
    };
    if let Some(answer) = PENDING.lock().ok().and_then(|mut p| p.take()) {
        answer(text);
    }
}
