//! Kugou lyric service on one host: `GET /search?keyword=&duration=` lists candidates (id +
//! accesskey), `GET /download?id=&accesskey=&fmt=lrc` returns a base64 plain-LRC body. The encrypted
//! KRC format is not used.
use super::{Candidate, Fail, Http, Query, SourceLyrics, decode_b64, json, lrc_lines, plain};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Item {
    id: String,
    accesskey: String,
    song: String,
    singer: String,
    duration: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SearchResponse {
    candidates: Vec<Item>,
}

pub(super) fn parse_search(bytes: &[u8]) -> Result<Vec<Candidate>, Fail> {
    let response: SearchResponse = json(bytes)?;
    Ok(response
        .candidates
        .into_iter()
        .filter(|c| !c.id.is_empty() && !c.accesskey.is_empty() && !c.song.is_empty())
        .take(10)
        .map(|c| Candidate {
            key: format!("{}:{}", c.id, c.accesskey),
            title: c.song,
            artist: c.singer,
            duration_ms: u32::try_from(c.duration).unwrap_or(0),
            synced: true,
            words: false,
            prefetched: None,
        })
        .collect())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DownloadResponse {
    status: i64,
    content: String,
}

pub(super) fn parse_download(bytes: &[u8]) -> Result<SourceLyrics, Fail> {
    let response: DownloadResponse = json(bytes)?;
    if response.status != 200 {
        return Err(Fail::NotFound);
    }
    let lines = lrc_lines(&decode_b64(&response.content)?)?;
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
        || format!("{} - {}", query.first_artist(), query.clean_title()),
        str::to_owned,
    );
    let duration = if text.is_some() {
        String::new()
    } else {
        query.duration_ms.to_string()
    };
    let mut params = vec![
        ("ver", "1"),
        ("man", "yes"),
        ("client", "pc"),
        ("keyword", keyword.as_str()),
    ];
    if !duration.is_empty() {
        params.push(("duration", duration.as_str()));
    }
    let body = http.get("/search", &params).await?;
    parse_search(&body)
}

pub(super) async fn fetch(http: &Http, key: &str) -> Result<SourceLyrics, Fail> {
    let (id, access) = key.split_once(':').ok_or(Fail::Failed)?;
    if id.is_empty()
        || !id.bytes().all(|b| b.is_ascii_digit())
        || access.is_empty()
        || !access.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return Err(Fail::Failed);
    }
    let body = http
        .get(
            "/download",
            &[
                ("ver", "1"),
                ("client", "pc"),
                ("id", id),
                ("accesskey", access),
                ("fmt", "lrc"),
                ("charset", "utf8"),
            ],
        )
        .await?;
    parse_download(&body)
}
