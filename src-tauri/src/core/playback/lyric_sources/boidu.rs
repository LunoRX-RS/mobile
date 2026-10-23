//! Boidu / BetterLyrics API (https://docs.betterlyrics.org): `GET /getLyrics?s=&a=&d=` returns
//! `{"ttml": "<tt ...>"}` with word-level Apple-style TTML. Without an API key only tracks the
//! service already has cached are answered; an uncached query is HTTP 401 and is reported as no
//! match (this app never sends a key). The response has no title or artist: only the length is checked.
use super::{
    Candidate, Fail, Http, Query, SourceLyrics, checked, json, line, lyricsplus, plain, unescape,
    word,
};
use crate::core::playback::lyrics::{TimedLine, TimedWord};
use serde::Deserialize;

#[derive(Deserialize)]
struct Response {
    ttml: String,
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(at) = tag[from..].find(name) {
        let start = from + at;
        let before_ok = tag[..start]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace);
        let after = &tag[start + name.len()..];
        if before_ok && let Some(rest) = after.strip_prefix('=') {
            let quote = rest.chars().next().filter(|q| *q == '"' || *q == '\'')?;
            let value = &rest[1..];
            return value.find(quote).map(|end| &value[..end]);
        }
        from = start + name.len();
    }
    None
}

/// `12.345`, `12.5s`, `1:02.345`, `00:01:02.345` to milliseconds.
fn time(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(ms) = value.strip_suffix("ms") {
        return ms
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|v| v as u64);
    }
    let value = value.strip_suffix('s').unwrap_or(value);
    if value.contains(':') {
        lyricsplus::clock_ms(value)
    } else {
        value
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| (n * 1000.0).round() as u64)
    }
}

struct Open {
    word: Option<(u64, u64, String)>,
    background: bool,
}

/// (lines, body duration ms). Err = malformed, over a bound or out-of-range times. Background
/// vocals (`x-bg`) are left out. Text between spans marks a word boundary.
pub(super) fn parse_ttml(ttml: &str) -> Result<(Vec<TimedLine>, Option<u32>), Fail> {
    let mut lines = Vec::new();
    let mut length = None;
    let mut rest = ttml;
    // Current paragraph: start, words, direct text (for paragraphs without spans).
    let mut paragraph: Option<(u64, Vec<TimedWord>, String)> = None;
    let mut spans: Vec<Open> = Vec::new();
    while let Some(open) = rest.find('<') {
        let text = &rest[..open];
        let Some(close) = rest[open..].find('>') else {
            return Err(Fail::Failed);
        };
        let tag = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        if let Some((_, words, direct)) = paragraph.as_mut() {
            let decoded = unescape(text);
            match spans.last_mut() {
                Some(Open {
                    word: Some(w),
                    background: false,
                }) => w.2.push_str(&decoded),
                Some(Open {
                    background: true, ..
                }) => {}
                _ => {
                    if decoded.chars().any(char::is_whitespace)
                        && let Some(last) = words.last_mut()
                        && !last.text.ends_with(' ')
                    {
                        last.text.push(' ');
                    }
                    if spans.is_empty() {
                        direct.push_str(&decoded);
                    }
                }
            }
        }
        let name = tag.split_whitespace().next().unwrap_or("");
        match name {
            "body" => {
                length = attr(tag, "dur")
                    .and_then(time)
                    .and_then(|v| u32::try_from(v).ok());
            }
            "p" if !tag.ends_with('/') => {
                let start = attr(tag, "begin").and_then(time).ok_or(Fail::Failed)?;
                paragraph = Some((start, Vec::new(), String::new()));
            }
            "/p" => {
                if let Some((start, words, direct)) = paragraph.take() {
                    let joined: String = words.iter().map(|w| w.text.as_str()).collect();
                    let text = if words.is_empty() { direct } else { joined };
                    let words = (!words.is_empty()).then_some(words);
                    lines.extend(line(start, &text, words));
                }
                spans.clear();
            }
            "span" if paragraph.is_some() && !tag.ends_with('/') => {
                let background = tag.contains("x-bg") || spans.last().is_some_and(|s| s.background);
                let word = match (
                    attr(tag, "begin").and_then(time),
                    attr(tag, "end").and_then(time),
                ) {
                    (Some(b), Some(e)) => Some((b, e, String::new())),
                    _ => None,
                };
                spans.push(Open { word, background });
            }
            "/span" => {
                if let Some(Open {
                    word: Some((b, e, t)),
                    background: false,
                }) = spans.pop()
                    && let Some((_, words, _)) = paragraph.as_mut()
                {
                    words.extend(word(b, e, &t));
                }
            }
            _ => {}
        }
    }
    Ok((checked(lines)?, length))
}

pub(super) async fn search(
    http: &Http,
    query: &Query,
    text: Option<&str>,
) -> Result<Vec<Candidate>, Fail> {
    if text.is_some() {
        return Ok(Vec::new());
    }
    let (title, secs) = (query.clean_title(), query.secs().to_string());
    let body = http
        .get_status(
            "/getLyrics",
            &[
                ("s", title.as_str()),
                ("a", query.first_artist()),
                ("d", secs.as_str()),
            ],
        )
        .await
        // 401: this app has no key, and the service answers only cached tracks without one.
        .map_err(|(fail, code)| if code == 401 { Fail::NotFound } else { fail })?;
    let response: Response = json(&body)?;
    let (lines, length) = parse_ttml(&response.ttml)?;
    if lines.is_empty() {
        return Err(Fail::NotFound);
    }
    let words = lines.iter().any(|l| l.words.is_some());
    Ok(vec![Candidate {
        key: "getLyrics".into(),
        title: query.title.clone(),
        artist: query.first_artist().to_owned(),
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
