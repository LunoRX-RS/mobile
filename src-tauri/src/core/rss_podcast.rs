//! Podcasts from their own RSS feeds. Shows are found through Apple's public iTunes Search API; episodes and their audio
//! files come from the show's feed, so a track is a plain `https` audio file the player reads like any other, with none
//! of a video site's ads or background-play limits. Nothing here talks to a music provider.
//!
//! Every address that reaches the network is checked by [`public_https`]: `https` only, a domain name rather than an
//! address, and no local-only names. Feeds and audio hosts are third parties, so redirects are checked hop by hop.
use crate::core::{errors::CoreError, playback::extract::ResolvedAudio};
use quick_xml::{Reader, events::Event};
use serde::Deserialize;
use std::time::Duration;

const MAX_FEED_BYTES: usize = 8 * 1024 * 1024;
const MAX_EPISODES: usize = 300;
const MAX_QUERY_CHARS: usize = 120;
// Download links pass through analytics hops (op3, podtrac, the host, a CDN) and end in long signed addresses.
const MAX_URL_CHARS: usize = 4096;
pub const MAX_REDIRECTS: usize = 10;
const TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_LIMIT: u32 = 25;

/// A show as found by search. `id` is the address of its RSS feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Show {
    pub id: String,
    pub title: String,
    pub author: String,
    pub artwork_url: Option<String>,
}

/// One episode of a show. `id` is the address of its audio file: that address is what identifies the track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Episode {
    pub id: String,
    pub title: String,
    pub show: String,
    pub published: Option<String>,
    pub duration_ms: Option<u64>,
    pub artwork_url: Option<String>,
}

/// The address as an `https` URL, or `None` when it must not be fetched. A plain `http` address is upgraded (nearly
/// every host serves both, and a phone should not listen in the clear); addresses with credentials, a port other than
/// 443, an IP literal or a local-only name are refused.
pub fn public_https(raw: &str) -> Option<reqwest::Url> {
    if raw.len() > MAX_URL_CHARS || raw.chars().any(|c| c.is_control() || c == ' ') {
        return None;
    }
    let mut url = reqwest::Url::parse(raw.trim()).ok()?;
    if url.scheme() == "http" {
        url.set_scheme("https").ok()?;
        if url.port().is_some() {
            return None;
        }
    }
    let host = match url.host()? {
        url::Host::Domain(d) => d.to_ascii_lowercase(),
        _ => return None,
    };
    let local = host == "localhost"
        || !host.contains('.')
        || ["local", "localhost", "internal", "lan", "home", "intranet"]
            .iter()
            .any(|s| host.ends_with(&format!(".{s}")));
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|p| p == 443)
        && !local)
        .then_some(url)
}

/// The same check for a redirect hop, which may legitimately name another host.
pub fn is_public_https(url: &reqwest::Url) -> bool {
    public_https(url.as_str()).is_some_and(|checked| checked.host_str() == url.host_str())
}

fn client() -> Result<reqwest::Client, CoreError> {
    reqwest::Client::builder()
        .user_agent("LunoRX/1.0 (podcast reader)")
        .connect_timeout(Duration::from_secs(8))
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < MAX_REDIRECTS && is_public_https(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|_| CoreError::ProviderUnavailable)
}

pub fn search_term(raw: &str) -> Result<String, CoreError> {
    let text = raw.trim();
    if text.is_empty()
        || text.chars().count() > MAX_QUERY_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(CoreError::Validation("podcast query".into()));
    }
    Ok(text.to_owned())
}

#[derive(Deserialize)]
struct ItunesAnswer {
    #[serde(default)]
    results: Vec<ItunesShow>,
}

#[derive(Deserialize)]
struct ItunesShow {
    #[serde(rename = "collectionId")]
    id: Option<u64>,
    #[serde(rename = "collectionName")]
    name: Option<String>,
    #[serde(rename = "artistName")]
    artist: Option<String>,
    #[serde(rename = "feedUrl")]
    feed: Option<String>,
    #[serde(rename = "artworkUrl600")]
    art: Option<String>,
}

/// Shows from one iTunes answer that have a usable feed, in the order Apple ranked them.
pub fn shows_from_answer(json: &str) -> Vec<(u64, Show)> {
    let Ok(answer) = serde_json::from_str::<ItunesAnswer>(json) else {
        return Vec::new();
    };
    answer
        .results
        .into_iter()
        .filter_map(|r| {
            let feed = public_https(r.feed.as_deref()?)?;
            let title = r.name.filter(|n| !n.trim().is_empty())?;
            Some((
                r.id?,
                Show {
                    id: feed.into(),
                    title,
                    author: r.artist.unwrap_or_default(),
                    artwork_url: r.art.and_then(|a| public_https(&a)).map(Into::into),
                },
            ))
        })
        .collect()
}

/// Shows for `query`. Two storefronts are asked together (the global one and Thailand's) because a show is listed
/// where it is published; their answers are merged without repeating a show.
pub async fn search_shows(query: &str) -> Result<Vec<Show>, CoreError> {
    let term = search_term(query)?;
    let client = client()?;
    let ask = |country: &'static str| {
        let client = client.clone();
        let term = term.clone();
        async move {
            let mut url = reqwest::Url::parse("https://itunes.apple.com/search").ok()?;
            url.query_pairs_mut()
                .append_pair("media", "podcast")
                .append_pair("entity", "podcast")
                .append_pair("limit", &SEARCH_LIMIT.to_string())
                .append_pair("country", country)
                .append_pair("term", &term);
            let body = limited_text(client.get(url).send().await.ok()?, MAX_FEED_BYTES)
                .await
                .ok()?;
            Some(shows_from_answer(&body))
        }
    };
    let (global, thai) = tokio::join!(ask("US"), ask("TH"));
    if global.is_none() && thai.is_none() {
        return Err(CoreError::ProviderUnavailable);
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let (mut a, mut b) = (
        global.unwrap_or_default().into_iter(),
        thai.unwrap_or_default().into_iter(),
    );
    // Interleaved, so neither storefront crowds the other out.
    loop {
        let pair = [a.next(), b.next()];
        if pair.iter().all(Option::is_none) {
            break;
        }
        for (id, show) in pair.into_iter().flatten() {
            if seen.insert(id) {
                out.push(show);
            }
        }
    }
    Ok(out)
}

async fn limited_text(mut response: reqwest::Response, limit: usize) -> Result<String, CoreError> {
    if !response.status().is_success() {
        return Err(CoreError::ProviderUnavailable);
    }
    let mut bytes = Vec::new();
    while let Some(part) = response
        .chunk()
        .await
        .map_err(|_| CoreError::ProviderUnavailable)?
    {
        if bytes.len() + part.len() > limit {
            return Err(CoreError::ProviderUnavailable);
        }
        bytes.extend_from_slice(&part);
    }
    String::from_utf8(bytes).map_err(|_| CoreError::ProviderUnavailable)
}

/// The episodes of the feed at `feed_url`, newest first as the feed lists them.
pub async fn episodes(feed_url: &str) -> Result<Vec<Episode>, CoreError> {
    let url = public_https(feed_url).ok_or_else(|| CoreError::Validation("feed address".into()))?;
    let response = client()?
        .get(url)
        .header(
            "accept",
            "application/rss+xml, application/xml, text/xml;q=0.9, */*;q=0.5",
        )
        .send()
        .await
        .map_err(|_| CoreError::ProviderUnavailable)?;
    let xml = limited_text(response, MAX_FEED_BYTES).await?;
    Ok(parse_feed(&xml))
}

/// What the player needs to stream an episode: the address it ends up at after redirects, and its size. The host must
/// answer byte-range requests, because that is how the player reads and seeks.
pub async fn open_audio(audio_url: &str) -> Result<ResolvedAudio, CoreError> {
    let url =
        public_https(audio_url).ok_or_else(|| CoreError::Validation("audio address".into()))?;
    let response = client()?
        .get(url)
        .header("range", "bytes=0-0")
        .send()
        .await
        .map_err(|_| CoreError::ProviderUnavailable)?;
    let landed = response.url().clone();
    if !is_public_https(&landed) {
        return Err(CoreError::ProviderUnavailable);
    }
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    let size = match response.status().as_u16() {
        206 => header("content-range")
            .and_then(|v| v.rsplit('/').next().and_then(|t| t.parse::<u64>().ok())),
        200 if header("accept-ranges").is_some_and(|v| v.eq_ignore_ascii_case("bytes")) => {
            header("content-length").and_then(|v| v.parse::<u64>().ok())
        }
        _ => None,
    };
    match size {
        Some(size) if size > 0 => Ok(ResolvedAudio {
            url: landed.into(),
            size,
        }),
        _ => Err(CoreError::UnsupportedCapability),
    }
}

/// `HH:MM:SS`, `MM:SS` or a bare number of seconds.
pub fn parse_duration(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    for part in &parts {
        let value = part.trim().split('.').next()?.parse::<u64>().ok()?;
        total = total.checked_mul(60)?.checked_add(value)?;
    }
    (total > 0).then(|| total.saturating_mul(1000))
}

#[derive(Default)]
struct Pending {
    title: String,
    published: String,
    duration: String,
    audio: Option<String>,
    art: Option<String>,
}

fn attr(e: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok())
        .map(|v| v.into_owned())
}

fn entity(name: &str, reference: Option<char>) -> Option<char> {
    reference.or(match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    })
}

/// The episodes of one RSS document. Items without a usable audio address are left out; a feed that is not XML gives
/// no episodes rather than an error, which the caller shows as "nothing to play".
pub fn parse_feed(xml: &str) -> Vec<Episode> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut show = String::new();
    let mut show_art: Option<String> = None;
    let mut item: Option<Pending> = None;
    let mut out: Vec<Episode> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_owned();
                if name == "item" {
                    item = Some(Pending::default());
                }
                text.clear();
                open_attrs(&name, &e, &stack, &mut item, &mut show_art);
                stack.push(name);
            }
            Ok(Event::Empty(e)) => {
                let name = e.name().as_ref().to_owned();
                open_attrs(&name, &e, &stack, &mut item, &mut show_art);
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml10_content()),
            Ok(Event::CData(c)) => text.push_str(&c.xml10_content()),
            Ok(Event::GeneralRef(r)) => {
                let name = r.xml10_content();
                if let Some(c) = entity(&name, r.resolve_char_ref().ok().flatten()) {
                    text.push(c);
                }
            }
            Ok(Event::End(_)) => {
                let Some(name) = stack.pop() else { continue };
                let value = text.trim().to_owned();
                text.clear();
                let parent = stack.last().map(String::as_str);
                if name == "item" {
                    if let Some(p) = item.take()
                        && let Some(audio) = p.audio.and_then(|a| public_https(&a))
                        && !p.title.is_empty()
                        && out.len() < MAX_EPISODES
                    {
                        out.push(Episode {
                            id: audio.into(),
                            title: p.title,
                            show: show.clone(),
                            published: (!p.published.is_empty()).then_some(p.published),
                            duration_ms: parse_duration(&p.duration),
                            artwork_url: p.art.and_then(|a| public_https(&a)).map(Into::into),
                        });
                    }
                } else if let Some(p) = item.as_mut() {
                    match name.as_str() {
                        "title" if parent == Some("item") => p.title = value,
                        "pubDate" => p.published = value,
                        "itunes:duration" => p.duration = value,
                        _ => {}
                    }
                } else if name == "title" && parent == Some("channel") {
                    show = value;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    for episode in &mut out {
        if episode.artwork_url.is_none() {
            episode.artwork_url = show_art
                .clone()
                .and_then(|a| public_https(&a))
                .map(Into::into);
        }
    }
    out
}

fn open_attrs(
    name: &str,
    e: &quick_xml::events::BytesStart<'_>,
    stack: &[String],
    item: &mut Option<Pending>,
    show_art: &mut Option<String>,
) {
    match (name, item.as_mut()) {
        ("enclosure", Some(p)) => {
            let kind = attr(e, "type").unwrap_or_default().to_ascii_lowercase();
            if kind.is_empty() || kind.starts_with("audio/") || kind == "application/octet-stream" {
                p.audio = attr(e, "url");
            }
        }
        ("itunes:image", Some(p)) => p.art = attr(e, "href"),
        ("itunes:image", None) if stack.last().map(String::as_str) == Some("channel") => {
            *show_art = attr(e, "href");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
<title>The Show &amp; Friends</title>
<itunes:image href="https://cdn.example.com/show.jpg"/>
<item><title><![CDATA[Ep 2: "Quotes" & more]]></title><guid>g2</guid><pubDate>Fri, 24 Oct 2026 10:00:00 +0000</pubDate>
<itunes:duration>01:02:03</itunes:duration><itunes:image href="https://cdn.example.com/e2.jpg"/>
<enclosure url="http://media.example.com/ep2.mp3?x=1" length="1" type="audio/mpeg"/></item>
<item><title>Ep 1</title><enclosure url="https://media.example.com/ep1.mp3" type="audio/mpeg"/><itunes:duration>754</itunes:duration></item>
<item><title>Video only</title><enclosure url="https://media.example.com/v.mp4" type="video/mp4"/></item>
<item><title>Private host</title><enclosure url="https://192.168.0.5/a.mp3" type="audio/mpeg"/></item>
<item><title></title><enclosure url="https://media.example.com/none.mp3" type="audio/mpeg"/></item>
</channel></rss>"#;

    #[test]
    fn episodes_come_from_enclosures_with_clean_text() {
        let list = parse_feed(FEED);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].title, "Ep 2: \"Quotes\" & more");
        assert_eq!(list[0].show, "The Show & Friends");
        assert_eq!(
            list[0].id, "https://media.example.com/ep2.mp3?x=1",
            "http is upgraded"
        );
        assert_eq!(list[0].duration_ms, Some(3_723_000));
        assert_eq!(
            list[0].artwork_url.as_deref(),
            Some("https://cdn.example.com/e2.jpg")
        );
        assert_eq!(list[1].duration_ms, Some(754_000));
        assert_eq!(
            list[1].artwork_url.as_deref(),
            Some("https://cdn.example.com/show.jpg"),
            "falls back to the show art"
        );
    }

    #[test]
    fn a_document_that_is_not_a_feed_has_no_episodes() {
        assert!(parse_feed("<html><body>nope").is_empty());
        assert!(parse_feed("").is_empty());
    }

    #[test]
    fn durations_read_clock_and_seconds_forms() {
        assert_eq!(parse_duration("1:02:03"), Some(3_723_000));
        assert_eq!(parse_duration("45:10"), Some(2_710_000));
        assert_eq!(parse_duration("90"), Some(90_000));
        assert_eq!(parse_duration("12.5"), Some(12_000));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("a:b"), None);
        assert_eq!(parse_duration("1:2:3:4"), None);
    }

    #[test]
    fn only_public_https_names_may_be_fetched() {
        assert!(public_https("https://feeds.example.com/rss").is_some());
        assert_eq!(
            public_https("http://feeds.example.com/rss")
                .unwrap()
                .scheme(),
            "https"
        );
        for bad in [
            "",
            "ftp://example.com/a",
            "https://localhost/a",
            "https://127.0.0.1/a",
            "https://[::1]/a",
            "https://10.0.0.1/a",
            "https://printer.local/a",
            "https://intranet/a",
            "https://user:pw@example.com/a",
            "https://example.com:8443/a",
            "http://example.com:8080/a",
            "https://example.com/has space",
            "file:///etc/passwd",
        ] {
            assert!(public_https(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn itunes_answers_keep_shows_with_a_usable_feed() {
        let json = r#"{"resultCount":3,"results":[
          {"collectionId":1,"collectionName":"A","artistName":"X","feedUrl":"http://feeds.example.com/a","artworkUrl600":"https://img.example.com/a.jpg"},
          {"collectionId":2,"collectionName":"No feed"},
          {"collectionId":3,"collectionName":"Local","feedUrl":"https://192.168.1.2/f"}]}"#;
        let shows = shows_from_answer(json);
        assert_eq!(shows.len(), 1);
        assert_eq!(shows[0].1.id, "https://feeds.example.com/a");
        assert_eq!(shows[0].1.author, "X");
        assert!(shows_from_answer("not json").is_empty());
    }

    #[test]
    fn search_terms_are_bounded() {
        assert_eq!(search_term(" ghost radio ").unwrap(), "ghost radio");
        assert!(search_term("  ").is_err());
        assert!(search_term("a\nb").is_err());
        assert!(search_term(&"x".repeat(MAX_QUERY_CHARS + 1)).is_err());
    }
}
