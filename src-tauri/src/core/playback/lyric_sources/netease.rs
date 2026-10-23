//! Netease Cloud Music public web endpoints (no login, no cookie):
//! `GET /api/search/get?s=&type=1` and `GET /api/song/lyric/v1?id=&lv=1&kv=1&tv=-1&yv=1`.
//! `lrc` is plain LRC; `yrc` (when present) carries real word timings:
//! `[startMs,durMs](startMs,durMs,0)word(startMs,durMs,0)word...`.
use super::{
    Candidate, Fail, Http, Query, SourceLyrics, checked, join_names, json, line, lrc_lines, plain,
    strip_credits, unescape, word,
};
use crate::core::playback::lyrics::{TimedLine, TimedWord};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Artist {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Song {
    id: u64,
    name: String,
    artists: Vec<Artist>,
    duration: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SearchResult {
    songs: Vec<Song>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SearchResponse {
    result: SearchResult,
}

pub(super) fn parse_search(bytes: &[u8]) -> Result<Vec<Candidate>, Fail> {
    let response: SearchResponse = json(bytes)?;
    Ok(response
        .result
        .songs
        .into_iter()
        .filter(|s| s.id > 0 && !s.name.is_empty())
        .take(10)
        .map(|s| Candidate {
            key: s.id.to_string(),
            title: s.name,
            artist: join_names(s.artists.iter().map(|a| a.name.as_str())),
            duration_ms: u32::try_from(s.duration).unwrap_or(0),
            synced: true,
            words: false,
            prefetched: None,
        })
        .collect())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Block {
    lyric: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LyricResponse {
    nolyric: bool,
    lrc: Block,
    yrc: Block,
}

/// `(start,duration,flag)` tag at the start of `s`: (start, duration, byte length).
fn word_tag(s: &str) -> Option<(u64, u64, usize)> {
    let inner = s.strip_prefix('(')?;
    let end = inner.find(')')?;
    let mut parts = inner[..end].split(',');
    let (a, b, c) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || ![a, b, c]
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|x| x.is_ascii_digit()))
    {
        return None;
    }
    Some((a.parse().ok()?, b.parse().ok()?, end + 2))
}

/// Word-level lines. Lines that do not start with a `[start,duration]` tag (the JSON credit
/// lines) are skipped. Err = over a bound or out-of-range times.
pub(super) fn parse_yrc(text: &str) -> Result<Vec<TimedLine>, Fail> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Some(body) = raw.trim().strip_prefix('[') else {
            continue;
        };
        let Some((head, mut rest)) = body.split_once(']') else {
            continue;
        };
        let Some((start, _)) = head.split_once(',') else {
            continue;
        };
        let Ok(start) = start.parse::<u64>() else {
            continue;
        };
        let mut words: Vec<TimedWord> = Vec::new();
        while let Some((begin, duration, used)) = word_tag(rest) {
            rest = &rest[used..];
            let mut end = rest.len();
            for (at, _) in rest.match_indices('(') {
                if word_tag(&rest[at..]).is_some() {
                    end = at;
                    break;
                }
            }
            let piece = unescape(&rest[..end]);
            rest = &rest[end..];
            if piece.trim().is_empty() {
                // A bare space after a word is the word boundary.
                if let Some(last) = words.last_mut()
                    && !last.text.ends_with(' ')
                {
                    last.text.push(' ');
                }
                continue;
            }
            words.extend(word(begin, begin + duration, &piece));
        }
        if words.is_empty() {
            continue;
        }
        let text: String = words.iter().map(|w| w.text.as_str()).collect();
        lines.extend(line(start, &text, Some(words)));
    }
    let mut lines = checked(lines)?;
    strip_credits(&mut lines);
    Ok(lines)
}

pub(super) fn parse_lyric(bytes: &[u8]) -> Result<SourceLyrics, Fail> {
    let response: LyricResponse = json(bytes)?;
    if response.nolyric {
        return Ok(SourceLyrics {
            instrumental: true,
            ..SourceLyrics::default()
        });
    }
    let word_lines = if response.yrc.lyric.trim().is_empty() {
        Vec::new()
    } else {
        parse_yrc(&response.yrc.lyric)?
    };
    let lines = if word_lines.len() >= 4 {
        word_lines
    } else {
        lrc_lines(&response.lrc.lyric)?
    };
    Ok(SourceLyrics {
        text: (!lines.is_empty()).then(|| plain(&lines)),
        lines: (!lines.is_empty()).then_some(lines),
        instrumental: false,
    })
}

pub(super) async fn search(
    http: &Http,
    query: &Query,
    text: Option<&str>,
) -> Result<Vec<Candidate>, Fail> {
    let keyword = text.map_or_else(
        || format!("{} {}", query.clean_title(), query.first_artist()),
        str::to_owned,
    );
    let body = http
        .get(
            "/api/search/get",
            &[
                ("s", keyword.as_str()),
                ("type", "1"),
                ("limit", "10"),
                ("offset", "0"),
            ],
        )
        .await?;
    parse_search(&body)
}

pub(super) async fn fetch(http: &Http, key: &str) -> Result<SourceLyrics, Fail> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Fail::Failed);
    }
    let body = http
        .get(
            "/api/song/lyric/v1",
            &[
                ("id", key),
                ("lv", "1"),
                ("kv", "1"),
                ("tv", "-1"),
                ("yv", "1"),
            ],
        )
        .await?;
    parse_lyric(&body)
}
