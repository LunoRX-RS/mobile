// No webview commands: the plugin is called from Rust only, so the webview cannot reach native audio or secrets.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .ios_path("ios")
        .build();
}
