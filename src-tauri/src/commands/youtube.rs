//! App-owned Google sign-in window. No remote IPC grants, password interception or OAuth flow.
use crate::{commands::providers::KeyringKeyStore, core::yt::Yt, sync::credentials::TokenStore};
use serde::Serialize;
use std::{sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, webview::Cookie};
#[cfg(desktop)]
use tauri::{
    WebviewUrl, WebviewWindowBuilder, WindowEvent,
    webview::{NewWindowResponse, PageLoadEvent},
};

#[cfg(desktop)]
const LOGIN_WINDOW: &str = "youtube-sign-in";
const EVENT: &str = "core://youtube/connection";
/// The embedded WebKit view sends no `Safari/` token, so YouTube Music answers "your browser is not supported" and the
/// sign-in never finishes. The window presents the same engine as the regular browser for the platform instead.
/// WebView2 on Windows already identifies as a current browser and keeps its own value.
#[cfg(all(desktop, target_os = "macos"))]
const LOGIN_USER_AGENT: Option<&str> = Some(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
);
#[cfg(all(desktop, target_os = "linux"))]
const LOGIN_USER_AGENT: Option<&str> = Some(
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15",
);
#[cfg(all(desktop, not(any(target_os = "macos", target_os = "linux"))))]
const LOGIN_USER_AGENT: Option<&str> = None;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionStatus {
    Restoring,
    Disconnected,
    Connecting,
    Connected,
    Expired,
    StoreError,
    Unavailable,
}

pub struct YouTubeState {
    pub yt: Option<Yt>,
    store: KeyringKeyStore,
    status: Mutex<ConnectionStatus>,
    operation: tokio::sync::Mutex<()>,
}
impl YouTubeState {
    pub fn new(yt: Option<Yt>) -> Self {
        let status = if yt.is_some() {
            ConnectionStatus::Restoring
        } else {
            ConnectionStatus::Unavailable
        };
        Self {
            yt,
            store: KeyringKeyStore::youtube_session(),
            status: Mutex::new(status),
            operation: tokio::sync::Mutex::new(()),
        }
    }
    pub fn connected(&self) -> bool {
        self.status() == ConnectionStatus::Connected
    }
    fn status(&self) -> ConnectionStatus {
        *self.status.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn idle_status(&self) -> ConnectionStatus {
        if self.yt.as_ref().is_some_and(Yt::signed_in) {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        }
    }
    fn publish(&self, app: &AppHandle, status: ConnectionStatus) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
        let _ = app.emit_to("main", EVENT, status);
    }
    pub async fn restore(app: AppHandle) {
        let state = app.state::<Self>();
        let _guard = state.operation.lock().await;
        let Some(yt) = &state.yt else {
            return;
        };
        let status = match state.store.get() {
            Ok(Some(cookie)) => {
                match tokio::time::timeout(Duration::from_secs(20), yt.connect(&cookie)).await {
                    Ok(Ok(())) => ConnectionStatus::Connected,
                    _ => ConnectionStatus::Expired,
                }
            }
            Ok(None) => ConnectionStatus::Disconnected,
            Err(_) => ConnectionStatus::StoreError,
        };
        state.publish(&app, status);
        if status == ConnectionStatus::Connected {
            let importing = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::commands::youtube_import::run_auto(&importing, false).await;
            });
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "category", rename_all = "camelCase")]
pub enum YouTubeError {
    Unavailable,
    WindowFailed,
    SignInIncomplete,
    SessionRejected,
    StoreAccess,
    Forbidden,
}

fn main_only(window: &WebviewWindow) -> Result<(), YouTubeError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(YouTubeError::Forbidden)
    }
}
/// YouTube answers a phone user agent with the mobile site at `m.youtube.com`, and every other `*.youtube.com` host
/// belongs to the same service. The leading dot in the suffix check keeps a lookalike such as `evil-youtube.com` out.
#[cfg(desktop)]
fn youtube_host(host: &str) -> bool {
    host == "youtube.com" || host.ends_with(".youtube.com")
}
/// Google runs sign-in across its own host family: `google.com` and subdomains such as `accounts.google.com`,
/// `consent.google.com` and `ogs.google.com`, plus the account's country domain in the forms `google.<cc>`,
/// `google.co.<cc>` and `google.com.<cc>` with a two-letter country code. A lookalike such as `notgoogle.com`,
/// `google.co.th.evil.test` or `google.com.evil` does not pass.
#[cfg(desktop)]
fn google_host(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    let Some(index) = labels.iter().position(|label| *label == "google") else {
        return false;
    };
    let code = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_lowercase());
    match &labels[index + 1..] {
        ["com"] => true,
        [cc] => code(cc),
        ["co" | "com", cc] => code(cc),
        _ => false,
    }
}
#[cfg(desktop)]
fn youtube_origin(url: &tauri::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.host_str().is_some_and(youtube_host)
}
#[cfg(desktop)]
fn login_navigation(url: &tauri::Url) -> bool {
    // The sign-in page opens blank frames while it loads; they carry no remote content.
    if url.as_str() == "about:blank" {
        return true;
    }
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url
            .host_str()
            .is_some_and(|host| youtube_host(host) || google_host(host))
}

fn session_cookie(cookies: &[Cookie<'_>]) -> Option<String> {
    let allowed = [
        "SAPISID",
        "APISID",
        "SID",
        "HSID",
        "SSID",
        "__Secure-1PAPISID",
        "__Secure-3PAPISID",
        "__Secure-1PSID",
        "__Secure-3PSID",
        "__Secure-1PSIDTS",
        "__Secure-3PSIDTS",
        "LOGIN_INFO",
    ];
    // Google's sign-in can leave the session cookies on `google.com` with none on `youtube.com`, so both count. A
    // `youtube.com` cookie wins over a `google.com` one of the same name.
    let rank = |domain: &str| match domain {
        "youtube.com" | "www.youtube.com" | "music.youtube.com" => Some(0u8),
        "google.com" | "www.google.com" => Some(1),
        _ => None,
    };
    let mut ranked: Vec<(u8, &Cookie<'_>)> = cookies
        .iter()
        .filter_map(|c| rank(c.domain().unwrap_or("").trim_start_matches('.')).map(|r| (r, c)))
        .collect();
    ranked.sort_by_key(|(r, _)| std::cmp::Reverse(*r));
    let mut selected = std::collections::BTreeMap::new();
    for (_, cookie) in ranked {
        if !allowed.contains(&cookie.name()) {
            continue;
        }
        let value = cookie.value();
        if value.is_empty()
            || value.len() > 8192
            || !value
                .bytes()
                .all(|b| (0x21..=0x7e).contains(&b) && b != b';')
        {
            continue;
        }
        selected.insert(cookie.name(), value);
    }
    if !["SAPISID", "__Secure-1PAPISID", "__Secure-3PAPISID"]
        .iter()
        .any(|key| selected.contains_key(key))
    {
        return None;
    }
    let header = selected
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    (header.len() <= 32_768).then_some(header)
}

#[cfg(desktop)]
async fn capture(app: &AppHandle) -> Result<ConnectionStatus, YouTubeError> {
    let state = app.state::<YouTubeState>();
    let _guard = state.operation.lock().await;
    if state.status() == ConnectionStatus::Connected {
        return Ok(ConnectionStatus::Connected);
    }
    if state.status() != ConnectionStatus::Connecting {
        return Err(YouTubeError::SignInIncomplete);
    }
    let window = app
        .get_webview_window(LOGIN_WINDOW)
        .ok_or(YouTubeError::SignInIncomplete)?;
    if !youtube_origin(&window.url().map_err(|_| YouTubeError::WindowFailed)?) {
        return Err(YouTubeError::SignInIncomplete);
    }
    // Not `cookies_for_url`: on macOS it keeps only cookies whose domain equals the host exactly, which drops every
    // `.youtube.com` session cookie. `session_cookie` does its own domain and name filtering.
    let cookies = window.cookies().map_err(|_| YouTubeError::WindowFailed)?;
    // Debug builds only: cookie names and domains, never values, to see why a finished sign-in selects nothing.
    #[cfg(debug_assertions)]
    {
        let seen: Vec<String> = cookies
            .iter()
            .map(|c| {
                format!(
                    "{}@{}{}",
                    c.name(),
                    c.domain().unwrap_or("-"),
                    if c.value().is_empty() { "(empty)" } else { "" }
                )
            })
            .collect();
        eprintln!(
            "[youtube sign-in] window host {:?}, {} cookies: {}",
            window
                .url()
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned)),
            seen.len(),
            seen.join(", ")
        );
    }
    let cookie = session_cookie(&cookies).ok_or(YouTubeError::SignInIncomplete)?;
    let yt = state.yt.as_ref().ok_or(YouTubeError::Unavailable)?;
    if !matches!(
        tokio::time::timeout(Duration::from_secs(20), yt.connect(&cookie)).await,
        Ok(Ok(()))
    ) {
        return Err(YouTubeError::SessionRejected);
    }
    if state.store.set(&cookie).is_err() {
        yt.clear_session();
        state.publish(app, ConnectionStatus::StoreError);
        return Err(YouTubeError::StoreAccess);
    }
    state.publish(app, ConnectionStatus::Connected);
    let _ = window.close();
    // A new sign-in brings the collection over straight away.
    let importing = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::commands::youtube_import::run_auto(&importing, true).await;
    });
    Ok(ConnectionStatus::Connected)
}

#[cfg(desktop)]
/// The session cookies land a moment after the first YouTube Music page load, and that page is a single-page app that
/// fires no further load events. So after a load on a YouTube origin, retry for a few seconds until the cookies are
/// there. One retry loop runs at a time, and it stops as soon as the window is gone or the answer is final.
#[cfg(desktop)]
async fn capture_when_ready(app: &AppHandle) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    for attempt in 0..10 {
        if app.get_webview_window(LOGIN_WINDOW).is_none() {
            break;
        }
        match capture(app).await {
            Err(YouTubeError::SignInIncomplete) if attempt < 9 => {
                tokio::time::sleep(Duration::from_millis(1500)).await;
            }
            _ => break,
        }
    }
    RUNNING.store(false, std::sync::atomic::Ordering::Release);
}

/// Mobile: Tauri cannot open a second window on iOS or Android, so the lunor-native plugin shows a full-screen native
/// web view (non-persistent cookie storage, the same host allowlist as the desktop window, popups denied) and returns
/// the cookies once the user reaches YouTube. An empty list means the user cancelled: not an error. The caller holds
/// the `operation` lock.
#[cfg(mobile)]
async fn sign_in_window(
    app: &AppHandle,
    state: &YouTubeState,
) -> Result<ConnectionStatus, YouTubeError> {
    use tauri_plugin_lunor_native::LunorNativeExt;
    #[derive(serde::Deserialize)]
    struct NativeCookie {
        name: String,
        value: String,
        domain: String,
    }
    #[derive(serde::Deserialize)]
    struct NativeCookies {
        cookies: Vec<NativeCookie>,
    }
    let handle = app.clone();
    let reply = tauri::async_runtime::spawn_blocking(move || {
        handle
            .lunor_native()
            .call::<NativeCookies>("signIn", serde_json::json!({}))
    })
    .await;
    let native = match reply {
        Ok(Ok(native)) => native,
        Ok(Err(_)) | Err(_) => {
            state.publish(app, state.idle_status());
            return Err(YouTubeError::WindowFailed);
        }
    };
    if native.cookies.is_empty() {
        let status = state.idle_status();
        state.publish(app, status);
        return Ok(status);
    }
    let jar: Vec<Cookie<'static>> = native
        .cookies
        .into_iter()
        .take(256)
        .map(|c| Cookie::build((c.name, c.value)).domain(c.domain).build())
        .collect();
    match finish_sign_in(app, state, &jar).await {
        Ok(()) => Ok(ConnectionStatus::Connected),
        Err(error) => {
            if state.status() != ConnectionStatus::StoreError {
                state.publish(app, state.idle_status());
            }
            Err(error)
        }
    }
}

/// Mobile: validates the session cookies with the service, stores them in the OS key store and publishes Connected.
/// Same rules as the desktop capture: only the session names, and a value the service accepts.
#[cfg(mobile)]
async fn finish_sign_in(
    app: &AppHandle,
    state: &YouTubeState,
    cookies: &[Cookie<'_>],
) -> Result<(), YouTubeError> {
    let cookie = session_cookie(cookies).ok_or_else(|| {
        eprintln!(
            "sign-in failed: no session cookie among {} cookies",
            cookies.len()
        );
        YouTubeError::SignInIncomplete
    })?;
    let yt = state.yt.as_ref().ok_or(YouTubeError::Unavailable)?;
    if !matches!(
        tokio::time::timeout(Duration::from_secs(20), yt.connect(&cookie)).await,
        Ok(Ok(()))
    ) {
        eprintln!("sign-in failed: the service did not accept the session");
        return Err(YouTubeError::SessionRejected);
    }
    if state.store.set(&cookie).is_err() {
        eprintln!("sign-in failed: the key store refused the session");
        yt.clear_session();
        state.publish(app, ConnectionStatus::StoreError);
        return Err(YouTubeError::StoreAccess);
    }
    state.publish(app, ConnectionStatus::Connected);
    let importing = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::commands::youtube_import::run_auto(&importing, true).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn youtube_connect(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, YouTubeState>,
) -> Result<ConnectionStatus, YouTubeError> {
    main_only(&window)?;
    let _guard = state.operation.lock().await;
    if state.yt.is_none() {
        return Err(YouTubeError::Unavailable);
    }
    state.publish(&app, ConnectionStatus::Connecting);
    sign_in_window(&app, &state).await
}

#[cfg(desktop)]
/// Opens the app-owned Google sign-in window. The window is the only place the user types credentials.
async fn sign_in_window(
    app: &AppHandle,
    state: &YouTubeState,
) -> Result<ConnectionStatus, YouTubeError> {
    if let Some(existing) = app.get_webview_window(LOGIN_WINDOW) {
        existing
            .set_focus()
            .map_err(|_| YouTubeError::WindowFailed)?;
        return Ok(ConnectionStatus::Connecting);
    }
    let mut builder = WebviewWindowBuilder::new(app, LOGIN_WINDOW, WebviewUrl::External(
        "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F".parse().expect("fixed URL")))
        .title("Sign in to YouTube — Google").inner_size(520.0, 720.0).incognito(true);
    if let Some(agent) = LOGIN_USER_AGENT {
        builder = builder.user_agent(agent);
    }
    let login = builder
        .on_navigation(|url| {
            let allowed = login_navigation(url);
            // Debug builds only, host and scheme only (a full URL can carry tokens): shows which hop a blank sign-in window stopped at.
            #[cfg(debug_assertions)]
            eprintln!(
                "[youtube sign-in] navigation {} {}://{}",
                if allowed { "allowed" } else { "BLOCKED" },
                url.scheme(),
                url.host_str().unwrap_or("-")
            );
            allowed
        })
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished && youtube_origin(payload.url()) {
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    capture_when_ready(&app).await;
                });
            }
        })
        .build()
        .map_err(|_| {
            state.publish(app, state.idle_status());
            YouTubeError::WindowFailed
        })?;
    let handle = app.clone();
    login.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let app = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<YouTubeState>();
                let _guard = state.operation.lock().await;
                if state.status() == ConnectionStatus::Connecting {
                    state.publish(&app, state.idle_status());
                }
            });
        }
    });
    Ok(ConnectionStatus::Connecting)
}

#[tauri::command]
pub async fn youtube_connect_finish(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<ConnectionStatus, YouTubeError> {
    main_only(&window)?;
    #[cfg(desktop)]
    return capture(&app).await;
    // Mobile completes sign-in inside `youtube_connect`; nothing is left to finish.
    #[cfg(mobile)]
    return Ok(app.state::<YouTubeState>().status());
}

#[tauri::command]
pub async fn youtube_connection_status(
    window: WebviewWindow,
    state: State<'_, YouTubeState>,
) -> Result<ConnectionStatus, YouTubeError> {
    main_only(&window)?;
    Ok(state.status())
}

/// Import now, on request. Returns what was added; a run that is already going answers `inUse`.
#[tauri::command]
pub async fn youtube_import(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, YouTubeState>,
) -> Result<crate::commands::youtube_import::ImportReport, crate::core::library::LibraryError> {
    if window.label() != "main" {
        return Err(crate::core::library::LibraryError::Validation);
    }
    if state.status() != ConnectionStatus::Connected {
        return Err(crate::core::library::LibraryError::Unavailable);
    }
    crate::commands::youtube_import::run(&app).await
}

/// Whether likes made here are also sent to the signed-in YouTube account, and how many are still waiting to be sent.
#[tauri::command]
pub fn youtube_like_push_get(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<crate::commands::youtube_import::LikePushDto, YouTubeError> {
    main_only(&window)?;
    Ok(crate::commands::youtube_import::like_push_state(&app))
}

/// Off by default: turning it on lets this app like and un-like songs on the user's YouTube account.
#[tauri::command]
pub fn youtube_like_push_set(
    app: AppHandle,
    window: WebviewWindow,
    enabled: bool,
) -> Result<crate::commands::youtube_import::LikePushDto, YouTubeError> {
    main_only(&window)?;
    Ok(crate::commands::youtube_import::set_like_push(
        &app, enabled,
    ))
}

#[tauri::command]
pub async fn youtube_disconnect(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, YouTubeState>,
) -> Result<ConnectionStatus, YouTubeError> {
    main_only(&window)?;
    let _guard = state.operation.lock().await;
    #[cfg(desktop)]
    if let Some(login) = app.get_webview_window(LOGIN_WINDOW) {
        let _ = login.close();
    }
    if let Some(yt) = &state.yt {
        yt.clear_session();
    }
    if state.store.clear().is_err() {
        state.publish(&app, ConnectionStatus::StoreError);
        return Err(YouTubeError::StoreAccess);
    }
    let _ = crate::commands::player::player_stop(
        app.clone(),
        app.state::<crate::commands::player::PlayerState>(),
    )
    .await;
    state.publish(&app, ConnectionStatus::Disconnected);
    Ok(ConnectionStatus::Disconnected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_youtube_session_cookies_are_selected() {
        let cookies = [
            Cookie::build(("SAPISID", "SECRET"))
                .domain(".youtube.com")
                .build(),
            Cookie::build(("SID", "GOOGLE_SECRET"))
                .domain(".google.com")
                .build(),
            Cookie::build(("arbitrary", "OTHER_SECRET"))
                .domain(".youtube.com")
                .build(),
        ];
        // google.com session cookies count, other names and domains do not.
        assert_eq!(
            session_cookie(&cookies).as_deref(),
            Some("SAPISID=SECRET; SID=GOOGLE_SECRET")
        );
        assert!(session_cookie(&cookies[1..]).is_none());
        // A youtube.com cookie wins over a google.com one of the same name.
        let both = [
            Cookie::build(("SAPISID", "FROM_GOOGLE"))
                .domain(".google.com")
                .build(),
            Cookie::build(("SAPISID", "FROM_YOUTUBE"))
                .domain(".youtube.com")
                .build(),
            Cookie::build(("SAPISID", "OTHER"))
                .domain(".evil.test")
                .build(),
        ];
        assert_eq!(
            session_cookie(&both).as_deref(),
            Some("SAPISID=FROM_YOUTUBE")
        );
        let only_google = [Cookie::build(("SAPISID", "G"))
            .domain(".google.com")
            .build()];
        assert_eq!(session_cookie(&only_google).as_deref(), Some("SAPISID=G"));
        let bad = [Cookie::build(("SAPISID", "bad\r\nheader"))
            .domain(".youtube.com")
            .build()];
        assert!(session_cookie(&bad).is_none());
    }
    #[test]
    fn login_navigation_stays_on_youtube_and_google_hosts() {
        for url in [
            "https://accounts.google.com/signin",
            "https://accounts.google.co.th/signin",
            "https://accounts.google.com.au/",
            "https://accounts.google.de/",
            "https://www.google.com/accounts/SetSID",
            "https://ogs.google.com/u/0/widget",
            "https://google.com/",
            "https://google.co.th/",
            "https://consent.youtube.com/m",
            "about:blank",
            "https://youtube.com/",
            "https://www.youtube.com/",
            // iPhone user agents are sent to the mobile site, which the exact list did not cover.
            "https://m.youtube.com/watch?v=abc",
            "https://music.youtube.com/",
        ] {
            assert!(login_navigation(&url.parse().unwrap()), "{url}");
        }
        for url in [
            "http://accounts.google.com/",
            "https://accounts.google.com.evil.test/",
            "https://accounts.google.evil/",
            "https://accounts.google.co.evil/",
            "https://accounts.google.th.evil.test/",
            "https://google.co.th.evil.test/",
            "https://google.com.evil/",
            "https://notgoogle.com/",
            "https://google..com/",
            "https://accounts.google.com./",
            "https://m.youtube.com.evil.test/",
            "https://evil-youtube.com/",
            "https://music.youtube.com:8443/",
            "about:srcdoc",
            "https://evil.test/",
            "file:///tmp/anything",
            "https://u@music.youtube.com/",
        ] {
            assert!(!login_navigation(&url.parse().unwrap()), "{url}");
        }
    }
    #[test]
    fn youtube_origin_includes_the_mobile_site() {
        for url in [
            "https://youtube.com/",
            "https://www.youtube.com/",
            "https://m.youtube.com/",
            "https://music.youtube.com/",
        ] {
            assert!(youtube_origin(&url.parse().unwrap()), "{url}");
        }
        assert!(!youtube_origin(&"https://www.google.com/".parse().unwrap()));
    }
}
