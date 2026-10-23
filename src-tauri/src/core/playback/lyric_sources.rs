//! Extra lyric sources: LyricsPlus, Boidu, Netease Cloud Music, QQ Music, Kugou.
//!
//! These are UNOFFICIAL third-party endpoints (see docs/development/lyrics-sources.md). The owner
//! accepted that they may change or break. Rules enforced here:
//! - only public track metadata (title, artists, album, duration) is sent; no cookie, token,
//!   Authorization header, Google session or user identifier;
//! - HTTPS only (relaxed in unit tests for the local mock), no redirects, a fixed host per source
//!   from a constant table, never a host taken from input;
//! - 1 MiB response cap, at most 3 requests per source and track, at most 4 sources in flight,
//!   an 8 s budget per source, cancellation when another track is requested;
//! - one source failing never affects another source or audio; lyric text is never logged.
use super::{
    lyric_prefs::{self, Settings},
    lyrics::{self, TimedLine, TimedWord},
    lyrics_cache,
    track_info::LyricsSource,
};
use serde::de::DeserializeOwned;
use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, watch};

mod boidu;
mod kugou;
mod lyricsplus;
mod netease;
mod qq;

const MAX_BODY: usize = 1_048_576;
const MAX_REQUESTS: usize = 3;
const MAX_IN_FLIGHT: usize = 4;
const SOURCE_BUDGET: Duration = Duration::from_secs(8);
const MAX_LINES: usize = 1_000;
const MAX_WORDS: usize = 80;
const MAX_MS: u64 = 86_400_000;
const MAX_LINE_CHARS: usize = 300;
const MAX_CANDIDATES: usize = 30;

/// Fixed host per source. Nothing in a request or a response can change where requests go.
pub const HOSTS: [(&str, &str); 5] = [
    ("lyricsplus", "https://lyricsplus.binimum.org"),
    ("boidu", "https://lyrics-api.boidu.dev"),
    ("netease", "https://music.163.com"),
    ("qq", "https://c.y.qq.com"),
    ("kugou", "https://lyrics.kugou.com"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceId {
    LyricsPlus,
    Boidu,
    Netease,
    Qq,
    Kugou,
}

impl SourceId {
    pub const ALL: [SourceId; 5] = [
        SourceId::LyricsPlus,
        SourceId::Boidu,
        SourceId::Netease,
        SourceId::Qq,
        SourceId::Kugou,
    ];
    pub fn id(self) -> &'static str {
        match self {
            SourceId::LyricsPlus => "lyricsplus",
            SourceId::Boidu => "boidu",
            SourceId::Netease => "netease",
            SourceId::Qq => "qq",
            SourceId::Kugou => "kugou",
        }
    }
    pub fn label(self) -> &'static str {
        lyric_prefs::SOURCES
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, label)| label)
    }
    pub fn parse(id: &str) -> Option<SourceId> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
    fn base(self) -> &'static str {
        HOSTS
            .iter()
            .find(|(id, _)| *id == self.id())
            .map_or("", |(_, base)| base)
    }
    /// LyricsPlus aggregates other services on a cold request, which takes up to ~8 s (measured).
    fn timeout(self) -> Duration {
        match self {
            SourceId::LyricsPlus => Duration::from_secs(8),
            _ => Duration::from_secs(4),
        }
    }
    fn referer(self) -> Option<&'static str> {
        (self == SourceId::Qq).then_some("https://y.qq.com/")
    }
    /// Sources with a title search that "Wrong song? Search by title" can use.
    pub fn can_search(self) -> bool {
        matches!(self, SourceId::Netease | SourceId::Qq | SourceId::Kugou)
    }
}

/// The playing track. Public metadata only.
#[derive(Debug, Clone)]
pub struct Query {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: u32,
}

impl Query {
    fn secs(&self) -> u32 {
        self.duration_ms / 1000
    }
    pub fn searchable(&self) -> bool {
        lyrics::searchable(&self.artists, Some(self.secs()))
    }
    fn first_artist(&self) -> &str {
        self.artists.first().map_or("", String::as_str)
    }
    fn clean_title(&self) -> String {
        lyrics::clean_title(&self.title)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    pub synced: bool,
    pub words: bool,
    /// Sources whose API answers a direct lookup carry the lyrics already.
    prefetched: Option<SourceLyrics>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceLyrics {
    pub text: Option<String>,
    pub lines: Option<Vec<TimedLine>>,
    pub instrumental: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fail {
    NotFound,
    Unreachable,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Matched {
    pub text: String,
    pub lines: Option<Vec<TimedLine>>,
    pub words: bool,
    pub timing_mismatch: bool,
    pub bound: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Found(Matched),
    Instrumental,
    NoMatch,
    Unreachable,
    Failed,
    Disabled,
    Unavailable,
}

impl From<Fail> for Outcome {
    fn from(fail: Fail) -> Outcome {
        match fail {
            Fail::NotFound => Outcome::NoMatch,
            Fail::Unreachable => Outcome::Unreachable,
            Fail::Failed => Outcome::Failed,
        }
    }
}

/// What the caller decided: settings, per-track bindings, cache directory. Defaults mean "all
/// enabled, default order, no bindings, no cache".
#[derive(Default)]
pub struct Ctx {
    pub settings: Settings,
    /// (source id, candidate key) for the track being looked up.
    pub bindings: Vec<(String, String)>,
    pub cache_dir: Option<PathBuf>,
    /// Tests only: replaces every source's fixed base URL with a local mock.
    pub base_override: Option<String>,
    /// Tests only: replaces the per-request timeout.
    pub request_timeout: Option<Duration>,
}

struct Http {
    client: reqwest::Client,
    base: String,
    referer: Option<&'static str>,
    budget: AtomicUsize,
}

impl Http {
    fn new(kind: SourceId, ctx: &Ctx) -> Option<Http> {
        let timeout = ctx.request_timeout.unwrap_or_else(|| kind.timeout());
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .https_only(!cfg!(test))
            .user_agent("LUNOR-X/0.1.0 (desktop lyrics)")
            .build()
            .ok()?;
        Some(Http {
            client,
            base: ctx
                .base_override
                .clone()
                .unwrap_or_else(|| kind.base().to_owned()),
            referer: kind.referer(),
            budget: AtomicUsize::new(MAX_REQUESTS),
        })
    }

    /// One GET. 404 is a clean miss; 401/403 are the caller's to interpret; 429, 5xx, timeouts and
    /// connection errors are unreachable, never a miss.
    async fn get(&self, path: &str, params: &[(&str, &str)]) -> Result<Vec<u8>, Fail> {
        self.get_status(path, params)
            .await
            .map_err(|(fail, _)| fail)
    }

    async fn get_status(
        &self,
        path: &str,
        params: &[(&str, &str)],
    ) -> Result<Vec<u8>, (Fail, u16)> {
        if !take_one(&self.budget) {
            return Err((Fail::Failed, 0));
        }
        let mut request = self
            .client
            .get(format!("{}{path}", self.base))
            .query(params);
        if let Some(referer) = self.referer {
            request = request.header(reqwest::header::REFERER, referer);
        }
        let mut response = request.send().await.map_err(|_| (Fail::Unreachable, 0))?;
        let status = response.status();
        let code = status.as_u16();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Err((Fail::NotFound, code));
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err((Fail::Unreachable, code));
        }
        if !status.is_success() {
            return Err((Fail::Failed, code));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| (Fail::Unreachable, code))?
        {
            if body.len() + chunk.len() > MAX_BODY {
                return Err((Fail::Failed, code));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

fn json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Fail> {
    serde_json::from_slice(bytes).map_err(|_| Fail::Failed)
}

fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    names.collect::<Vec<_>>().join(", ")
}

/// A line with checked time and trimmed, bounded text. None for empty text or an out-of-range time.
fn line(start: u64, text: &str, words: Option<Vec<TimedWord>>) -> Option<TimedLine> {
    let text: String = text.trim().chars().take(MAX_LINE_CHARS).collect();
    if text.is_empty() || start > MAX_MS {
        return None;
    }
    Some(TimedLine {
        start_ms: start as u32,
        text,
        words,
    })
}

fn word(start: u64, end: u64, text: &str) -> Option<TimedWord> {
    (start <= MAX_MS && end <= MAX_MS && end >= start && !text.trim().is_empty()).then(|| {
        TimedWord {
            start_ms: start as u32,
            end_ms: end as u32,
            text: text.chars().take(MAX_LINE_CHARS).collect(),
        }
    })
}

/// Sorts by start time and checks the bounds every source shares: at most 1000 lines, at most 80
/// words per line, times within 24 h, word starts never going backwards. Err = malformed response.
fn checked(mut lines: Vec<TimedLine>) -> Result<Vec<TimedLine>, Fail> {
    if lines.len() > MAX_LINES {
        return Err(Fail::Failed);
    }
    lines.sort_by_key(|l| l.start_ms);
    for l in &lines {
        let Some(words) = &l.words else { continue };
        let ordered = words.windows(2).all(|w| w[0].start_ms <= w[1].start_ms);
        if words.len() > MAX_WORDS
            || words.is_empty()
            || !ordered
            || words.iter().any(|w| w.end_ms < w.start_ms)
        {
            return Err(Fail::Failed);
        }
    }
    Ok(lines)
}

/// XML/HTML entities that appear in lyric text (`&amp;`, `&#39;`, ...).
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Credit lines such as `作词 : name` at the head of a lyric are not lyrics.
fn strip_credits(lines: &mut Vec<TimedLine>) {
    const PREFIXES: [&str; 12] = [
        "作词",
        "作曲",
        "编曲",
        "作詞",
        "混音",
        "制作人",
        "词",
        "曲",
        "lyrics by",
        "composed by",
        "produced by",
        "arranged by",
    ];
    let mut index = 0;
    lines.retain(|l| {
        index += 1;
        let low = l.text.to_lowercase();
        let credit = index <= 12
            && PREFIXES.iter().any(|p| {
                low.strip_prefix(p)
                    .is_some_and(|rest| rest.trim_start().starts_with([':', '：']))
            });
        !credit
    });
}

fn plain(lines: &[TimedLine]) -> String {
    lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A decoded LRC body from a Chinese service, with credits removed. None only over the line bound.
fn lrc_lines(text: &str) -> Result<Vec<TimedLine>, Fail> {
    let mut lines = lyrics::parse_lrc_lines(text).ok_or(Fail::Failed)?;
    for l in &mut lines {
        l.text = unescape(&l.text);
    }
    strip_credits(&mut lines);
    checked(lines)
}

fn decode_b64(value: &str) -> Result<String, Fail> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|_| Fail::Failed)?;
    let text = String::from_utf8(bytes).map_err(|_| Fail::Failed)?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

async fn search(
    kind: SourceId,
    http: &Http,
    query: &Query,
    text: Option<&str>,
) -> Result<Vec<Candidate>, Fail> {
    match kind {
        SourceId::LyricsPlus => lyricsplus::search(http, query, text).await,
        SourceId::Boidu => boidu::search(http, query, text).await,
        SourceId::Netease => netease::search(http, query, text).await,
        SourceId::Qq => qq::search(http, query, text).await,
        SourceId::Kugou => kugou::search(http, query, text).await,
    }
}

async fn fetch(kind: SourceId, http: &Http, key: &str) -> Result<SourceLyrics, Fail> {
    match kind {
        SourceId::Netease => netease::fetch(http, key).await,
        SourceId::Qq => qq::fetch(http, key).await,
        SourceId::Kugou => kugou::fetch(http, key).await,
        // Direct-lookup sources have nothing to fetch by key.
        SourceId::LyricsPlus | SourceId::Boidu => Err(Fail::Failed),
    }
}

struct Done {
    text: Option<String>,
    lines: Option<Vec<TimedLine>>,
    instrumental: bool,
    timing_mismatch: bool,
}

/// Turns raw source lyrics into something showable. Timestamps are kept only when the length rule
/// allows it (`timed_ok`) and none lies past the end of the track (`limit_ms` + 2 s); otherwise the
/// text is shown plain and flagged as a timing mismatch. None = nothing usable.
fn finish(found: SourceLyrics, timed_ok: bool, limit_ms: u32) -> Option<Done> {
    if found.instrumental {
        return Some(Done {
            text: None,
            lines: None,
            instrumental: true,
            timing_mismatch: false,
        });
    }
    let lines = found.lines.filter(|v| !v.is_empty());
    let joined = lines.as_deref().map(plain);
    let had_timing = lines.as_ref().is_some_and(|v| v.len() >= 4)
        && joined.as_deref().is_some_and(lyrics::complete);
    let within = lines
        .as_ref()
        .and_then(|v| v.last())
        .is_some_and(|l| u64::from(l.start_ms) <= u64::from(limit_ms) + 2000);
    if had_timing && timed_ok && within {
        return Some(Done {
            text: found.text.filter(|t| lyrics::complete(t)).or(joined),
            lines,
            instrumental: false,
            timing_mismatch: false,
        });
    }
    let text = found
        .text
        .filter(|t| lyrics::complete(t))
        .or_else(|| joined.filter(|t| lyrics::complete(t)))?;
    Some(Done {
        text: Some(text),
        lines: None,
        instrumental: false,
        timing_mismatch: had_timing,
    })
}

fn matched(done: Done, bound: bool) -> Option<Matched> {
    let words = done
        .lines
        .as_ref()
        .is_some_and(|v| v.iter().any(|l| l.words.is_some()));
    Some(Matched {
        text: done.text?,
        lines: done.lines,
        words,
        timing_mismatch: done.timing_mismatch,
        bound,
    })
}

/// Candidates that pass the shared matching rules, best first (timing allowed beats plain only).
fn rank(candidates: Vec<Candidate>, query: &Query) -> Vec<(Candidate, bool)> {
    let mut ranked: Vec<(Candidate, lyrics::Assessment)> = candidates
        .into_iter()
        .filter_map(|c| {
            let a = lyrics::assess_parts(
                &c.title,
                &c.artist,
                f64::from(c.duration_ms) / 1000.0,
                c.synced,
                &query.title,
                &query.artists,
                query.secs(),
            )?;
            Some((c, a))
        })
        .collect();
    ranked.sort_by(|a, b| {
        (b.1.timed, b.1.score)
            .partial_cmp(&(a.1.timed, a.1.score))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    ranked.into_iter().map(|(c, a)| (c, a.timed)).collect()
}

/// Automatic match: search, rank, then fetch at most the two best (1 + 2 = 3 requests).
async fn automatic(kind: SourceId, http: &Http, query: &Query) -> Outcome {
    let candidates = match search(kind, http, query, None).await {
        Ok(c) => c,
        Err(fail) => return fail.into(),
    };
    let mut plain_fallback: Option<Matched> = None;
    let mut failure = None;
    for (mut candidate, timed_ok) in rank(candidates, query).into_iter().take(2) {
        let found = match candidate.prefetched.take() {
            Some(found) => found,
            None => match fetch(kind, http, &candidate.key).await {
                Ok(found) => found,
                Err(Fail::NotFound) => continue,
                Err(fail) => {
                    failure = Some(fail);
                    continue;
                }
            },
        };
        match finish(found, timed_ok, candidate.duration_ms) {
            Some(done) if done.instrumental => return Outcome::Instrumental,
            Some(done) => {
                if let Some(m) = matched(done, false) {
                    if m.lines.is_some() {
                        return Outcome::Found(m);
                    }
                    plain_fallback.get_or_insert(m);
                }
            }
            None => {}
        }
    }
    match (plain_fallback, failure) {
        (Some(m), _) => Outcome::Found(m),
        (None, Some(fail)) => fail.into(),
        (None, None) => Outcome::NoMatch,
    }
}

/// The user picked this exact candidate, so the length rule does not apply; timestamps still must
/// not run past the end of the track.
async fn bound(kind: SourceId, http: &Http, query: &Query, key: &str) -> Outcome {
    if !lyric_prefs::valid_key(key) {
        return Outcome::Failed;
    }
    match fetch(kind, http, key).await {
        Ok(found) => match finish(found, true, query.duration_ms) {
            Some(done) if done.instrumental => Outcome::Instrumental,
            Some(done) => matched(done, true).map_or(Outcome::NoMatch, Outcome::Found),
            None => Outcome::NoMatch,
        },
        Err(fail) => fail.into(),
    }
}

fn cache_key(query: &Query, bound_key: Option<&str>) -> String {
    format!(
        "{}\u{0}{}",
        lyrics::negative_key(&query.title, &query.artists, query.secs()),
        bound_key.unwrap_or("")
    )
}

fn from_cache(entry: lyrics_cache::SourceEntry, query: &Query, bound: bool) -> Option<Outcome> {
    if entry.instrumental {
        return Some(Outcome::Instrumental);
    }
    // Timestamps were accepted for a track of about this length; another length is a miss.
    if entry.lines.is_some() && entry.duration_ms.abs_diff(query.duration_ms) > 3000 {
        return None;
    }
    let words = entry
        .lines
        .as_ref()
        .is_some_and(|v| v.iter().any(|l| l.words.is_some()));
    Some(Outcome::Found(Matched {
        text: entry.text?,
        lines: entry.lines,
        words,
        timing_mismatch: entry.timing_mismatch,
        bound,
    }))
}

/// One source for one track: disabled/unavailable checks, cache, the 8 s budget, then the network.
/// `bound_key` is the user's chosen candidate; `ignore_negative` is for "Try again".
pub async fn run_one(
    ctx: &Ctx,
    kind: SourceId,
    query: &Query,
    bound_key: Option<&str>,
    ignore_negative: bool,
) -> Outcome {
    if ctx.settings.is_disabled(kind.id()) {
        return Outcome::Disabled;
    }
    if !query.searchable() {
        return Outcome::Unavailable;
    }
    let key = cache_key(query, bound_key);
    let paths = ctx.cache_dir.as_deref().map(|dir| {
        let positive = lyrics_cache::source_path(dir, kind.id(), &key);
        let negative = lyrics_cache::negative_path(&positive, &format!("{}\u{0}{key}", kind.id()));
        (positive, negative)
    });
    let negative_key = format!("{}\u{0}{key}", kind.id());
    if let Some((positive, negative)) = &paths {
        if let Some(hit) = lyrics_cache::source_load(positive, &key)
            .and_then(|e| from_cache(e, query, bound_key.is_some()))
        {
            return hit;
        }
        if !ignore_negative
            && negative
                .as_deref()
                .is_some_and(|p| lyrics_cache::negative_fresh(p, &negative_key))
        {
            return Outcome::NoMatch;
        }
    }
    let Some(http) = Http::new(kind, ctx) else {
        return Outcome::Failed;
    };
    let work = async {
        match bound_key {
            Some(key) => bound(kind, &http, query, key).await,
            None => automatic(kind, &http, query).await,
        }
    };
    let outcome = tokio::time::timeout(SOURCE_BUDGET, work)
        .await
        .unwrap_or(Outcome::Unreachable);
    // Only definite answers are remembered. Network errors, timeouts and bad bodies never are.
    if let Some((positive, negative)) = &paths {
        match &outcome {
            Outcome::Found(m) => lyrics_cache::source_save(
                positive,
                &lyrics_cache::SourceEntry {
                    key,
                    text: Some(m.text.clone()),
                    lines: m.lines.clone(),
                    instrumental: false,
                    timing_mismatch: m.timing_mismatch,
                    duration_ms: query.duration_ms,
                },
            ),
            Outcome::Instrumental => lyrics_cache::source_save(
                positive,
                &lyrics_cache::SourceEntry {
                    key,
                    text: None,
                    lines: None,
                    instrumental: true,
                    timing_mismatch: false,
                    duration_ms: query.duration_ms,
                },
            ),
            Outcome::NoMatch => {
                if let Some(path) = negative {
                    lyrics_cache::negative_save(path, &negative_key);
                }
            }
            _ => {}
        }
    }
    outcome
}

static SLOTS: Semaphore = Semaphore::const_new(MAX_IN_FLIGHT);
static LATEST: OnceLock<watch::Sender<String>> = OnceLock::new();

fn latest() -> &'static watch::Sender<String> {
    LATEST.get_or_init(|| watch::channel(String::new()).0)
}

/// Resolves when a different track has been requested after `track`.
async fn superseded(track: &str) {
    let mut seen = latest().subscribe();
    loop {
        if *seen.borrow_and_update() != track {
            return;
        }
        if seen.changed().await.is_err() {
            return;
        }
    }
}

async fn run_slot(ctx: &Ctx, kind: SourceId, query: &Query) -> Outcome {
    if ctx.settings.is_disabled(kind.id()) {
        return Outcome::Disabled;
    }
    if !query.searchable() {
        return Outcome::Unavailable;
    }
    // Disabled sources never reach this point, so they never wait for a slot or send a request.
    let Ok(_slot) = SLOTS.acquire().await else {
        return Outcome::Failed;
    };
    let bound_key = ctx
        .bindings
        .iter()
        .find(|(source, _)| source == kind.id())
        .map(|(_, key)| key.as_str());
    run_one(ctx, kind, query, bound_key, false).await
}

/// All five sources, concurrently (at most 4 in flight process-wide). Another call for a different
/// track cancels this one; cancelled sources report `Failed`, which the stale caller discards.
pub async fn run_all(ctx: &Ctx, track_id: &str, query: &Query) -> Vec<(SourceId, Outcome)> {
    latest().send_replace(track_id.to_owned());
    let all = async {
        let (a, b, c, d, e) = tokio::join!(
            run_slot(ctx, SourceId::LyricsPlus, query),
            run_slot(ctx, SourceId::Boidu, query),
            run_slot(ctx, SourceId::Netease, query),
            run_slot(ctx, SourceId::Qq, query),
            run_slot(ctx, SourceId::Kugou, query),
        );
        [a, b, c, d, e]
    };
    let outcomes = tokio::select! {
        outcomes = all => outcomes,
        () = superseded(track_id) => [const { Outcome::Failed }; 5],
    };
    SourceId::ALL.into_iter().zip(outcomes).collect()
}

/// "Wrong song? Search by title": up to 30 candidates of one source for a free-text query.
/// Sources without a title search return none.
pub async fn search_text(
    ctx: &Ctx,
    kind: SourceId,
    query: &Query,
    text: &str,
) -> Result<Vec<Candidate>, Outcome> {
    if ctx.settings.is_disabled(kind.id()) {
        return Err(Outcome::Disabled);
    }
    if !kind.can_search() {
        return Ok(Vec::new());
    }
    let Some(http) = Http::new(kind, ctx) else {
        return Err(Outcome::Failed);
    };
    let _slot = SLOTS.acquire().await.map_err(|_| Outcome::Failed)?;
    let found = tokio::time::timeout(SOURCE_BUDGET, search(kind, &http, query, Some(text)))
        .await
        .map_err(|_| Outcome::Unreachable)?;
    match found {
        Ok(mut list) => {
            list.retain(|c| lyric_prefs::valid_key(&c.key));
            list.truncate(MAX_CANDIDATES);
            Ok(list)
        }
        Err(Fail::NotFound) => Ok(Vec::new()),
        Err(fail) => Err(fail.into()),
    }
}

/// How close a free-text search result is to the playing track (title similarity + artist overlap).
pub fn closeness(candidate: &Candidate, title: &str, artists: &[String]) -> f64 {
    let artist = if lyrics::artist_overlap(artists, &candidate.artist) {
        1.0
    } else {
        0.0
    };
    if title.trim().is_empty() {
        artist
    } else {
        lyrics::similarity(title, &candidate.title) + artist
    }
}

/// Source entry for the DTO. Lines are never mixed between entries.
pub fn entry(kind: SourceId, outcome: &Outcome) -> LyricsSource {
    entry_for(kind.id(), kind.label(), outcome)
}

pub fn entry_for(id: &'static str, label: &str, outcome: &Outcome) -> LyricsSource {
    let status = match outcome {
        Outcome::Found(m) if m.timing_mismatch => "timingMismatch",
        Outcome::Found(_) => "available",
        Outcome::Instrumental => "instrumental",
        Outcome::NoMatch => "noMatch",
        Outcome::Unreachable => "unreachable",
        Outcome::Failed => "failed",
        Outcome::Disabled => "disabled",
        Outcome::Unavailable => "unavailable",
    };
    let found = match outcome {
        Outcome::Found(m) => Some(m),
        _ => None,
    };
    LyricsSource {
        id,
        label: label.to_owned(),
        available: found.is_some(),
        text: found.map(|m| m.text.clone()),
        lines: found.and_then(|m| m.lines.clone()),
        synced: found.is_some_and(|m| m.lines.is_some()),
        words: found.is_some_and(|m| m.words),
        bound: found.is_some_and(|m| m.bound),
        attribution: found.map(|m| {
            let kind = if m.words {
                "word by word"
            } else if m.lines.is_some() {
                "synced"
            } else {
                "plain"
            };
            format!("Lyrics from {label} · {kind}")
        }),
        status,
    }
}

/// Takes one unit from the request budget. Not `fetch_update`: newer toolchains deprecate it for `try_update`.
fn take_one(budget: &std::sync::atomic::AtomicUsize) -> bool {
    let mut n = budget.load(Ordering::SeqCst);
    while n > 0 {
        match budget.compare_exchange_weak(n, n - 1, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(now) => n = now,
        }
    }
    false
}

#[cfg(test)]
mod tests;
