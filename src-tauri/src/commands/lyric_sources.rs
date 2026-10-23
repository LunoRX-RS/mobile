//! Lyric source IPC: thin layer over `core::playback::{lyric_sources, lyric_prefs}`. The frontend
//! sends ids, track metadata and opaque candidate keys, never paths, hosts or URLs.
use crate::core::playback::{
    extract,
    lyric_prefs::{self, Binding, PrefsError, Settings},
    lyric_sources::{self as sources, Ctx, Outcome, Query, SourceId},
    lyrics,
    track_info::{self, LyricsSource},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, WebviewWindow};

const MAX_TITLE: usize = 300;
const MAX_ARTIST: usize = 200;
const MAX_ARTISTS: usize = 8;
const MAX_QUERY: usize = 200;
const MAX_DURATION_MS: i64 = 86_400_000;

/// Fixed categories only; no paths, URLs or OS text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "category", rename_all = "camelCase")]
pub enum LyricsCommandError {
    Validation,
    UnknownSource,
    Unsupported,
    StorageUnavailable,
    TooLarge,
    Unreachable,
    SourceFailed,
}

impl From<PrefsError> for LyricsCommandError {
    fn from(error: PrefsError) -> Self {
        match error {
            PrefsError::Validation => LyricsCommandError::Validation,
            PrefsError::TooLarge => LyricsCommandError::TooLarge,
            PrefsError::Storage => LyricsCommandError::StorageUnavailable,
        }
    }
}

type Result<T> = std::result::Result<T, LyricsCommandError>;

fn main(window: &WebviewWindow) -> Result<()> {
    (window.label() == "main")
        .then_some(())
        .ok_or(LyricsCommandError::Validation)
}

async fn blocking<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|_| LyricsCommandError::StorageUnavailable)
}

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|_| LyricsCommandError::StorageUnavailable)
}

fn settings_path(dir: &Path) -> PathBuf {
    dir.join("lyrics-settings.json")
}

fn bindings_path(dir: &Path) -> PathBuf {
    dir.join("lyrics-bindings.json")
}

/// Settings, this track's bindings and the shared lyric cache directory. Missing or damaged files
/// mean defaults, so a broken settings file never stops lyrics.
pub fn build_ctx(data: Option<&Path>, cache: Option<PathBuf>, track: &str) -> Ctx {
    Ctx {
        settings: data
            .map(|d| lyric_prefs::load_settings(&settings_path(d)))
            .unwrap_or_default(),
        bindings: data
            .map(|d| lyric_prefs::bindings_for(&bindings_path(d), track))
            .unwrap_or_default(),
        cache_dir: cache,
        ..Ctx::default()
    }
}

/// Used by `player_track_info`.
pub async fn context(app: &AppHandle, track: &str) -> Ctx {
    let data = app.path().app_data_dir().ok();
    let cache = app.path().app_cache_dir().ok().map(|d| d.join("lyrics"));
    let track = track.to_owned();
    tauri::async_runtime::spawn_blocking(move || build_ctx(data.as_deref(), cache, &track))
        .await
        .unwrap_or_default()
}

async fn load_ctx(app: &AppHandle, track: &str) -> Result<Ctx> {
    let (data, cache) = (
        data_dir(app)?,
        app.path().app_cache_dir().ok().map(|d| d.join("lyrics")),
    );
    let track = track.to_owned();
    blocking(move || build_ctx(Some(&data), cache, &track)).await
}

fn text_ok(value: &str, max: usize) -> bool {
    let t = value.trim();
    !t.is_empty() && t.chars().count() <= max && !t.chars().any(char::is_control)
}

fn track_query(
    title: &str,
    artists: &[String],
    album: Option<&str>,
    duration_ms: i64,
) -> Result<Query> {
    if !text_ok(title, MAX_TITLE)
        || artists.is_empty()
        || artists.len() > MAX_ARTISTS
        || !artists.iter().all(|a| text_ok(a, MAX_ARTIST))
        || album.is_some_and(|a| !a.is_empty() && !text_ok(a, MAX_TITLE))
        || !(1..=MAX_DURATION_MS).contains(&duration_ms)
    {
        return Err(LyricsCommandError::Validation);
    }
    Ok(Query {
        title: title.trim().to_owned(),
        artists: artists.iter().map(|a| a.trim().to_owned()).collect(),
        album: album.filter(|a| !a.is_empty()).map(|a| a.trim().to_owned()),
        duration_ms: u32::try_from(duration_ms).map_err(|_| LyricsCommandError::Validation)?,
    })
}

fn track_id(value: &str) -> Result<()> {
    extract::valid_id(value)
        .then_some(())
        .ok_or(LyricsCommandError::Validation)
}

fn source(id: &str) -> Result<SourceId> {
    SourceId::parse(id).ok_or(LyricsCommandError::UnknownSource)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchArgs {
    pub provider_track_id: String,
    pub source_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: i64,
}

/// "Try again" on one source: ignores its negative cache and re-runs only that source.
#[tauri::command]
pub async fn lyrics_source_fetch(
    window: WebviewWindow,
    app: AppHandle,
    args: FetchArgs,
) -> Result<LyricsSource> {
    main(&window)?;
    track_id(&args.provider_track_id)?;
    let query = track_query(
        &args.title,
        &args.artists,
        args.album.as_deref(),
        args.duration_ms,
    )?;
    let ctx = load_ctx(&app, &args.provider_track_id).await?;
    refetch(&ctx, &args.source_id, &query).await
}

async fn refetch(ctx: &Ctx, source_id: &str, query: &Query) -> Result<LyricsSource> {
    match source_id {
        "lrclib" => {
            let disabled = ctx.settings.is_disabled("lrclib");
            let lookup = if disabled || !query.searchable() {
                Ok(None)
            } else {
                tokio::time::timeout(
                    std::time::Duration::from_secs(9),
                    lyrics::fallback(
                        &query.title,
                        &query.artists,
                        query.album.as_deref(),
                        Some(query.duration_ms / 1000),
                    ),
                )
                .await
                .unwrap_or(Err(lyrics::LyricsError))
            };
            let mut entry = track_info::lrclib_source(&lookup);
            if disabled {
                entry.status = "disabled";
            } else if !query.searchable() {
                entry.status = "unavailable";
            }
            Ok(entry)
        }
        // YouTube Music lyrics need the YouTube client; the track info call is the way to refresh it.
        "youtubeMusic" => Err(LyricsCommandError::Unsupported),
        id => {
            let kind = source(id)?;
            let bound = ctx
                .bindings
                .iter()
                .find(|(s, _)| s == kind.id())
                .map(|(_, key)| key.as_str());
            Ok(sources::entry(
                kind,
                &sources::run_one(ctx, kind, query, bound, true).await,
            ))
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchArgs {
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    pub query: String,
    pub source_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateDto {
    pub source_id: &'static str,
    pub key: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    pub synced: bool,
    pub words: bool,
}

#[derive(Serialize)]
pub struct SearchResults {
    pub candidates: Vec<CandidateDto>,
}

fn clean_query(value: &str) -> Result<String> {
    let t = value.trim();
    (!t.is_empty() && t.chars().count() <= MAX_QUERY && !t.chars().any(char::is_control))
        .then(|| t.to_owned())
        .ok_or(LyricsCommandError::Validation)
}

/// "Wrong song? Search by title" on one source or on every enabled source with a title search.
#[tauri::command]
pub async fn lyrics_source_search(
    window: WebviewWindow,
    app: AppHandle,
    args: SearchArgs,
) -> Result<SearchResults> {
    main(&window)?;
    let text = clean_query(&args.query)?;
    if !text_ok(&args.title, MAX_TITLE)
        || args.artists.len() > MAX_ARTISTS
        || !args.artists.iter().all(|a| text_ok(a, MAX_ARTIST))
    {
        return Err(LyricsCommandError::Validation);
    }
    let one = args.source_id.as_deref().map(source).transpose()?;
    let data = data_dir(&app)?;
    let ctx = blocking(move || build_ctx(Some(&data), None, "")).await?;
    let query = Query {
        title: args.title.trim().to_owned(),
        artists: args.artists,
        album: None,
        duration_ms: 0,
    };
    search_all(&ctx, one, &query, &text).await
}

async fn search_all(
    ctx: &Ctx,
    one: Option<SourceId>,
    query: &Query,
    text: &str,
) -> Result<SearchResults> {
    let kinds: Vec<SourceId> = match one {
        Some(kind) => vec![kind],
        None => SourceId::ALL
            .into_iter()
            .filter(|k| k.can_search())
            .collect(),
    };
    let (a, b, c) = {
        let kinds = &kinds;
        let get = |i: usize| async move {
            match kinds.get(i) {
                Some(kind) => Some((*kind, sources::search_text(ctx, *kind, query, text).await)),
                None => None,
            }
        };
        tokio::join!(get(0), get(1), get(2))
    };
    let mut candidates = Vec::new();
    let mut failure = None;
    for (kind, result) in [a, b, c].into_iter().flatten() {
        match result {
            Ok(list) => candidates.extend(list.into_iter().map(|c| (kind, c))),
            Err(Outcome::Unreachable) => failure = Some(LyricsCommandError::Unreachable),
            Err(Outcome::Disabled) => {}
            Err(_) => failure = Some(LyricsCommandError::SourceFailed),
        }
    }
    // One named source that failed is an error; among several, the others still answer.
    if one.is_some()
        && let Some(error) = failure
    {
        return Err(error);
    }
    let mut ordered = candidates;
    ordered.sort_by(|(_, a), (_, b)| {
        let score = |c: &sources::Candidate| sources::closeness(c, &query.title, &query.artists);
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(SearchResults {
        candidates: ordered
            .into_iter()
            .take(30)
            .map(|(kind, c)| CandidateDto {
                source_id: kind.id(),
                key: c.key,
                title: c.title,
                artist: c.artist,
                duration_ms: c.duration_ms,
                synced: c.synced,
                words: c.words,
            })
            .collect(),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindArgs {
    pub provider_track_id: String,
    pub source_id: String,
    pub candidate_key: String,
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: i64,
}

/// Fetches exactly the chosen candidate and, when it gave lyrics, remembers the choice for this track.
#[tauri::command]
pub async fn lyrics_source_bind(
    window: WebviewWindow,
    app: AppHandle,
    args: BindArgs,
) -> Result<LyricsSource> {
    main(&window)?;
    track_id(&args.provider_track_id)?;
    let kind = source(&args.source_id)?;
    if !kind.can_search() {
        return Err(LyricsCommandError::Unsupported);
    }
    if !lyric_prefs::valid_key(&args.candidate_key) {
        return Err(LyricsCommandError::Validation);
    }
    let query = track_query(&args.title, &args.artists, None, args.duration_ms)?;
    let ctx = load_ctx(&app, &args.provider_track_id).await?;
    let outcome = sources::run_one(&ctx, kind, &query, Some(&args.candidate_key), true).await;
    if matches!(outcome, Outcome::Found(_) | Outcome::Instrumental) {
        let path = bindings_path(&data_dir(&app)?);
        let binding = Binding {
            provider_track_id: args.provider_track_id,
            source_id: kind.id().to_owned(),
            candidate_key: args.candidate_key,
        };
        blocking(move || lyric_prefs::bind(&path, binding))
            .await?
            .map_err(LyricsCommandError::from)?;
    }
    Ok(sources::entry(kind, &outcome))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnbindArgs {
    pub provider_track_id: String,
    pub source_id: String,
}

#[derive(Serialize)]
pub struct Unbound {
    pub unbound: bool,
}

#[tauri::command]
pub async fn lyrics_source_unbind(
    window: WebviewWindow,
    app: AppHandle,
    args: UnbindArgs,
) -> Result<Unbound> {
    main(&window)?;
    track_id(&args.provider_track_id)?;
    source(&args.source_id)?;
    let path = bindings_path(&data_dir(&app)?);
    blocking(move || lyric_prefs::unbind(&path, &args.provider_track_id, &args.source_id))
        .await?
        .map(|unbound| Unbound { unbound })
        .map_err(LyricsCommandError::from)
}

#[derive(Serialize)]
pub struct KnownSource {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Serialize)]
pub struct SettingsDto {
    pub order: Vec<String>,
    pub disabled: Vec<String>,
    pub sources: Vec<KnownSource>,
}

fn settings_dto(settings: Settings) -> SettingsDto {
    SettingsDto {
        order: settings.order,
        disabled: settings.disabled,
        sources: lyric_prefs::SOURCES
            .iter()
            .map(|(id, label)| KnownSource { id, label })
            .collect(),
    }
}

/// `lyrics_settings_get {}`: an empty args object, so the call shape matches the other commands.
#[derive(Deserialize)]
pub struct SettingsGetArgs {}

#[tauri::command]
pub async fn lyrics_settings_get(
    window: WebviewWindow,
    app: AppHandle,
    _args: SettingsGetArgs,
) -> Result<SettingsDto> {
    main(&window)?;
    let path = settings_path(&data_dir(&app)?);
    blocking(move || settings_dto(lyric_prefs::load_settings(&path))).await
}

#[derive(Deserialize)]
pub struct SettingsSetArgs {
    pub order: Vec<String>,
    pub disabled: Vec<String>,
}

#[tauri::command]
pub async fn lyrics_settings_set(
    window: WebviewWindow,
    app: AppHandle,
    args: SettingsSetArgs,
) -> Result<SettingsDto> {
    main(&window)?;
    let settings = lyric_prefs::normalize(&args.order, &args.disabled)?;
    let path = settings_path(&data_dir(&app)?);
    blocking(move || lyric_prefs::save_settings(&path, &settings).map(|()| settings))
        .await?
        .map(settings_dto)
        .map_err(LyricsCommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artists(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn track_arguments_are_validated() {
        let ok = |t: &str, a: &[&str], d: i64| track_query(t, &artists(a), None, d);
        assert!(ok("Song", &["A"], 180_000).is_ok());
        assert!(ok("", &["A"], 180_000).is_err());
        assert!(ok(&"t".repeat(301), &["A"], 180_000).is_err());
        assert!(ok("Song", &[], 180_000).is_err());
        assert!(ok("Song", &["A"; 9], 180_000).is_err());
        assert!(ok("Song", &[&"a".repeat(201)], 180_000).is_err());
        assert!(ok("Song", &["A"], 0).is_err());
        assert!(ok("Song", &["A"], 86_400_001).is_err());
        assert!(ok("Song", &["A"], 86_400_000).is_ok());
        assert!(ok("So\nng", &["A"], 1).is_err());
        assert!(track_query("S", &artists(&["A"]), Some(&"x".repeat(301)), 5).is_err());
    }

    #[test]
    fn search_text_is_trimmed_bounded_and_without_control_characters() {
        assert_eq!(clean_query("  hello ").unwrap(), "hello");
        assert!(clean_query("   ").is_err());
        assert!(clean_query(&"q".repeat(201)).is_err());
        assert!(clean_query(&"q".repeat(200)).is_ok());
        assert!(clean_query("a\u{0}b").is_err());
        assert!(clean_query("a\nb").is_err());
    }

    #[test]
    fn errors_serialize_as_fixed_categories() {
        let json = serde_json::to_value(LyricsCommandError::UnknownSource).unwrap();
        assert_eq!(json, serde_json::json!({"category": "unknownSource"}));
        assert_eq!(
            LyricsCommandError::from(PrefsError::TooLarge),
            LyricsCommandError::TooLarge
        );
    }

    #[test]
    fn unknown_and_simpmusic_ids_are_not_sources() {
        assert_eq!(source("simpmusic"), Err(LyricsCommandError::UnknownSource));
        assert_eq!(source("netease").unwrap(), SourceId::Netease);
    }

    #[test]
    fn context_reads_settings_and_only_this_tracks_bindings() {
        let root = std::env::temp_dir().join(format!("lunor-ctx-{:032x}", rand::random::<u128>()));
        let settings = lyric_prefs::normalize(&artists(&["qq"]), &artists(&["boidu"])).unwrap();
        lyric_prefs::save_settings(&settings_path(&root), &settings).unwrap();
        lyric_prefs::bind(
            &bindings_path(&root),
            Binding {
                provider_track_id: "dQw4w9WgXcQ".into(),
                source_id: "qq".into(),
                candidate_key: "abc".into(),
            },
        )
        .unwrap();
        let ctx = build_ctx(Some(&root), None, "dQw4w9WgXcQ");
        assert!(ctx.settings.is_disabled("boidu"));
        assert_eq!(ctx.bindings, [("qq".to_owned(), "abc".to_owned())]);
        assert!(
            build_ctx(Some(&root), None, "other_track")
                .bindings
                .is_empty()
        );
        assert_eq!(build_ctx(None, None, "x").settings, Settings::default());
        std::fs::remove_dir_all(root).unwrap();
    }
}
