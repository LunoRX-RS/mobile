//! Hands the Android JVM to `rustls-platform-verifier`. innerlight's HTTP client (reqwest 0.13) verifies certificates
//! through the system trust store and panics on the first HTTPS request until this has run, which broke playback,
//! sign-in and the YouTube import. The Kotlin half comes from the Gradle dependency in the lunor-native plugin.
use jni::{EnvUnowned, objects::JObject};
use tauri::{AppHandle, Manager, Wry};

/// Call once from `setup`, before any network request.
pub fn init(app: &AppHandle<Wry>) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("tls init skipped: no main window");
        return;
    };
    let queued = window.with_webview(|webview| {
        webview.jni_handle().exec(|env, activity, _webview| {
            // The Application context outlives the activity, so the verifier never pins a destroyed one.
            let context = match env
                .call_method(
                    activity,
                    "getApplicationContext",
                    "()Landroid/content/Context;",
                    &[],
                )
                .and_then(|v| v.l())
            {
                Ok(context) => context,
                Err(_) => {
                    eprintln!("tls init failed: no application context");
                    return;
                }
            };
            // tauri uses jni 0.21 and the verifier jni 0.22: both wrap the same raw JNI pointers.
            let raw_context = context.as_raw() as jni::sys::jobject;
            let outcome = unsafe {
                let mut unowned = EnvUnowned::from_raw(env.get_raw() as *mut jni::sys::JNIEnv);
                unowned
                    .with_env(|env| {
                        let context = JObject::from_raw(env, raw_context);
                        rustls_platform_verifier::android::init_with_env(env, context)
                    })
                    .into_outcome()
            };
            if !matches!(outcome, jni::Outcome::Ok(_)) {
                eprintln!("tls init failed: the verifier rejected the JVM handles");
            }
        });
    });
    if queued.is_err() {
        eprintln!("tls init failed: could not reach the webview");
    }
}
