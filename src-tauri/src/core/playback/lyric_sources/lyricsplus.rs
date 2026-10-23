//! LyricsPlus (YouLy+ backend, https://github.com/ibratabian17/LyricsPlus): `GET /v2/lyrics/get`.
//! Answers a direct lookup with syllable-level lyrics aggregated from other services. No key.
use super::{Candidate, Fail, Http, Query, SourceLyrics, checked, json, line, plain, word};
use crate::core::playback::lyrics::TimedLine;
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Meta {
    title: String,
    artist: String,
    total_duration: Option<String>,
}

#[derive(Deserialize)]
#[serde(default)]
struct Syllable {
    time: f64,
    duration: f64,
    text: String,
    #[serde(rename = "isBackground")]
    background: bool,
}

impl Default for Syllable {
    fn default() -> Self {
        Syllable {
            time: -1.0,
            duration: 0.0,
            text: String::new(),
            background: false,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Row {
    time: f64,
    text: String,
    syllabus: Vec<Syllable>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Response {
    metadata: Meta,
    lyrics: Vec<Row>,
}

fn ms(value: f64) -> Option<u64> {
    (value.is_finite() && (0.0..=86_400_000.0).contains(&value)).then_some(value as u64)
}

/// `m:ss.mmm` or `h:mm:ss.mmm` to milliseconds.
pub(super) fn clock_ms(value: &str) -> Option<u64> {
    let mut total = 0f64;
    for part in value.trim().split(':') {
        total = total * 60.0 + part.parse::<f64>().ok().filter(|n| *n >= 0.0)?;
    }
    ms(total * 1000.0)
}

type Parsed = (String, String, Option<u32>, Vec<TimedLine>);

/// (title, artist, length in ms, lines). Err = malformed or over a bound.
pub(super) fn parse(bytes: &[u8]) -> Result<Parsed, Fail> {
    let response: Response = json(bytes)?;
    let mut lines = Vec::new();
    for row in &response.lyrics {
        let start = ms(row.time).ok_or(Fail::Failed)?;
        if row.syllabus.is_empty() {
            lines.extend(line(start, &row.text, None));
            continue;
        }
        // Background vocals overlap the lead line and are left out of both words and line text.
        let mut words = Vec::new();
        for s in row.syllabus.iter().filter(|s| !s.background) {
            let begin = ms(s.time).ok_or(Fail::Failed)?;
            let end = ms(s.time + s.duration.max(0.0)).ok_or(Fail::Failed)?;
            words.extend(word(begin, end, &s.text));
        }
        if words.is_empty() {
            continue;
        }
        let text: String = words.iter().map(|w| w.text.as_str()).collect();
        lines.extend(line(start, &text, Some(words)));
    }
    let length = response
        .metadata
        .total_duration
        .as_deref()
        .and_then(clock_ms)
        .and_then(|v| u32::try_from(v).ok());
    Ok((
        response.metadata.title,
        response.metadata.artist,
        length,
        checked(lines)?,
    ))
}

pub(super) async fn search(
    http: &Http,
    query: &Query,
    text: Option<&str>,
) -> Result<Vec<Candidate>, Fail> {
    // No title search endpoint is used: a free-text query has nothing to ask.
    if text.is_some() {
        return Ok(Vec::new());
    }
    let (title, duration) = (query.clean_title(), query.duration_ms.to_string());
    let mut params = vec![
        ("title", title.as_str()),
        ("artist", query.first_artist()),
        ("duration", duration.as_str()),
    ];
    if let Some(album) = query.album.as_deref().filter(|a| !a.is_empty()) {
        params.push(("album", album));
    }
    let body = http.get("/v2/lyrics/get", &params).await?;
    let (title, artist, length, lines) = parse(&body)?;
    if lines.is_empty() {
        return Err(Fail::NotFound);
    }
    let words = lines.iter().any(|l| l.words.is_some());
    Ok(vec![Candidate {
        key: "get".into(),
        title: if title.is_empty() {
            query.title.clone()
        } else {
            title
        },
        artist: if artist.is_empty() {
            query.first_artist().to_owned()
        } else {
            artist
        },
        duration_ms: length.unwrap_or(query.duration_ms),
        synced: true,
        words,
        prefetched: Some(SourceLyrics {
            text: Some(plain(&lines)),
            lines: Some(lines),
            instrumental: false,
        }),
    }])
}
