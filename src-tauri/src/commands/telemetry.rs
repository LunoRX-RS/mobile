//! Opt-in anonymous usage counters. Thin commands over `core::telemetry`; the upload carries counter names and counts only.
use crate::core::library::LibraryError;
use crate::core::telemetry::{Telemetry, TelemetryView};
use crate::sync::api::{ApiClient, TelemetryBatch, TelemetryCounter};
use crate::sync::engine::{ErrorCategory, category};
use crate::sync::settings::default_base_url;
use tauri::{State, WebviewWindow};

pub struct TelemetryState {
    inner: std::sync::Mutex<Result<Telemetry, LibraryError>>,
    path: Result<std::path::PathBuf, LibraryError>,
}

impl TelemetryState {
    pub fn open(path: Result<std::path::PathBuf, LibraryError>) -> Self {
        let inner = path
            .as_ref()
            .map_err(|e| *e)
            .and_then(|p| Telemetry::open(p.clone()));
        if let Err(kind) = &inner {
            eprintln!("telemetry unavailable: {kind:?}");
        }
        Self {
            inner: std::sync::Mutex::new(inner),
            path,
        }
    }

    pub(crate) fn with<T>(
        &self,
        action: impl FnOnce(&Telemetry) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        let mut inner = self.inner.lock().map_err(|_| LibraryError::Storage)?;
        if inner.is_err() {
            *inner = self
                .path
                .as_ref()
                .map_err(|e| *e)
                .and_then(|p| Telemetry::open(p.clone()));
        }
        action(inner.as_ref().map_err(|e| *e)?)
    }

    /// For Rust-side events such as app start. Never fails the caller.
    pub fn record(&self, name: &str) {
        let _ = self.with(|t| t.record(name));
    }
}

fn main(window: &WebviewWindow) -> Result<(), LibraryError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(LibraryError::Validation)
    }
}

fn os() -> &'static str {
    match std::env::consts::OS {
        o @ ("macos" | "windows" | "linux") => o,
        _ => "other",
    }
}

#[tauri::command]
pub fn telemetry_get(
    window: WebviewWindow,
    state: State<'_, TelemetryState>,
) -> Result<TelemetryView, LibraryError> {
    main(&window)?;
    state.with(Telemetry::view)
}

#[tauri::command]
pub fn telemetry_set(
    window: WebviewWindow,
    state: State<'_, TelemetryState>,
    enabled: bool,
) -> Result<TelemetryView, LibraryError> {
    main(&window)?;
    state.with(|t| t.set_enabled(enabled))
}

/// `name` is validated against the server's rules; anything else is refused.
#[tauri::command]
pub fn telemetry_record(
    window: WebviewWindow,
    state: State<'_, TelemetryState>,
    name: String,
) -> Result<(), LibraryError> {
    main(&window)?;
    state.with(|t| t.record(&name))
}

/// Uploads pending counters. Nothing is sent while telemetry is off or nothing is pending.
#[tauri::command]
pub async fn telemetry_flush(
    window: WebviewWindow,
    state: State<'_, TelemetryState>,
) -> Result<(), ErrorCategory> {
    main(&window).map_err(|_| ErrorCategory::Validation)?;
    let Some((install_id, counters)) = state
        .with(Telemetry::take)
        .map_err(|_| ErrorCategory::Storage)?
    else {
        return Ok(());
    };
    let sent = async {
        let api = ApiClient::new(&default_base_url())?;
        api.post_telemetry(&TelemetryBatch {
            install_id: &install_id,
            app_version: env!("CARGO_PKG_VERSION"),
            os: os(),
            arch: std::env::consts::ARCH,
            counters: counters
                .iter()
                .map(|(name, count)| TelemetryCounter {
                    name,
                    count: *count,
                })
                .collect(),
        })
        .await
    }
    .await;
    if let Err(e) = sent {
        let _ = state.with(|t| t.restore(counters));
        return Err(category(&e));
    }
    Ok(())
}

/// Asks the server to delete everything it holds for this install, then turns telemetry off with a new anonymous id.
#[tauri::command]
pub async fn telemetry_delete(
    window: WebviewWindow,
    state: State<'_, TelemetryState>,
) -> Result<TelemetryView, ErrorCategory> {
    main(&window).map_err(|_| ErrorCategory::Validation)?;
    let view = state
        .with(Telemetry::view)
        .map_err(|_| ErrorCategory::Storage)?;
    ApiClient::new(&default_base_url())
        .map_err(|e| category(&e))?
        .delete_telemetry(&view.install_id)
        .await
        .map_err(|e| category(&e))?;
    state
        .with(Telemetry::reset)
        .map_err(|_| ErrorCategory::Storage)
}
