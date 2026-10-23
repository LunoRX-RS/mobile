//! Thin Rust handle over the Swift (iOS) and Kotlin (Android) plugin classes. Mobile targets only; the app registers
//! it under `cfg(mobile)`. Commands are listed in `ios/Sources/LunorNativePlugin.swift` and
//! `android/.../LunorNativePlugin.kt`; both implement the same JSON contract.
use serde::{Serialize, de::DeserializeOwned};
use tauri::{
    Manager, Runtime,
    plugin::{Builder, PluginHandle, TauriPlugin},
};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
compile_error!(
    "tauri-plugin-lunor-native supports iOS and Android only; depend on it under cfg(mobile)."
);

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_lunor_native);

/// Category only: native error text may name paths or URLs, so it is dropped here.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("native plugin call failed")]
    Failed,
}

pub struct LunorNative<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> LunorNative<R> {
    /// Blocking: the call waits for the platform answer. Run it from `spawn_blocking`, never on the main thread.
    pub fn call<T: DeserializeOwned>(
        &self,
        command: &str,
        payload: impl Serialize,
    ) -> Result<T, Error> {
        self.0
            .run_mobile_plugin(command, payload)
            .map_err(|_| Error::Failed)
    }
}

pub trait LunorNativeExt<R: Runtime> {
    fn lunor_native(&self) -> &LunorNative<R>;
}

impl<R: Runtime, T: Manager<R>> LunorNativeExt<R> for T {
    fn lunor_native(&self) -> &LunorNative<R> {
        self.state::<LunorNative<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("lunor-native")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle =
                api.register_android_plugin("io.lunorx.nativeplugin", "LunorNativePlugin")?;
            #[cfg(target_os = "ios")]
            let handle = api.register_ios_plugin(init_plugin_lunor_native)?;
            app.manage(LunorNative(handle));
            Ok(())
        })
        .build()
}
