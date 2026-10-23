//! QQ Music public web endpoints. Search: `GET c.y.qq.com/soso/fcgi-bin/search_for_qq_cp` (JSON).
//! Lyric: `GET /lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid=` with a base64 plain-LRC body. The
//! constant `Referer: https://y.qq.com/` is required by the service. The encrypted QRC format is
//! not used.
use super::{
    Candidate, Fail, Http, Query, SourceLyrics, decode_b64, join_names, json, lrc_lines, plain,
};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Singer {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Song {
    songmid: String,
    songname: String,
    singer: Vec<Singer>,
    interval: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Songs {
    list: Vec<Song>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Data {
    song: Songs,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SearchResponse {
    data: Data,
}

pub(super) fn parse_search(bytes: &[u8]) -> Result<Vec<Candidate>, Fail> {
    let response: SearchResponse = json(bytes)?;
    Ok(response
        .data
        .song
        .list
        .into_iter()
        .filter(|s| !s.songmid.is_empty() && !s.songname.is_empty())
        .take(10)
        .map(|s| Candidate {
            key: s.songmid,
            title: s.songname,
            artist: join_names(s.singer.iter().map(|a| a.name.as_str())),
            duration_ms: u32::try_from(s.interval.saturating_mul(1000)).unwrap_or(0),
            synced: true,
            words: false,
            prefetched: None,
        })
        .collect())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct LyricResponse {
    retcode: i64,
    code: i64,
    lyric: String,
}

pub(super) fn parse_lyric(bytes: &[u8]) -> Result<SourceLyrics, Fail> {
    let response: LyricResponse = json(bytes)?;
    if response.retcode != 0 || response.code != 0 {
        return Err(Fail::Failed);
    }
    if response.lyric.trim().is_empty() {
        return Ok(SourceLyrics::default());
    }
    let lines = lrc_lines(&decode_b64(&response.lyric)?)?;
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
            "/soso/fcgi-bin/search_for_qq_cp",
            &[
                ("w", keyword.as_str()),
                ("format", "json"),
                ("n", "10"),
                ("p", "1"),
            ],
        )
        .await?;
    parse_search(&body)
}

pub(super) async fn fetch(http: &Http, key: &str) -> Result<SourceLyrics, Fail> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Fail::Failed);
    }
    let body = http
        .get(
            "/lyric/fcgi-bin/fcg_query_lyric_new.fcg",
            &[("songmid", key), ("format", "json")],
        )
        .await?;
    parse_lyric(&body)
}
