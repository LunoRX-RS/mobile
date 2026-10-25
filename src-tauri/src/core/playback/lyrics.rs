//! Independent fallback: only public track metadata goes to LRCLIB, never the Google session.
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// One word or syllable with real start/end times from a word-level source. Never estimated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedWord {
    pub start_ms: u32,
    pub end_ms: u32,
    /// Includes the trailing space when the source separated it from the next word.
    pub text: String,
}

/// One lyric line with a real start time taken from an LRC record. Never estimated.
/// `words` is set only when the source carried real word timings; it serializes as null otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimedLine {
    pub start_ms: u32,
    pub text: String,
    #[serde(default)]
    pub words: Option<Vec<TimedWord>>,
}

const MAX_TIMED_LINES: usize = 1_000;
const MAX_LINE_CHARS: usize = 300;
const MAX_TAGS_PER_LINE: usize = 16;
// Longest start time accepted: 24 h, the same bound as seeking.
const MAX_START_MS: u64 = 86_400_000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Record {
    pub track_name: String,
    pub artist_name: String,
    pub duration: f64,
    #[serde(default)]
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct LyricsError;

pub struct FallbackLyrics {
    pub text: Option<String>,
    /// Present only when the matching record has complete, real timestamps.
    pub timed: Option<Vec<TimedLine>>,
    pub instrumental: bool,
    /// LRCLIB has timestamps for this song but its length differs too much to trust them.
    pub timing_mismatch: bool,
}

// Matching rules. A record is accepted only when the title is a near match, an artist overlaps and
// the length is close; the length decides whether its timestamps may be used.
const MIN_TITLE_SIM: f64 = 0.82;
/// Timestamps from another edit are wrong, so timed lyrics need a near-identical length.
const TIMED_MAX_DELTA_S: f64 = 3.0;
/// Beyond this the record is probably another recording. Between the two bounds: plain text only.
const PLAIN_MAX_DELTA_S: f64 = 12.0;
const MAX_REQUESTS: usize = 4;
// Under the 9 s deadline track_info puts around the whole lookup.
const TOTAL_BUDGET: Duration = Duration::from_millis(8_500);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_SEARCH_RECORDS: usize = 50;

/// Characters that never distinguish titles or names: whitespace, ASCII and general punctuation
/// (includes zero-width characters, U+2000 block), CJK and fullwidth punctuation, BOM.
fn is_sep(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(c, '\u{2000}'..='\u{206f}' | '\u{3000}'..='\u{303f}' | '\u{feff}' | '\u{ff01}'..='\u{ff0f}' | '\u{ff1a}'..='\u{ff20}')
}

// Thai vowel and tone marks are kept: they distinguish words.
// ponytail: no Unicode NFC step (no normalization crate is a dependency); LRCLIB and YouTube text is NFC in practice.
fn normalized(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .filter(|c| !is_sep(*c))
        .collect()
}

fn tokens(value: &str) -> std::collections::BTreeSet<String> {
    tokens_in_order(value).into_iter().collect()
}

fn dice(a: &[char], b: &[char]) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return f64::from(u8::from(a == b && !a.is_empty()));
    }
    let mut counts = std::collections::HashMap::new();
    for pair in a.windows(2) {
        *counts.entry((pair[0], pair[1])).or_insert(0i32) += 1;
    }
    let mut shared = 0;
    for pair in b.windows(2) {
        if let Some(n) = counts.get_mut(&(pair[0], pair[1]))
            && *n > 0
        {
            *n -= 1;
            shared += 1;
        }
    }
    2.0 * f64::from(shared) / ((a.len() - 1 + b.len() - 1) as f64)
}

/// `1 - distance / longer length`, over at most 128 characters each.
fn edit_ratio(a: &[char], b: &[char]) -> f64 {
    let (a, b) = (&a[..a.len().min(128)], &b[..b.len().min(128)]);
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(x != y));
            diagonal = above;
        }
    }
    let longest = a.len().max(b.len()).max(1);
    1.0 - row[b.len()] as f64 / longest as f64
}

/// 0..=1. Bigram Dice (works for Thai, which has no spaces), edit ratio, and token-set Dice.
pub(super) fn similarity(a: &str, b: &str) -> f64 {
    let (na, nb) = (normalized(a), normalized(b));
    if na.is_empty() || nb.is_empty() {
        return 0.0;
    }
    if na == nb {
        return 1.0;
    }
    let (ca, cb): (Vec<char>, Vec<char>) = (na.chars().collect(), nb.chars().collect());
    let (ta, tb) = (tokens(a), tokens(b));
    let token_dice = 2.0 * ta.intersection(&tb).count() as f64 / (ta.len() + tb.len()) as f64;
    dice(&ca, &cb).max(edit_ratio(&ca, &cb)).max(token_dice)
}

const NOISE: &[&[&str]] = &[
    &["official", "music", "video"],
    &["official", "lyric", "video"],
    &["official", "lyrics", "video"],
    &["official", "visualizer"],
    &["official", "video"],
    &["official", "audio"],
    &["official", "lyrics"],
    &["official", "mv"],
    &["official", "m/v"],
    &["lyric", "video"],
    &["lyrics", "video"],
    &["music", "video"],
    &["mv"],
    &["m/v"],
];
const CREDITS: &[&str] = &["feat", "ft", "featuring", "prod"];

/// Title for querying: no bracketed segments, no trailing feat/ft/prod credit, no "Official Video"
/// style noise. The original stays what the UI shows. Falls back to the input when nothing is left.
pub fn clean_title(title: &str) -> String {
    let mut depth = 0usize;
    let mut flat = String::new();
    for c in title.chars() {
        match c {
            '(' | '[' | '{' | '（' | '【' | '「' => depth += 1,
            ')' | ']' | '}' | '）' | '】' | '」' => depth = depth.saturating_sub(1),
            _ if depth == 0 => flat.push(c),
            _ => {}
        }
    }
    let words: Vec<&str> = flat.split_whitespace().collect();
    let low = |w: &str| {
        w.to_lowercase()
            .trim_matches(|c: char| c != '/' && is_sep(c))
            .to_owned()
    };
    let end = words
        .iter()
        .position(|w| CREDITS.contains(&low(w).as_str()))
        .filter(|i| *i > 0)
        .unwrap_or(words.len());
    let mut kept: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < end {
        let skip = NOISE
            .iter()
            .find(|p| {
                i + p.len() <= end && p.iter().enumerate().all(|(k, n)| low(words[i + k]) == *n)
            })
            .map_or(0, |p| p.len());
        if skip > 0 {
            i += skip;
        } else {
            kept.push(words[i]);
            i += 1;
        }
    }
    if kept.len() > 1
        && kept
            .last()
            .is_some_and(|w| matches!(low(w).as_str(), "lyrics" | "lyric"))
    {
        kept.pop();
    }
    let separator = |w: &&str| w.chars().all(is_sep);
    while kept.last().is_some_and(separator) {
        kept.pop();
    }
    while kept.first().is_some_and(separator) {
        kept.remove(0);
    }
    if kept.is_empty() {
        return title.trim().to_owned();
    }
    kept.join(" ")
}

/// Version labels that mean a different recording. `sped up` is two words, so words are scanned in pairs.
fn version_markers(title: &str) -> std::collections::BTreeSet<&'static str> {
    const SINGLE: &[&str] = &[
        "live",
        "remix",
        "acoustic",
        "instrumental",
        "karaoke",
        "slowed",
        "reverb",
        "8d",
        "nightcore",
        "cover",
    ];
    let lower = title.to_lowercase();
    let words: Vec<&str> = lower.split(is_sep).filter(|w| !w.is_empty()).collect();
    let mut found = std::collections::BTreeSet::new();
    for (i, word) in words.iter().enumerate() {
        if let Some(m) = SINGLE.iter().find(|m| *m == word) {
            found.insert(*m);
        }
        if matches!(*word, "spedup" | "speedup")
            || (*word == "sped" && words.get(i + 1) == Some(&"up"))
        {
            found.insert("sped up");
        }
    }
    found
}

pub(super) fn artist_overlap(wanted: &[String], record_artist: &str) -> bool {
    let parts: Vec<String> = record_artist
        .split([',', '&', ';', '/', '×'])
        .flat_map(|part| part.split(" feat. "))
        .map(normalized)
        .filter(|n| !n.is_empty())
        .collect();
    let record_tokens: Vec<String> = tokens_in_order(record_artist);
    wanted.iter().any(|name| {
        let n = normalized(name);
        let words = tokens_in_order(name);
        !n.is_empty()
            && (parts.contains(&n)
                // "SURIYA MQT" inside "SURIYA MQT P6ICK": whole words, in order, 2+ of them.
                // One word would let "Artist" match "Other Artist".
                || (words.len() >= 2 && record_tokens.windows(words.len()).any(|w| w == words.as_slice())))
    })
}

fn tokens_in_order(value: &str) -> Vec<String> {
    value
        .to_lowercase()
        .split(is_sep)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Assessment {
    /// Timestamps of this record may be used.
    pub timed: bool,
    pub score: f64,
}

/// None = reject. Pure: no I/O. See the constants above for the thresholds.
fn assess(record: &Record, title: &str, artists: &[String], duration: u32) -> Option<Assessment> {
    assess_parts(
        &record.track_name,
        &record.artist_name,
        record.duration,
        record
            .synced_lyrics
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        title,
        artists,
        duration,
    )
}

/// The same rules for any source's candidate: `found_*` describe the candidate, the rest the playing track.
pub(super) fn assess_parts(
    found_title: &str,
    found_artist: &str,
    found_duration_s: f64,
    synced: bool,
    title: &str,
    artists: &[String],
    duration: u32,
) -> Option<Assessment> {
    if !found_duration_s.is_finite() {
        return None;
    }
    let delta = (found_duration_s - f64::from(duration)).abs();
    if delta > PLAIN_MAX_DELTA_S {
        return None;
    }
    let (query, found) = (version_markers(title), version_markers(found_title));
    // A marker the other side lacks means another recording; only a query marker with an
    // unmarked record is tolerated (the text of the original song), and then without timing.
    let same = query == found;
    if !same && !found.is_empty() {
        return None;
    }
    let sim = similarity(found_title, title)
        .max(similarity(&clean_title(found_title), &clean_title(title)));
    if sim < MIN_TITLE_SIM || !artist_overlap(artists, found_artist) {
        return None;
    }
    Some(Assessment {
        timed: same && delta <= TIMED_MAX_DELTA_S,
        score: sim * 2.0
            + (1.0 - delta / PLAIN_MAX_DELTA_S) * 0.5
            + if synced { 0.3 } else { 0.0 }
            + if same { 0.2 } else { 0.0 },
    })
}

/// A matched record turned into lyrics. `record` is what to cache: for a plain-only match it has
/// its timestamps removed, so a later offline restore cannot expose timing from another edit.
struct Pick {
    record: Record,
    found: FallbackLyrics,
    score: f64,
}

impl Pick {
    /// Timed or instrumental ends the search; plain text is kept as a fallback.
    fn acceptable(&self) -> bool {
        self.found.timed.is_some() || self.found.instrumental
    }
}

pub(super) fn complete(text: &str) -> bool {
    text.trim().chars().count() >= 40
}

fn realize(record: Record, timed: bool) -> Option<Pick> {
    if timed {
        let found = usable(record.clone())?;
        return Some(Pick {
            record,
            found,
            score: 0.0,
        });
    }
    let had_timing = record
        .synced_lyrics
        .as_deref()
        .and_then(parse_lrc)
        .is_some();
    let text = record
        .plain_lyrics
        .clone()
        .filter(|t| complete(t))
        .or_else(|| {
            record
                .synced_lyrics
                .as_deref()
                .map(plain_lrc)
                .filter(|t| complete(t))
        });
    let record = Record {
        plain_lyrics: text,
        synced_lyrics: None,
        ..record
    };
    let mut found = usable(record.clone())?;
    found.timing_mismatch = had_timing && !found.instrumental;
    Some(Pick {
        record,
        found,
        score: 0.0,
    })
}

/// Best usable candidate: acceptable (timed/instrumental) beats plain, then higher score.
fn pick(records: Vec<Record>, title: &str, artists: &[String], duration: u32) -> Option<Pick> {
    records
        .into_iter()
        .take(MAX_SEARCH_RECORDS)
        .filter_map(|record| {
            let a = assess(&record, title, artists, duration)?;
            realize(record, a.timed).map(|p| Pick {
                score: a.score,
                ..p
            })
        })
        .reduce(|best, next| if better(&next, &best) { next } else { best })
}

fn better(a: &Pick, b: &Pick) -> bool {
    (a.acceptable(), a.score) > (b.acceptable(), b.score)
}

#[derive(Debug, Clone, PartialEq)]
enum Variant {
    Get {
        title: String,
        artist: String,
        album: Option<String>,
    },
    Search(Vec<(&'static str, String)>),
}

/// Query variants in priority order, deduplicated, never more than MAX_REQUESTS.
/// (b) with the second artist comes last because search already covers the first artist.
fn variants(title: &str, artists: &[String], album: Option<&str>) -> Vec<Variant> {
    let clean = clean_title(title);
    let first = artists.first().cloned().unwrap_or_default();
    let get = |t: &str, artist: &str, album: Option<&str>| Variant::Get {
        title: t.to_owned(),
        artist: artist.to_owned(),
        album: album.filter(|a| !a.is_empty()).map(str::to_owned),
    };
    let mut list = vec![
        get(title, &first, album),
        get(&clean, &first, None),
        Variant::Search(vec![
            ("track_name", clean.clone()),
            ("artist_name", first.clone()),
        ]),
        Variant::Search(vec![("q", format!("{clean} {first}"))]),
    ];
    if let Some(second) = artists.get(1).filter(|a| !a.trim().is_empty()) {
        list.push(get(&clean, second, None));
    }
    let mut unique: Vec<Variant> = Vec::new();
    for v in list {
        if !unique.contains(&v) {
            unique.push(v);
        }
    }
    unique.truncate(MAX_REQUESTS);
    unique
}

/// Same track, same 5 s length bucket. Cleaned title so remix/feat suffixes share one entry.
pub(super) fn negative_key(title: &str, artists: &[String], duration: u32) -> String {
    format!(
        "{}\u{0}{}\u{0}{}",
        normalized(&clean_title(title)),
        artists.first().map(|a| normalized(a)).unwrap_or_default(),
        duration / 5
    )
}

/// Whether a lookup can run at all. Unknown or hour-long recordings cannot be matched safely.
pub fn searchable(artists: &[String], duration: Option<u32>) -> bool {
    duration.is_some_and(|d| (1..=3600).contains(&d))
        && artists.first().is_some_and(|a| !a.trim().is_empty())
}

/// True when this track already has a positive or a fresh negative cache entry.
pub fn cache_has_entry(
    cache: &std::path::Path,
    title: &str,
    artists: &[String],
    duration: u32,
) -> bool {
    super::lyrics_cache::load(cache).is_some()
        || super::lyrics_cache::negative_path(cache, &negative_key(title, artists, duration))
            .is_some_and(|p| {
                super::lyrics_cache::negative_fresh(&p, &negative_key(title, artists, duration))
            })
}

pub(super) fn plain_lrc(value: &str) -> String {
    value
        .lines()
        .filter_map(|line| {
            let mut rest = line.trim();
            let mut timed = false;
            while let Some(tag) = rest.strip_prefix('[').and_then(|rest| rest.split_once(']')) {
                let (minutes, seconds) = tag.0.split_once(':')?;
                if minutes.parse::<u32>().is_err()
                    || !seconds
                        .parse::<f64>()
                        .is_ok_and(|n| n.is_finite() && (0.0..60.0).contains(&n))
                {
                    return None;
                }
                timed = true;
                rest = tag.1.trim_start();
            }
            (timed && !rest.is_empty()).then_some(rest)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `mm:ss`, `mm:ss.f`, `mm:ss.ff` or `mm:ss.fff` to milliseconds. None for anything else.
fn time_tag(tag: &str) -> Option<u32> {
    let (minutes, seconds) = tag.split_once(':')?;
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let digits = |value: &str| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    if !digits(minutes) || minutes.len() > 4 || !digits(whole) || whole.len() != 2 {
        return None;
    }
    if !fraction.is_empty() && (!digits(fraction) || fraction.len() > 3) {
        return None;
    }
    let seconds: u64 = whole.parse().ok()?;
    if seconds >= 60 {
        return None;
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()? * 10u64.pow(3 - fraction.len() as u32)
    };
    let total = minutes.parse::<u64>().ok()? * 60_000 + seconds * 1000 + millis;
    (total <= MAX_START_MS).then_some(total as u32)
}

/// Removes enhanced-LRC word tags such as `<00:01.20>`; word timings are not used.
fn strip_word_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        if time_tag(&rest[open + 1..open + close]).is_some() {
            out.push_str(&rest[..open]);
            rest = &rest[open + close + 1..];
        } else {
            out.push_str(&rest[..=open]);
            rest = &rest[open + 1..];
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parses standard LRC into lines sorted by start time. Metadata tags (`[ar:..]`, `[offset:..]`),
/// junk lines and empty lines are skipped; `[offset:..]` is not applied. Returns None when the set
/// is over the line bound or not complete (under 4 lines or under 40 characters in total).
pub fn parse_lrc(value: &str) -> Option<Vec<TimedLine>> {
    let lines = parse_lrc_lines(value)?;
    let chars: usize = lines.iter().map(|l| l.text.chars().count()).sum();
    (lines.len() >= 4 && chars >= 40).then_some(lines)
}

/// Same bounded parse as `parse_lrc` without the completeness check, for user-authored files
/// that may still be short. None only when over the line bound.
pub fn parse_lrc_lines(value: &str) -> Option<Vec<TimedLine>> {
    let mut lines: Vec<TimedLine> = Vec::new();
    for raw in value.lines() {
        let mut rest = raw.trim();
        let mut starts: Vec<u32> = Vec::new();
        while let Some((tag, after)) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            match time_tag(tag.trim()) {
                Some(ms) if starts.len() < MAX_TAGS_PER_LINE => starts.push(ms),
                _ => break,
            }
            rest = after.trim_start();
        }
        if starts.is_empty() {
            continue;
        }
        let text = strip_word_tags(rest);
        if text.is_empty() {
            continue;
        }
        let text: String = text.chars().take(MAX_LINE_CHARS).collect();
        for start_ms in starts {
            if lines.len() >= MAX_TIMED_LINES {
                return None;
            }
            lines.push(TimedLine {
                start_ms,
                text: text.clone(),
                words: None,
            });
        }
    }
    lines.sort_by_key(|line| line.start_ms);
    Some(lines)
}

/// The line that should be on screen at `position_ms`: the last one whose start time has passed.
/// `lines` is sorted by start time, so a partition point is enough. None before the first line.
pub fn line_at(lines: &[TimedLine], position_ms: u64) -> Option<&TimedLine> {
    lines[..lines.partition_point(|line| u64::from(line.start_ms) <= position_ms)].last()
}

pub(super) fn usable(record: Record) -> Option<FallbackLyrics> {
    if record.instrumental {
        return Some(FallbackLyrics {
            text: None,
            timed: None,
            instrumental: true,
            timing_mismatch: false,
        });
    }
    // Community records can contain placeholder fragments instead of a song's lyrics.
    let complete = |text: &String| complete(text);
    // Timestamps later than the matched duration mean the record belongs to another recording.
    let limit = (record.duration + 2.0) * 1000.0;
    let timed = record
        .synced_lyrics
        .as_deref()
        .and_then(parse_lrc)
        .filter(|lines| lines.last().is_some_and(|l| f64::from(l.start_ms) <= limit));
    let text = record
        .plain_lyrics
        .filter(complete)
        .or_else(|| {
            record
                .synced_lyrics
                .map(|text| plain_lrc(&text))
                .filter(complete)
        })
        .or_else(|| {
            timed.as_ref().map(|lines| {
                lines
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })?;
    Some(FallbackLyrics {
        text: Some(text),
        timed,
        instrumental: false,
        timing_mismatch: false,
    })
}

async fn read<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, LyricsError> {
    if !response.status().is_success() {
        return Err(LyricsError);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| LyricsError)? {
        if body.len() + chunk.len() > 1_048_576 {
            return Err(LyricsError);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| LyricsError)
}

pub async fn fallback(
    title: &str,
    artists: &[String],
    album: Option<&str>,
    duration: Option<u32>,
) -> Result<Option<FallbackLyrics>, LyricsError> {
    fallback_cached(title, artists, album, duration, None).await
}

pub async fn fallback_cached(
    title: &str,
    artists: &[String],
    album: Option<&str>,
    duration: Option<u32>,
    cache: Option<&std::path::Path>,
) -> Result<Option<FallbackLyrics>, LyricsError> {
    let Some(duration) = duration.filter(|_| searchable(artists, duration)) else {
        return Ok(None);
    };
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("LUNOR-X/0.1.0 (desktop lyrics)")
        .build()
        .map_err(|_| LyricsError)?;
    lookup(
        &client,
        "https://lrclib.net/api",
        title,
        artists,
        album,
        duration,
        cache,
    )
    .await
}

/// One HTTP request. 404 is a clean miss; any other failure is an error, never a miss.
async fn fetch(
    client: &reqwest::Client,
    base: &str,
    variant: &Variant,
    duration: u32,
) -> Result<Vec<Record>, LyricsError> {
    match variant {
        Variant::Get {
            title,
            artist,
            album,
        } => {
            let mut params = vec![
                ("track_name", title.clone()),
                ("artist_name", artist.clone()),
                ("duration", duration.to_string()),
            ];
            if let Some(album) = album {
                params.push(("album_name", album.clone()));
            }
            let response = client
                .get(format!("{base}/get"))
                .query(&params)
                .send()
                .await
                .map_err(|_| LyricsError)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(Vec::new());
            }
            read::<Record>(response).await.map(|r| vec![r])
        }
        Variant::Search(params) => {
            let response = client
                .get(format!("{base}/search"))
                .query(params)
                .send()
                .await
                .map_err(|_| LyricsError)?;
            read(response).await
        }
    }
}

async fn lookup(
    client: &reqwest::Client,
    base: &str,
    title: &str,
    artists: &[String],
    album: Option<&str>,
    duration: u32,
    cache: Option<&std::path::Path>,
) -> Result<Option<FallbackLyrics>, LyricsError> {
    if let Some(record) = cache.and_then(super::lyrics_cache::load)
        && assess(&record, title, artists, duration).is_some_and(|a| a.timed)
        && let Some(found) = usable(record)
        && (found.timed.is_some() || found.instrumental)
    {
        return Ok(Some(found));
    }
    let key = negative_key(title, artists, duration);
    let negative = cache.and_then(|path| super::lyrics_cache::negative_path(path, &key));
    if negative
        .as_deref()
        .is_some_and(|p| super::lyrics_cache::negative_fresh(p, &key))
    {
        return Ok(None);
    }
    let deadline = tokio::time::Instant::now() + TOTAL_BUDGET;
    let mut best: Option<Pick> = None;
    let mut incomplete = false;
    for variant in variants(title, artists, album) {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            incomplete = true;
            break;
        }
        let request = fetch(client, base, &variant, duration);
        match tokio::time::timeout(left.min(REQUEST_TIMEOUT), request).await {
            Ok(Ok(records)) => {
                if let Some(found) = pick(records, title, artists, duration) {
                    if found.acceptable() {
                        if let Some(path) = cache {
                            super::lyrics_cache::save(path, &found.record);
                        }
                        return Ok(Some(found.found));
                    }
                    best = Some(match best.take() {
                        Some(old) if !better(&found, &old) => old,
                        _ => found,
                    });
                }
            }
            // Network errors, timeouts, rate limits and bad bodies are not "no match".
            _ => incomplete = true,
        }
    }
    match best {
        Some(found) => {
            if let Some(path) = cache {
                super::lyrics_cache::save(path, &found.record);
            }
            Ok(Some(found.found))
        }
        None if incomplete => Err(LyricsError),
        None => {
            if let Some(path) = &negative {
                super::lyrics_cache::negative_save(path, &key);
            }
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn line_at_picks_the_current_line() {
        let timed = |starts: &[u32]| -> Vec<TimedLine> {
            starts
                .iter()
                .map(|&start_ms| TimedLine {
                    start_ms,
                    text: format!("{start_ms}"),
                    words: None,
                })
                .collect()
        };
        let lines = timed(&[1000, 5000, 9000]);
        assert_eq!(line_at(&lines, 0), None);
        assert_eq!(line_at(&lines, 999), None);
        assert_eq!(line_at(&lines, 1000).map(|l| l.text.as_str()), Some("1000"));
        assert_eq!(line_at(&lines, 4999).map(|l| l.text.as_str()), Some("1000"));
        assert_eq!(line_at(&lines, 5000).map(|l| l.text.as_str()), Some("5000"));
        assert_eq!(
            line_at(&lines, u64::from(u32::MAX)).map(|l| l.text.as_str()),
            Some("9000")
        );
        assert_eq!(line_at(&timed(&[]), 1000), None);
    }

    /// Local HTTP stand-in for LRCLIB. `handler` maps "path?query" to (status line, body).
    struct Mock {
        base: String,
        seen: Arc<Mutex<Vec<String>>>,
        leaked: Arc<AtomicBool>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Mock {
        fn requests(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
        fn stop(self) {
            self.task.abort();
            assert!(
                !self.leaked.load(Ordering::SeqCst),
                "cookie or authorization header sent"
            );
        }
    }
    async fn mock(
        handler: impl Fn(&str) -> (&'static str, String) + Send + Sync + 'static,
    ) -> Mock {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/api", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let leaked = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);
        let (s, l) = (seen.clone(), leaked.clone());
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let (handler, s, l) = (handler.clone(), s.clone(), l.clone());
                tokio::spawn(async move {
                    let mut header = Vec::new();
                    while !header.ends_with(b"\r\n\r\n") {
                        let Ok(b) = socket.read_u8().await else {
                            return;
                        };
                        header.push(b);
                    }
                    let header = String::from_utf8_lossy(&header).to_lowercase();
                    if header.contains("\r\ncookie:") || header.contains("\r\nauthorization:") {
                        l.store(true, Ordering::SeqCst);
                    }
                    let target = header.split_whitespace().nth(1).unwrap_or("").to_owned();
                    let (status, body) = handler(&target);
                    s.lock().unwrap().push(target);
                    let reply = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        Mock {
            base,
            seen,
            leaked,
            task,
        }
    }
    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap()
    }
    fn json(records: &[&Record]) -> String {
        serde_json::to_string(records).unwrap()
    }
    fn complete_record() -> Record {
        let mut r = record("Song", "Artist", 180.0);
        r.plain_lyrics =
            Some("A complete synthetic lyric for fixture testing only, not a real song".into());
        r
    }
    fn synced_record() -> Record {
        let mut r = complete_record();
        r.synced_lyrics=Some("[00:01.00]Fixture lyric line one\n[00:02.00]Fixture lyric line two\n[00:03.00]Fixture lyric line three\n[00:04.00]Fixture lyric line four".into());
        r
    }
    fn artists(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).into()).collect()
    }
    #[tokio::test]
    async fn missing_exact_lookup_uses_search_and_keeps_credentials_out() {
        let mut wrong = complete_record();
        wrong.artist_name = "Wrong artist".into();
        let mut live = complete_record();
        live.track_name = "Song (Live)".into();
        live.plain_lyrics = Some("wrong version wrong version wrong version wrong version".into());
        let right = complete_record();
        let server = mock(move |target| {
            if target.starts_with("/api/get?") {
                ("404 Not Found", "{}".into())
            } else {
                ("200 OK", json(&[&wrong, &live, &right]))
            }
        })
        .await;
        let found = lookup(
            &client(),
            &server.base,
            "Song",
            &artists(&["Artist"]),
            None,
            180,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(found.text, complete_record().plain_lyrics);
        assert!(found.timed.is_none() && !found.timing_mismatch);
        let seen = server.requests();
        assert!(seen.len() <= MAX_REQUESTS);
        assert!(seen[0].starts_with("/api/get?") && seen[1].starts_with("/api/search?"));
        server.stop();
    }
    #[tokio::test]
    async fn karaoke_prefers_synced_over_exact_plain_and_reuses_disk_without_network() {
        let (plain, synced) = (complete_record(), synced_record());
        let mut wrong = synced.clone();
        wrong.duration = 240.0;
        let server = mock(move |target| {
            if target.starts_with("/api/get?") {
                (
                    "200 OK",
                    json(&[&plain]).trim_matches(['[', ']']).to_owned(),
                )
            } else {
                ("200 OK", json(&[&wrong, &synced]))
            }
        })
        .await;
        let root =
            std::env::temp_dir().join(format!("lunor-karaoke-{:032x}", rand::random::<u128>()));
        let path = root.join("song.json");
        let a = artists(&["Artist"]);
        let found = lookup(&client(), &server.base, "Song", &a, None, 180, Some(&path))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.timed.as_ref().unwrap()[0].start_ms, 1000);
        server.stop();
        let offline = lookup(
            &client(),
            "http://127.0.0.1:1/api",
            "Song",
            &a,
            None,
            180,
            Some(&path),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(found.timed, offline.timed);
        // A different version is not served from the cache; with no network that is an error, not "no match".
        assert!(
            lookup(
                &client(),
                "http://127.0.0.1:1/api",
                "Other version",
                &a,
                None,
                180,
                Some(&path)
            )
            .await
            .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn synced_search_failure_does_not_discard_plain_exact_match() {
        let plain = complete_record();
        let server = mock(move |target| {
            if target.starts_with("/api/get?") {
                (
                    "200 OK",
                    json(&[&plain]).trim_matches(['[', ']']).to_owned(),
                )
            } else {
                ("500 Internal Server Error", "{}".into())
            }
        })
        .await;
        let found = lookup(
            &client(),
            &server.base,
            "Song",
            &artists(&["Artist"]),
            None,
            180,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(found.text, complete_record().plain_lyrics);
        assert!(found.timed.is_none());
        server.stop();
    }
    /// The reported case: YouTube title has a Remix suffix and two artists, LRCLIB has the song under one name.
    #[tokio::test]
    async fn thai_remix_with_two_artists_finds_lrclib_record() {
        let mut song = synced_record();
        song.track_name = "หนุ่มบ้านนอก".into();
        song.artist_name = "SURIYA MQT, P6ICK".into();
        song.duration = 250.0; // 6 s off: text only
        let server = mock(move |target| {
            if target.starts_with("/api/get?") {
                ("404 Not Found", "{}".into())
            } else {
                ("200 OK", json(&[&song]))
            }
        })
        .await;
        let a = artists(&["SURIYA MQT", "P6ICK"]);
        let found = lookup(
            &client(),
            &server.base,
            "หนุ่มบ้านนอก (Remix)",
            &a,
            None,
            244,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            found.timed.is_none(),
            "timestamps from another edit must not be exposed"
        );
        assert!(found.timing_mismatch);
        assert!(found.text.unwrap().contains("complete synthetic lyric"));
        assert!(server.requests().len() <= MAX_REQUESTS);
        server.stop();
    }
    #[tokio::test]
    async fn cleaned_title_get_finds_timed_record_for_feat_title() {
        let mut song = synced_record();
        song.track_name = "Song".into();
        let server = mock(move |target| {
            // Only the cleaned title exists in LRCLIB.
            if target.starts_with("/api/get?") && target.contains("track_name=song&") {
                ("200 OK", json(&[&song]).trim_matches(['[', ']']).to_owned())
            } else {
                ("404 Not Found", "{}".into())
            }
        })
        .await;
        let found = lookup(
            &client(),
            &server.base,
            "Song (feat. Someone) [Official Video]",
            &artists(&["Artist"]),
            None,
            181,
            None,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(found.timed.is_some() && !found.timing_mismatch);
        assert!(server.requests().len() <= 2);
        server.stop();
    }
    #[tokio::test]
    async fn no_match_is_remembered_but_network_errors_are_not() {
        let server = mock(|target| {
            if target.starts_with("/api/get?") {
                ("404 Not Found", "{}".into())
            } else {
                ("200 OK", "[]".into())
            }
        })
        .await;
        let root = std::env::temp_dir().join(format!("lunor-neg-{:032x}", rand::random::<u128>()));
        let path = root.join("abc.json");
        let a = artists(&["Artist"]);
        assert!(
            lookup(&client(), &server.base, "Song", &a, None, 180, Some(&path))
                .await
                .unwrap()
                .is_none()
        );
        let first = server.requests().len();
        assert!((1..=MAX_REQUESTS).contains(&first));
        // Same track, 2 s longer (same 5 s bucket): answered from the negative entry, no request.
        assert!(
            lookup(&client(), &server.base, "Song", &a, None, 181, Some(&path))
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(server.requests().len(), first);
        server.stop();
        // Errors (server gone, or HTTP 500) are failures and leave no entry behind.
        let other = root.join("def.json");
        assert!(
            lookup(
                &client(),
                "http://127.0.0.1:1/api",
                "Different",
                &a,
                None,
                180,
                Some(&other)
            )
            .await
            .is_err()
        );
        let failing = mock(|_| ("500 Internal Server Error", "{}".into())).await;
        assert!(
            lookup(
                &client(),
                &failing.base,
                "Different",
                &a,
                None,
                180,
                Some(&other)
            )
            .await
            .is_err()
        );
        failing.stop();
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            1,
            "only the first negative entry exists"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    fn record(title: &str, artist: &str, duration: f64) -> Record {
        Record {
            track_name: title.into(),
            artist_name: artist.into(),
            duration,
            instrumental: false,
            plain_lyrics: Some("fixture text".into()),
            synced_lyrics: None,
        }
    }
    fn accepts(candidate: &Record, title: &str, names: &[&str], duration: u32) -> Option<bool> {
        assess(candidate, title, &artists(names), duration).map(|a| a.timed)
    }
    #[test]
    fn rejects_wrong_artist_duration_and_version() {
        assert_eq!(
            accepts(&record("Song!", "artist", 181.0), "Song", &["Artist"], 180),
            Some(true)
        );
        for candidate in [
            record("Song", "Other Artist", 180.0),
            record("Song", "Artist", 200.0),
            record("Song (Live)", "Artist", 180.0),
            record("Song", "Artist", f64::NAN),
        ] {
            assert_eq!(accepts(&candidate, "Song", &["Artist"], 180), None);
        }
        assert_ne!(normalized("ข้าว"), normalized("ข่าว"));
    }
    #[test]
    fn duration_tolerance_timed_within_3s_plain_to_12s_then_rejected() {
        let song = |d: f64| record("Song", "Artist", d);
        assert_eq!(accepts(&song(183.0), "Song", &["Artist"], 180), Some(true));
        assert_eq!(accepts(&song(177.0), "Song", &["Artist"], 180), Some(true));
        assert_eq!(accepts(&song(185.0), "Song", &["Artist"], 180), Some(false));
        assert_eq!(accepts(&song(192.0), "Song", &["Artist"], 180), Some(false));
        assert_eq!(accepts(&song(200.0), "Song", &["Artist"], 180), None);
    }
    #[test]
    fn clean_title_strips_credits_brackets_and_video_noise() {
        for (raw, want) in [
            ("Song (feat. lil porapat)", "Song"),
            ("Song (Remix)", "Song"),
            ("Song [Official Video]", "Song"),
            ("Song feat. Someone", "Song"),
            ("Song ft. Someone Else", "Song"),
            ("Song - Official Music Video", "Song"),
            ("Song Official Audio", "Song"),
            ("Song MV", "Song"),
            ("Song M/V", "Song"),
            ("Song Lyrics", "Song"),
            ("Song prod. by Someone", "Song"),
            ("หนุ่มบ้านนอก (Remix) (feat. lil porapat)", "หนุ่มบ้านนอก"),
            // Legitimate title words survive.
            ("Video Games", "Video Games"),
            ("Dance With Me", "Dance With Me"),
            ("Feat of Strength", "Feat of Strength"),
            ("(Remix)", "(Remix)"),
        ] {
            assert_eq!(clean_title(raw), want, "{raw}");
        }
    }
    #[test]
    fn similarity_handles_thai_without_spaces_and_rejects_different_songs() {
        assert!(similarity("หนุ่มบ้านนอก", "หนุ่มบ้านนอก ") >= 0.99);
        assert!(similarity("หนุ่มบ้านนอก", "หนุ่มบ้านนอกก") >= MIN_TITLE_SIM);
        assert!(similarity("หนุ่มบ้านนอก", "สาวบ้านนอก") < MIN_TITLE_SIM);
        assert!(similarity("ข้าว", "ข่าว") < MIN_TITLE_SIM);
        assert!(similarity("Blinding Lights", "blinding   lights!") >= 0.99);
        assert!(similarity("Love Me", "Love Me Like You Do") < MIN_TITLE_SIM);
        assert!(similarity("Stay", "Stay Away") < MIN_TITLE_SIM);
        assert!(similarity("", "x") == 0.0);
        // The reported track, cleaned, matches the studio title.
        let studio = record("หนุ่มบ้านนอก", "SURIYA MQT, P6ICK", 247.0);
        assert_eq!(
            accepts(
                &studio,
                "หนุ่มบ้านนอก (feat. lil porapat)",
                &["SURIYA MQT", "P6ICK"],
                244
            ),
            Some(true)
        );
    }
    #[test]
    fn artist_overlap_accepts_any_listed_artist_and_containment() {
        let wanted = artists(&["SURIYA MQT"]);
        assert!(artist_overlap(&wanted, "SURIYA MQT, P6ICK"));
        assert!(artist_overlap(&artists(&["P6ICK"]), "SURIYA MQT, P6ICK"));
        assert!(artist_overlap(&artists(&["SURIYA MQT", "P6ICK"]), "p6ick"));
        assert!(artist_overlap(
            &artists(&["Artist A", "Artist B"]),
            "Artist A & Artist B"
        ));
        assert!(artist_overlap(&artists(&["The Weeknd"]), "The Weeknd"));
        assert!(artist_overlap(&wanted, "SURIYA MQT P6ICK"));
        assert!(!artist_overlap(&artists(&["Artist"]), "Other Artist"));
        assert!(!artist_overlap(&wanted, "Someone Else"));
        assert!(!artist_overlap(&wanted, ""));
        assert!(!artist_overlap(&artists(&["Al"]), "Alicia Keys"));
    }
    #[test]
    fn version_markers_must_agree_and_never_get_timing_from_an_unmarked_record() {
        let a = &["Artist"];
        // Query without a marker rejects marked records, whichever marker.
        for marked in [
            "Song (Live)",
            "Song - Acoustic",
            "Song (Sped Up)",
            "Song Slowed",
            "Song (8D Audio)",
            "Song [Instrumental]",
            "Song (Karaoke Version)",
            "Song (Nightcore)",
            "Song (Reverb)",
            "Song (Cover)",
            "Song Remix",
        ] {
            assert_eq!(
                accepts(&record(marked, "Artist", 180.0), "Song", a, 180),
                None,
                "{marked}"
            );
        }
        // Query with a marker rejects a different marker, accepts the same one with timing.
        assert_eq!(
            accepts(
                &record("Song (Live)", "Artist", 180.0),
                "Song (Remix)",
                a,
                180
            ),
            None
        );
        assert_eq!(
            accepts(
                &record("Song - Remix", "Artist", 180.0),
                "Song (Remix)",
                a,
                180
            ),
            Some(true)
        );
        // Unmarked record for a marked query: text of the original only, no timestamps.
        assert_eq!(
            accepts(&record("Song", "Artist", 180.0), "Song (Remix)", a, 180),
            Some(false)
        );
        assert!(version_markers("Song (Sped up)").contains("sped up"));
        assert!(version_markers("Alive and Lively").is_empty());
    }
    #[test]
    fn scoring_prefers_same_marker_synced_and_closer_duration() {
        let a = artists(&["Artist"]);
        let mut synced_far = synced_record();
        synced_far.duration = 177.0;
        let mut plain_exact = complete_record();
        plain_exact.duration = 180.0;
        let mut synced_near = synced_record();
        synced_near.duration = 181.0;
        let mut synced_exact = synced_record();
        synced_exact.duration = 180.0;
        // A timed result beats a plain one even when the plain record is the exact length.
        let best = pick(
            vec![plain_exact.clone(), synced_far.clone()],
            "Song",
            &a,
            180,
        )
        .unwrap();
        assert!(best.acceptable() && best.record.duration == 177.0);
        // Among timed results the closer duration wins.
        let best = pick(vec![synced_far, synced_near, synced_exact], "Song", &a, 180).unwrap();
        assert_eq!(best.record.duration, 180.0);
        // Plain-only candidates: nothing acceptable, best plain is returned without timing.
        let mut synced_off = synced_record();
        synced_off.duration = 186.0;
        let best = pick(vec![synced_off], "Song", &a, 180).unwrap();
        assert!(
            !best.acceptable() && best.found.timing_mismatch && best.record.synced_lyrics.is_none()
        );
        // Title similarity orders otherwise equal records.
        let mut close = complete_record();
        close.track_name = "Songs".into();
        let best = pick(vec![close, plain_exact], "Song", &a, 180).unwrap();
        assert_eq!(best.record.track_name, "Song");
        assert!(pick(vec![], "Song", &a, 180).is_none());
    }
    #[test]
    fn variants_are_ordered_deduplicated_and_within_the_request_budget() {
        let plain = variants("Song", &artists(&["Artist"]), Some("Album"));
        // Title already clean: the cleaned /get duplicates only if album is also absent.
        assert!(plain.len() <= MAX_REQUESTS);
        assert!(matches!(&plain[0], Variant::Get { album: Some(a), .. } if a == "Album"));
        let dirty = variants(
            "หนุ่มบ้านนอก (Remix) (feat. lil porapat)",
            &artists(&["SURIYA MQT", "P6ICK"]),
            None,
        );
        assert_eq!(dirty.len(), MAX_REQUESTS);
        assert!(
            matches!(&dirty[1], Variant::Get { title, artist, .. } if title == "หนุ่มบ้านนอก" && artist == "SURIYA MQT")
        );
        assert!(
            matches!(&dirty[2], Variant::Search(p) if p[0] == ("track_name", "หนุ่มบ้านนอก".to_string()))
        );
        assert!(
            matches!(&dirty[3], Variant::Search(p) if p[0].0 == "q" && p[0].1 == "หนุ่มบ้านนอก SURIYA MQT")
        );
        let same = variants("Song", &artists(&["A", "B"]), None);
        assert!(same.len() <= MAX_REQUESTS);
        let mut seen = Vec::new();
        for v in &same {
            assert!(!seen.contains(v));
            seen.push(v.clone());
        }
    }
    #[test]
    fn negative_key_ignores_remix_suffix_and_buckets_duration_by_5s() {
        let a = artists(&["Artist"]);
        assert_eq!(
            negative_key("Song", &a, 180),
            negative_key("Song (feat. X)", &a, 184)
        );
        assert_ne!(negative_key("Song", &a, 180), negative_key("Song", &a, 185));
        assert_ne!(
            negative_key("Song", &a, 180),
            negative_key("Other", &a, 180)
        );
        assert_ne!(
            negative_key("Song", &a, 180),
            negative_key("Song", &artists(&["Z"]), 180)
        );
    }
    #[test]
    fn supports_lrc_only_records_without_exposing_timestamps_as_text() {
        assert_eq!(
            plain_lrc(
                "[ar:Artist]\n[offset:100]\n[00:01.00]First\n[00:02.5][00:03.00]Second\n[00:99]bad"
            ),
            "First\nSecond"
        );
        let mut candidate = record("Song", "Artist", 180.0);
        candidate.plain_lyrics = None;
        candidate.synced_lyrics = Some(
            "[00:01.0]Example lyric fixture line one\n[00:02.0]Example lyric fixture line two"
                .into(),
        );
        assert_eq!(
            usable(candidate).unwrap().text.as_deref(),
            Some("Example lyric fixture line one\nExample lyric fixture line two")
        );
    }
    #[test]
    fn placeholder_fragments_are_not_presented_as_complete_lyrics() {
        let candidate = record("Song", "Artist", 180.0);
        assert!(usable(candidate).is_none());
    }
    #[test]
    fn instrumental_is_not_a_missing_or_failed_lyric() {
        let mut candidate = record("Song", "Artist", 180.0);
        candidate.instrumental = true;
        let lyrics = usable(candidate).unwrap();
        assert!(lyrics.instrumental);
        assert!(lyrics.text.is_none());
    }
    fn starts(lines: &[TimedLine]) -> Vec<u32> {
        lines.iter().map(|l| l.start_ms).collect()
    }
    const FOUR: &str = "[00:01.00]First line of text\n[00:02.00]Second line of text\n[00:03.00]Third line of text\n[00:04.00]Fourth line here";
    #[test]
    fn lrc_parses_forms_sorts_and_skips_metadata() {
        let lines = parse_lrc(
            "[ar:Artist]\n[offset:100]\n[length:03:00]\n[01:05]Late line with some words\n[00:01.5][00:30.250]Repeated chorus line text\n[00:02.123]Three digit fraction line\njunk without tag\n[00:05.00]\n[00:06.00]Another sizeable lyric line",
        )
        .unwrap();
        assert_eq!(starts(&lines), [1500, 2123, 6000, 30250, 65000]);
        assert_eq!(lines[0].text, "Repeated chorus line text");
        assert_eq!(lines[3].text, lines[0].text);
    }
    #[test]
    fn lrc_rejects_bad_times_and_incomplete_sets() {
        for bad in [
            "[00:60.00]x",
            "[-1:00]x",
            "[00:NaN]x",
            "[00:inf]x",
            "[00:1.0]x",
            "[00:01.1234]x",
            "[99999:00]x",
            "[00:01:00]x",
        ] {
            assert!(time_tag(&bad[1..bad.find(']').unwrap()]).is_none(), "{bad}");
        }
        assert_eq!(time_tag("00:01"), Some(1000));
        assert_eq!(time_tag("00:01.5"), Some(1500));
        assert_eq!(time_tag("01:02.003"), Some(62003));
        assert!(
            parse_lrc("[00:01.00]Only one line of lyrics that is long enough to pass").is_none()
        );
        assert!(parse_lrc("[00:01]a\n[00:02]b\n[00:03]c\n[00:04]d").is_none());
        assert!(parse_lrc("[00:60.00]x\n[-1:00]y\n[00:NaN]z\n[00:inf]w").is_none());
        assert_eq!(parse_lrc(FOUR).unwrap().len(), 4);
    }
    #[test]
    fn lrc_bounds_line_count_text_and_tags() {
        let many: String = (0..1001)
            .map(|i| format!("[{:02}:{:02}.00]line number {i}\n", i / 60, i % 60))
            .collect();
        assert!(parse_lrc(&many).is_none());
        let ok: String = (0..1000)
            .map(|i| format!("[{:02}:{:02}.00]line number {i}\n", i / 60, i % 60))
            .collect();
        assert_eq!(parse_lrc(&ok).unwrap().len(), 1000);
        let long = format!(
            "[00:01]{}\n{}",
            "ก".repeat(500),
            &FOUR[FOUR.find('\n').unwrap() + 1..]
        );
        assert_eq!(parse_lrc(&long).unwrap()[0].text.chars().count(), 300);
        // Tags beyond the per-line cap end the tag run; the rest becomes text, never extra lines.
        let tags: String = (0..40).map(|i| format!("[00:{i:02}]")).collect();
        let parsed = parse_lrc(&format!("{tags}Words in the chorus\n{FOUR}")).unwrap();
        assert_eq!(parsed.len(), 4 + MAX_TAGS_PER_LINE);
    }
    #[test]
    fn lrc_drops_enhanced_word_tags() {
        let lines = parse_lrc(&format!(
            "[00:00.50]<00:00.50>Word <00:00.90>by <00:01.20>word text\n{FOUR}"
        ))
        .unwrap();
        assert_eq!(lines[0].text, "Word by word text");
    }
    #[test]
    fn synced_record_exposes_timed_lines_and_keeps_plain_text() {
        let mut candidate = record("Song", "Artist", 180.0);
        candidate.plain_lyrics =
            Some("Plain version of the lyrics, long enough to be complete.".into());
        candidate.synced_lyrics = Some(FOUR.into());
        let found = usable(candidate).unwrap();
        assert_eq!(found.timed.unwrap().len(), 4);
        assert!(found.text.unwrap().starts_with("Plain version"));
        let mut lrc_only = record("Song", "Artist", 180.0);
        lrc_only.plain_lyrics = None;
        lrc_only.synced_lyrics = Some(FOUR.into());
        let found = usable(lrc_only).unwrap();
        assert!(found.timed.is_some());
        assert!(
            found
                .text
                .unwrap()
                .starts_with("First line of text\nSecond")
        );
        // Timestamps past the track length: keep plain text, drop the timing.
        let mut wrong = record("Song", "Artist", 3.0);
        wrong.plain_lyrics =
            Some("Plain version of the lyrics, long enough to be complete.".into());
        wrong.synced_lyrics = Some(FOUR.replace("[00:04.00]", "[09:04.00]"));
        let found = usable(wrong).unwrap();
        assert!(found.timed.is_none());
        assert!(found.text.is_some());
    }
}
