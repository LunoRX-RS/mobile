//! No network: parsers run on synthetic fixtures (never real lyrics), flows run against a local mock.
use super::*;
use base64::Engine;
use serde_json::json;
use std::sync::{Arc, Mutex};

const LONG: [&str; 5] = [
    "Fixture lyric line one",
    "Fixture lyric line two",
    "Fixture lyric line three",
    "Fixture lyric line four",
    "Fixture lyric line five",
];

fn lrc(lines: &[&str]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, t)| format!("[00:{:02}.50]{t}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn query() -> Query {
    Query {
        title: "Song".into(),
        artists: vec!["Artist".into()],
        album: None,
        duration_ms: 180_000,
    }
}

// ---------- mock server ----------

struct Mock {
    base: String,
    seen: Arc<Mutex<Vec<String>>>,
    heads: Arc<Mutex<Vec<String>>>,
    leaked: Arc<AtomicBool>,
    peak: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}
use std::sync::atomic::AtomicBool;

impl Mock {
    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
    fn count(&self, prefix: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.starts_with(prefix))
            .count()
    }
    fn stop(self) {
        self.task.abort();
        assert!(
            !self.leaked.load(Ordering::SeqCst),
            "cookie or authorization header sent"
        );
    }
    fn ctx(&self) -> Ctx {
        Ctx {
            base_override: Some(self.base.clone()),
            request_timeout: Some(Duration::from_secs(2)),
            ..Ctx::default()
        }
    }
}

async fn mock(
    delay_ms: u64,
    handler: impl Fn(&str) -> (&'static str, String) + Send + Sync + 'static,
) -> Mock {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (seen, heads) = (
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
    );
    let (leaked, peak, now) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
    );
    let handler = Arc::new(handler);
    let (s, h, l, p) = (seen.clone(), heads.clone(), leaked.clone(), peak.clone());
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (handler, s, h, l, p, now) = (
                handler.clone(),
                s.clone(),
                h.clone(),
                l.clone(),
                p.clone(),
                now.clone(),
            );
            tokio::spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    let Ok(b) = socket.read_u8().await else {
                        return;
                    };
                    header.push(b);
                }
                let header = String::from_utf8_lossy(&header).into_owned();
                let lower = header.to_lowercase();
                if lower.contains("\r\ncookie:") || lower.contains("\r\nauthorization:") {
                    l.store(true, Ordering::SeqCst);
                }
                let target = header.split_whitespace().nth(1).unwrap_or("").to_owned();
                let inflight = now.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(inflight, Ordering::SeqCst);
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                let (status, body) = handler(&target);
                s.lock().unwrap().push(target);
                h.lock().unwrap().push(lower);
                now.fetch_sub(1, Ordering::SeqCst);
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
        heads,
        leaked,
        peak,
        task,
    }
}

fn lp_body(title: &str, rows: usize) -> String {
    let lyrics: Vec<_> = (0..rows)
        .map(|i| {
            json!({"time": 1000 + i * 1000, "duration": 900, "text": LONG[i % 5],
                   "syllabus": [{"time": 1000 + i * 1000, "duration": 400, "text": "Fixture "},
                                {"time": 1400 + i * 1000, "duration": 400, "text": "lyric"}]})
        })
        .collect();
    json!({"type":"Word","metadata":{"title":title,"artist":"Artist","totalDuration":"3:00.000"},"lyrics":lyrics})
        .to_string()
}

fn ne_search(songs: &[(u64, &str, &str, u64)]) -> String {
    let songs: Vec<_> = songs
        .iter()
        .map(|(id, name, artist, dur)| {
            json!({"id": id, "name": name, "artists": [{"name": artist}], "duration": dur})
        })
        .collect();
    json!({"result": {"songs": songs}, "code": 200}).to_string()
}

fn ne_lyric() -> String {
    json!({"lrc": {"lyric": lrc(&LONG)}, "code": 200}).to_string()
}

// ---------- parsing ----------

#[test]
fn lyricsplus_keeps_real_word_times_and_drops_background_vocals() {
    let body = json!({"type":"Word","metadata":{"title":"Song","artist":"Artist","totalDuration":"3:21.570"},
        "lyrics":[{"time":2000,"duration":900,"text":"lead back","syllabus":[
            {"time":2000,"duration":300,"text":"lead "},
            {"time":2400,"duration":300,"text":"(back)","isBackground":true},
            {"time":2800,"duration":100,"text":"end"}]},
          {"time":1000,"duration":500,"text":"plain line","syllabus":[]}]})
    .to_string();
    let (title, artist, length, lines) = lyricsplus::parse(body.as_bytes()).unwrap();
    assert_eq!(
        (title.as_str(), artist.as_str(), length),
        ("Song", "Artist", Some(201_570))
    );
    // Sorted by start; the line without syllables has no words.
    assert_eq!(lines[0].start_ms, 1000);
    assert!(lines[0].words.is_none());
    let words = lines[1].words.as_ref().unwrap();
    assert_eq!(lines[1].text, "lead end");
    assert_eq!(
        words
            .iter()
            .map(|w| (w.start_ms, w.end_ms))
            .collect::<Vec<_>>(),
        [(2000, 2300), (2800, 2900)]
    );
}

#[test]
fn ttml_words_gaps_background_entities_and_clock_times() {
    let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body dur="3:21.570"><div>
<p begin="1.5" end="3.0"><span begin="1.5" end="2.0">Blin</span><span begin="2.0" end="2.5">ding</span> <span begin="2.5" end="3.0">R&amp;B</span><span ttm:role="x-bg"><span begin="2.0" end="2.2">(oh)</span></span></p>
<p begin="0:04.000" end="0:05.000">Plain &apos;para&apos;</p>
</div></body></tt>"#;
    let (lines, length) = boidu::parse_ttml(ttml).unwrap();
    assert_eq!(length, Some(201_570));
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].start_ms, 1500);
    assert_eq!(lines[0].text, "Blinding R&B");
    let words = lines[0].words.as_ref().unwrap();
    assert_eq!(words.len(), 3);
    assert_eq!(words[1].text, "ding ");
    assert_eq!((words[2].start_ms, words[2].end_ms), (2500, 3000));
    assert_eq!(lines[1].text, "Plain 'para'");
    assert!(lines[1].words.is_none());
    assert!(boidu::parse_ttml("<p begin=\"x\">").is_err());
}

#[test]
fn yrc_parses_word_tags_skips_json_credit_lines_and_keeps_parentheses_in_text() {
    let text = "{\"t\":0,\"c\":[{\"tx\":\"credit\"}]}\n[1000,2000](1000,500,0)Hel(1500,500,0)lo (2000,900,0)(oh)\n[bad\n[4000,1000](4000,1000,0)Solo";
    let lines = netease::parse_yrc(text).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].text, "Hello (oh)");
    let words = lines[0].words.as_ref().unwrap();
    assert_eq!(
        words.iter().map(|w| w.start_ms).collect::<Vec<_>>(),
        [1000, 1500, 2000]
    );
    assert_eq!(words[1].text, "lo ");
    assert_eq!(words[2].text, "(oh)");
}

#[test]
fn word_and_line_bounds_make_a_malformed_response() {
    // More than 1000 lines.
    assert_eq!(
        lyricsplus::parse(lp_body("Song", 1001).as_bytes()).unwrap_err(),
        Fail::Failed
    );
    assert!(lyricsplus::parse(lp_body("Song", 1000).as_bytes()).is_ok());
    // More than 80 words in a line.
    let syllables: Vec<_> = (0..81)
        .map(|i| json!({"time": 1000 + i * 10, "duration": 5, "text": "a "}))
        .collect();
    let body = json!({"lyrics":[{"time":1000,"text":"x","syllabus":syllables}]}).to_string();
    assert_eq!(
        lyricsplus::parse(body.as_bytes()).unwrap_err(),
        Fail::Failed
    );
    // Word starts going backwards.
    let body = json!({"lyrics":[{"time":1000,"text":"x","syllabus":[
        {"time":2000,"duration":5,"text":"a "},{"time":1500,"duration":5,"text":"b"}]}]})
    .to_string();
    assert_eq!(
        lyricsplus::parse(body.as_bytes()).unwrap_err(),
        Fail::Failed
    );
    // Past 24 h, and negative.
    for time in [86_400_001_u64, 99_999_999_999] {
        let body = json!({"lyrics":[{"time":time,"text":"x","syllabus":[]}]}).to_string();
        assert_eq!(
            lyricsplus::parse(body.as_bytes()).unwrap_err(),
            Fail::Failed
        );
    }
    let body = json!({"lyrics":[{"time":-5,"text":"x","syllabus":[]}]}).to_string();
    assert_eq!(
        lyricsplus::parse(body.as_bytes()).unwrap_err(),
        Fail::Failed
    );
    assert!(
        netease::parse_yrc("[100000000,5](100000000,5,0)late")
            .unwrap()
            .is_empty()
    );
    let many = (0..81)
        .map(|i| format!("({},5,0)w", i * 10))
        .collect::<String>();
    assert_eq!(
        netease::parse_yrc(&format!("[0,900]{many}")).unwrap_err(),
        Fail::Failed
    );
}

#[test]
fn qq_kugou_and_netease_responses_parse() {
    let body = json!({"retcode":0,"code":0,"lyric": b64(&format!("[ti:Song]\n[ar:Artist]\n{}\n[00:09.00]It&apos;s fine", lrc(&LONG)))})
        .to_string();
    let found = qq::parse_lyric(body.as_bytes()).unwrap();
    let lines = found.lines.unwrap();
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[5].text, "It's fine");
    assert!(lines.iter().all(|l| l.words.is_none()));
    assert_eq!(
        qq::parse_lyric(br#"{"retcode":0,"code":0,"lyric":""}"#).unwrap(),
        SourceLyrics::default()
    );
    assert_eq!(
        qq::parse_lyric(br#"{"retcode":-1,"code":0,"lyric":""}"#).unwrap_err(),
        Fail::Failed
    );
    assert_eq!(
        qq::parse_lyric(br#"{"retcode":0,"code":0,"lyric":"!!"}"#).unwrap_err(),
        Fail::Failed
    );

    let search = json!({"data":{"song":{"list":[{"songmid":"003JdsAh","songname":"Song","singer":[{"name":"A"},{"name":"B"}],"interval":181}]}}}).to_string();
    let c = &qq::parse_search(search.as_bytes()).unwrap()[0];
    assert_eq!(
        (c.key.as_str(), c.artist.as_str(), c.duration_ms),
        ("003JdsAh", "A, B", 181_000)
    );

    let search = json!({"status":200,"candidates":[{"id":"821","accesskey":"AB12","song":"Song","singer":"Artist","duration":180097}]}).to_string();
    let c = &kugou::parse_search(search.as_bytes()).unwrap()[0];
    assert_eq!((c.key.as_str(), c.duration_ms), ("821:AB12", 180_097));
    let dl = json!({"status":200,"content": b64(&format!("\u{feff}{}", lrc(&LONG)))}).to_string();
    assert_eq!(
        kugou::parse_download(dl.as_bytes())
            .unwrap()
            .lines
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        kugou::parse_download(br#"{"status":500}"#).unwrap_err(),
        Fail::NotFound
    );

    let c =
        &netease::parse_search(ne_search(&[(7, "Song", "Artist", 180_045)]).as_bytes()).unwrap()[0];
    assert_eq!((c.key.as_str(), c.duration_ms), ("7", 180_045));
    assert!(
        netease::parse_search(b"{\"result\":{},\"code\":200}")
            .unwrap()
            .is_empty()
    );
    assert_eq!(netease::parse_search(b"nope").unwrap_err(), Fail::Failed);
    let instrumental = netease::parse_lyric(br#"{"nolyric":true}"#).unwrap();
    assert!(instrumental.instrumental);
}

#[test]
fn netease_prefers_word_level_yrc_and_removes_credit_lines() {
    let yrc: String = (0..5)
        .map(|i| {
            format!(
                "[{},900]({},400,0)Fixture ({},400,0)lyric line {i}\n",
                1000 * (i + 1),
                1000 * (i + 1),
                1000 * (i + 1) + 400
            )
        })
        .collect();
    let body = json!({"lrc":{"lyric": format!("[00:00.00]作词 : Someone\n[00:00.50]作曲：Else\n{}", lrc(&LONG))},"yrc":{"lyric": yrc}}).to_string();
    let found = netease::parse_lyric(body.as_bytes()).unwrap();
    let lines = found.lines.unwrap();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0].words.as_ref().unwrap().len(), 2);
    // Without yrc the LRC is used and the credits at its head are gone.
    let body = json!({"lrc":{"lyric": format!("[00:00.00]作词 : Someone\n[00:00.50]作曲：Else\n{}", lrc(&LONG))}}).to_string();
    let lines = netease::parse_lyric(body.as_bytes())
        .unwrap()
        .lines
        .unwrap();
    assert_eq!(lines.len(), 5);
    assert!(
        lines
            .iter()
            .all(|l| l.words.is_none() && !l.text.contains("作"))
    );
}

#[test]
fn entities_and_time_forms() {
    assert_eq!(
        unescape("a &amp; b &lt;c&gt; &quot;d&quot; &#39;e&#x27; &bogus; &"),
        "a & b <c> \"d\" 'e' &bogus; &"
    );
    assert_eq!(lyricsplus::clock_ms("3:21.570"), Some(201_570));
    assert_eq!(lyricsplus::clock_ms("1:00:00"), Some(3_600_000));
    assert_eq!(lyricsplus::clock_ms("x"), None);
    assert_eq!(lyricsplus::clock_ms("25:00:00"), None);
}

fn lines_at(n: usize, last_ms: u32) -> Vec<TimedLine> {
    (0..n)
        .map(|i| TimedLine {
            start_ms: if i + 1 == n { last_ms } else { i as u32 * 1000 },
            text: LONG[i % 5].into(),
            words: None,
        })
        .collect()
}

#[test]
fn finish_applies_the_length_rules() {
    let lyrics = |lines| SourceLyrics {
        text: None,
        lines: Some(lines),
        instrumental: false,
    };
    // Timed: allowed, within the track.
    let done = finish(lyrics(lines_at(5, 170_000)), true, 180_000).unwrap();
    assert!(done.lines.is_some() && !done.timing_mismatch);
    // Length differs by 3-12 s: plain text only, flagged.
    let done = finish(lyrics(lines_at(5, 170_000)), false, 180_000).unwrap();
    assert!(done.lines.is_none() && done.timing_mismatch && done.text.is_some());
    // A timestamp past the end of the track: timing is not trusted.
    let done = finish(lyrics(lines_at(5, 190_000)), true, 180_000).unwrap();
    assert!(done.lines.is_none() && done.timing_mismatch);
    // Fewer than four lines never count as timed lyrics; long enough text is still shown plain.
    let done = finish(lyrics(lines_at(2, 1000)), true, 180_000).unwrap();
    assert!(done.lines.is_none() && !done.timing_mismatch);
    // Too short to be a lyric at all.
    let tiny = vec![TimedLine {
        start_ms: 1,
        text: "hi".into(),
        words: None,
    }];
    assert!(finish(lyrics(tiny), true, 180_000).is_none());
    assert!(
        finish(
            SourceLyrics {
                instrumental: true,
                ..SourceLyrics::default()
            },
            true,
            1
        )
        .unwrap()
        .instrumental
    );
}

// ---------- flows against the mock ----------

#[tokio::test]
async fn netease_ranks_candidates_and_fetches_only_the_right_one() {
    let server = mock(0, |t| {
        if t.starts_with("/api/search/get") {
            (
                "200 OK",
                ne_search(&[
                    (1, "Song", "Someone Else", 180_000),
                    (2, "Song (Live)", "Artist", 180_000),
                    (3, "Song", "Artist", 180_800),
                ]),
            )
        } else {
            ("200 OK", ne_lyric())
        }
    })
    .await;
    let outcome = run_one(&server.ctx(), SourceId::Netease, &query(), None, false).await;
    let Outcome::Found(m) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(m.lines.is_some() && !m.words && !m.bound);
    let seen = server.requests();
    assert_eq!(seen.len(), 2, "one search, one lyric: {seen:?}");
    assert!(seen[1].contains("id=3"), "{seen:?}");
    // The query carries the cleaned title and artist only.
    assert!(
        seen[0].contains("s=Song+Artist") || seen[0].contains("s=Song%20Artist"),
        "{seen:?}"
    );
    server.stop();
}

#[tokio::test]
async fn candidate_length_decides_timing_and_far_candidates_are_rejected() {
    // 8 s longer: plain text only, with the mismatch flag.
    let server = mock(0, |t| {
        if t.starts_with("/api/search/get") {
            ("200 OK", ne_search(&[(3, "Song", "Artist", 188_000)]))
        } else {
            ("200 OK", ne_lyric())
        }
    })
    .await;
    let Outcome::Found(m) = run_one(&server.ctx(), SourceId::Netease, &query(), None, false).await
    else {
        panic!()
    };
    assert!(m.lines.is_none() && m.timing_mismatch);
    assert_eq!(
        entry(SourceId::Netease, &Outcome::Found(m)).status,
        "timingMismatch"
    );
    server.stop();
    // 20 s longer: another recording.
    let server = mock(0, |_| {
        ("200 OK", ne_search(&[(3, "Song", "Artist", 200_000)]))
    })
    .await;
    assert_eq!(
        run_one(&server.ctx(), SourceId::Netease, &query(), None, false).await,
        Outcome::NoMatch
    );
    assert_eq!(
        server.requests().len(),
        1,
        "no lyric request for a rejected candidate"
    );
    server.stop();
}

#[tokio::test]
async fn direct_sources_still_validate_title_and_length() {
    let good = mock(0, |_| ("200 OK", lp_body("Song", 5))).await;
    let Outcome::Found(m) = run_one(&good.ctx(), SourceId::LyricsPlus, &query(), None, false).await
    else {
        panic!()
    };
    assert!(m.words && m.lines.is_some());
    assert!(good.requests()[0].starts_with("/v2/lyrics/get?"));
    good.stop();
    let wrong = mock(0, |_| ("200 OK", lp_body("A Different Tune", 5))).await;
    assert_eq!(
        run_one(&wrong.ctx(), SourceId::LyricsPlus, &query(), None, false).await,
        Outcome::NoMatch
    );
    wrong.stop();
}

#[tokio::test]
async fn http_and_parse_failures_map_to_statuses() {
    for (status, want) in [
        ("404 Not Found", Outcome::NoMatch),
        ("500 Internal Server Error", Outcome::Unreachable),
        ("429 Too Many Requests", Outcome::Unreachable),
        ("403 Forbidden", Outcome::Failed),
    ] {
        let server = mock(0, move |_| (status, "{}".into())).await;
        assert_eq!(
            run_one(&server.ctx(), SourceId::LyricsPlus, &query(), None, false).await,
            want,
            "{status}"
        );
        server.stop();
    }
    let server = mock(0, |_| ("200 OK", "<html>not json".into())).await;
    assert_eq!(
        run_one(&server.ctx(), SourceId::Netease, &query(), None, false).await,
        Outcome::Failed
    );
    server.stop();
    // Boidu answers 401 for tracks it has not cached (it needs a key this app does not have).
    let server = mock(0, |_| {
        ("401 Unauthorized", r#"{"error":"API key required"}"#.into())
    })
    .await;
    assert_eq!(
        run_one(&server.ctx(), SourceId::Boidu, &query(), None, false).await,
        Outcome::NoMatch
    );
    server.stop();
    // Slow source: timeout is unreachable.
    let server = mock(400, |_| ("200 OK", lp_body("Song", 5))).await;
    let mut ctx = server.ctx();
    ctx.request_timeout = Some(Duration::from_millis(100));
    assert_eq!(
        run_one(&ctx, SourceId::LyricsPlus, &query(), None, false).await,
        Outcome::Unreachable
    );
    server.stop();
    // Nothing listening.
    let ctx = Ctx {
        base_override: Some("http://127.0.0.1:1".into()),
        ..Ctx::default()
    };
    assert_eq!(
        run_one(&ctx, SourceId::Kugou, &query(), None, false).await,
        Outcome::Unreachable
    );
    assert_eq!(
        entry(SourceId::Kugou, &Outcome::Unreachable).status,
        "unreachable"
    );
}

#[tokio::test]
async fn oversized_response_is_a_failure() {
    let server = mock(0, |_| {
        (
            "200 OK",
            format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_BODY + 10)),
        )
    })
    .await;
    assert_eq!(
        run_one(&server.ctx(), SourceId::LyricsPlus, &query(), None, false).await,
        Outcome::Failed
    );
    server.stop();
}

#[tokio::test]
async fn disabled_sources_make_no_request() {
    let server = mock(0, |_| ("200 OK", lp_body("Song", 5))).await;
    let mut ctx = server.ctx();
    ctx.settings.disabled = lyric_prefs::SOURCES
        .iter()
        .map(|(id, _)| (*id).to_owned())
        .collect();
    for kind in SourceId::ALL {
        assert_eq!(
            run_one(&ctx, kind, &query(), None, false).await,
            Outcome::Disabled
        );
        assert_eq!(entry(kind, &Outcome::Disabled).status, "disabled");
        assert!(sources_search_disabled(&ctx, kind).await);
    }
    assert!(server.requests().is_empty(), "{:?}", server.requests());
    server.stop();
}

async fn sources_search_disabled(ctx: &Ctx, kind: SourceId) -> bool {
    matches!(
        search_text(ctx, kind, &query(), "x").await,
        Err(Outcome::Disabled)
    )
}

#[tokio::test]
async fn unknown_length_or_artist_is_unavailable_without_a_request() {
    let server = mock(0, |_| ("200 OK", "{}".into())).await;
    let ctx = server.ctx();
    let mut q = query();
    q.duration_ms = 0;
    assert_eq!(
        run_one(&ctx, SourceId::Qq, &q, None, false).await,
        Outcome::Unavailable
    );
    let mut q = query();
    q.artists.clear();
    assert_eq!(
        run_one(&ctx, SourceId::Qq, &q, None, false).await,
        Outcome::Unavailable
    );
    let mut q = query();
    q.duration_ms = 7_200_000;
    assert_eq!(
        run_one(&ctx, SourceId::Qq, &q, None, false).await,
        Outcome::Unavailable
    );
    assert!(server.requests().is_empty());
    server.stop();
}

#[tokio::test]
async fn bound_candidate_replaces_the_automatic_match() {
    let server = mock(0, |t| {
        if t.starts_with("/api/song/lyric/v1") {
            ("200 OK", ne_lyric())
        } else {
            ("500 Internal Server Error", "{}".into())
        }
    })
    .await;
    let Outcome::Found(m) = run_one(
        &server.ctx(),
        SourceId::Netease,
        &query(),
        Some("777"),
        true,
    )
    .await
    else {
        panic!()
    };
    assert!(m.bound && m.lines.is_some());
    let seen = server.requests();
    assert_eq!(seen.len(), 1, "no search for a bound candidate: {seen:?}");
    assert!(seen[0].contains("id=777"));
    let json = serde_json::to_value(entry(SourceId::Netease, &Outcome::Found(m))).unwrap();
    assert_eq!(json["bound"], true);
    // A key that is not a Netease id never reaches the network.
    assert_eq!(
        run_one(
            &server.ctx(),
            SourceId::Netease,
            &query(),
            Some("7/../x"),
            true
        )
        .await,
        Outcome::Failed
    );
    assert_eq!(server.requests().len(), 1);
    server.stop();
}

#[tokio::test]
async fn bindings_in_the_context_are_used_by_run_all() {
    let _serial = SERIAL.lock().await;
    let server = mock(0, |t| {
        if t.starts_with("/api/song/lyric/v1") {
            ("200 OK", ne_lyric())
        } else {
            ("404 Not Found", "{}".into())
        }
    })
    .await;
    let mut ctx = server.ctx();
    ctx.bindings = vec![("netease".into(), "555".into())];
    let all = run_all(&ctx, "bound_track1", &query()).await;
    let netease = all.iter().find(|(k, _)| *k == SourceId::Netease).unwrap();
    assert!(matches!(&netease.1, Outcome::Found(m) if m.bound));
    assert_eq!(server.count("/api/search/get"), 0);
    assert!(server.requests().iter().any(|r| r.contains("id=555")));
    server.stop();
}

#[tokio::test]
async fn caches_are_per_source_and_never_hold_network_errors() {
    let root =
        std::env::temp_dir().join(format!("lunor-src-cache-{:032x}", rand::random::<u128>()));
    let server = mock(0, |t| {
        if t.starts_with("/api/search/get") {
            ("200 OK", ne_search(&[(3, "Song", "Artist", 180_000)]))
        } else {
            ("200 OK", ne_lyric())
        }
    })
    .await;
    let mut ctx = server.ctx();
    ctx.cache_dir = Some(root.clone());
    let first = run_one(&ctx, SourceId::Netease, &query(), None, false).await;
    assert!(matches!(first, Outcome::Found(_)));
    // Same source, no network: served from its own cache entry.
    let offline = Ctx {
        base_override: Some("http://127.0.0.1:1".into()),
        cache_dir: Some(root.clone()),
        ..Ctx::default()
    };
    assert_eq!(
        run_one(&offline, SourceId::Netease, &query(), None, false).await,
        first
    );
    // Another source has no entry under the same track key, so it goes to the (dead) network.
    assert_eq!(
        run_one(&offline, SourceId::Qq, &query(), None, false).await,
        Outcome::Unreachable
    );
    let files = || std::fs::read_dir(&root).unwrap().count();
    assert_eq!(files(), 1, "only the definite answer was stored");
    server.stop();

    // A definite miss is remembered (24 h); "Try again" ignores it.
    let miss = mock(0, |_| ("404 Not Found", "{}".into())).await;
    let mut ctx = miss.ctx();
    ctx.cache_dir = Some(root.clone());
    assert_eq!(
        run_one(&ctx, SourceId::LyricsPlus, &query(), None, false).await,
        Outcome::NoMatch
    );
    assert_eq!(
        run_one(&ctx, SourceId::LyricsPlus, &query(), None, false).await,
        Outcome::NoMatch
    );
    assert_eq!(
        miss.requests().len(),
        1,
        "second lookup came from the negative cache"
    );
    assert_eq!(
        run_one(&ctx, SourceId::LyricsPlus, &query(), None, true).await,
        Outcome::NoMatch
    );
    assert_eq!(miss.requests().len(), 2);
    miss.stop();
    // Unreachable is not cached: a later success is found.
    let down = mock(0, |_| ("500 Internal Server Error", "{}".into())).await;
    let mut ctx = down.ctx();
    ctx.cache_dir = Some(root.clone());
    assert_eq!(
        run_one(&ctx, SourceId::Boidu, &query(), None, false).await,
        Outcome::Unreachable
    );
    assert_eq!(
        run_one(&ctx, SourceId::Boidu, &query(), None, false).await,
        Outcome::Unreachable
    );
    assert_eq!(down.requests().len(), 2);
    down.stop();
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn requests_carry_no_credentials_and_only_public_metadata() {
    let server = mock(0, |t| {
        if t.starts_with("/soso") {
            ("200 OK", json!({"data":{"song":{"list":[{"songmid":"mid1","songname":"Song","singer":[{"name":"Artist"}],"interval":180}]}}}).to_string())
        } else {
            ("200 OK", json!({"retcode":0,"code":0,"lyric": b64(&lrc(&LONG))}).to_string())
        }
    })
    .await;
    let mut q = query();
    q.album = Some("Album".into());
    assert!(matches!(
        run_one(&server.ctx(), SourceId::Qq, &q, None, false).await,
        Outcome::Found(_)
    ));
    for head in server.heads.lock().unwrap().iter() {
        assert!(
            head.contains("\r\nreferer: https://y.qq.com/\r\n"),
            "{head}"
        );
        assert!(
            !head.contains("cookie")
                && !head.contains("authorization")
                && !head.contains("x-api-key")
        );
        assert!(head.contains("user-agent: lunor-x/"));
    }
    server.stop();
    let server = mock(0, |_| ("200 OK", lp_body("Song", 5))).await;
    run_one(&server.ctx(), SourceId::LyricsPlus, &q, None, false).await;
    let seen = server.requests();
    assert!(
        seen[0].contains("title=Song")
            && seen[0].contains("artist=Artist")
            && seen[0].contains("album=Album")
            && seen[0].contains("duration=180000")
    );
    assert!(!server.heads.lock().unwrap()[0].contains("referer"));
    server.stop();
}

#[test]
fn hosts_are_a_fixed_https_table() {
    let mut seen = std::collections::BTreeSet::new();
    for kind in SourceId::ALL {
        let url = reqwest::Url::parse(kind.base()).unwrap();
        assert_eq!(url.scheme(), "https");
        assert!(url.username().is_empty() && url.port().is_none() && url.path() == "/");
        assert!(
            seen.insert(url.host_str().unwrap().to_owned()),
            "hosts are distinct"
        );
        assert_eq!(SourceId::parse(kind.id()), Some(kind));
        assert!(!kind.label().is_empty());
    }
    assert_eq!(seen.len(), 5);
    assert!(SourceId::parse("simpmusic").is_none());
    assert_eq!(SourceId::Qq.referer(), Some("https://y.qq.com/"));
}

#[tokio::test]
async fn a_source_makes_at_most_three_requests() {
    let server = mock(0, |t| {
        if t.starts_with("/api/search/get") {
            (
                "200 OK",
                ne_search(&[
                    (1, "Song", "Artist", 180_000),
                    (2, "Song", "Artist", 180_100),
                    (3, "Song", "Artist", 180_200),
                ]),
            )
        } else {
            (
                "200 OK",
                json!({"lrc":{"lyric":"[00:01.00]too short"}}).to_string(),
            )
        }
    })
    .await;
    assert_eq!(
        run_one(&server.ctx(), SourceId::Netease, &query(), None, false).await,
        Outcome::NoMatch
    );
    assert_eq!(
        server.requests().len(),
        3,
        "1 search + the 2 best candidates"
    );
    server.stop();
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn at_most_four_sources_are_in_flight_and_every_source_answers() {
    let _serial = SERIAL.lock().await;
    let server = mock(150, |t| {
        if t.starts_with("/v2/lyrics/get") {
            ("200 OK", lp_body("Song", 5))
        } else if t.starts_with("/getLyrics") {
            ("401 Unauthorized", "{}".into())
        } else if t.starts_with("/api/search/get") {
            ("200 OK", ne_search(&[(3, "Song", "Artist", 180_000)]))
        } else if t.starts_with("/api/song/lyric/v1") {
            ("200 OK", ne_lyric())
        } else {
            ("404 Not Found", "{}".into())
        }
    })
    .await;
    let all = run_all(&server.ctx(), "conc_track_1", &query()).await;
    assert_eq!(all.len(), 5);
    assert_eq!(
        all.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        SourceId::ALL
    );
    assert!(matches!(all[0].1, Outcome::Found(_)));
    assert_eq!(all[1].1, Outcome::NoMatch);
    assert!(matches!(all[2].1, Outcome::Found(_)));
    assert_eq!(all[3].1, Outcome::NoMatch);
    assert_eq!(all[4].1, Outcome::NoMatch);
    let peak = server.peak.load(Ordering::SeqCst);
    assert!((1..=MAX_IN_FLIGHT).contains(&peak), "peak {peak}");
    server.stop();
}

#[tokio::test]
async fn a_new_track_cancels_the_previous_lookup() {
    let _serial = SERIAL.lock().await;
    let slow = mock(3000, |_| ("200 OK", lp_body("Song", 5))).await;
    let ctx = slow.ctx();
    let started = std::time::Instant::now();
    let q = query();
    let old = run_all(&ctx, "cancel_old_1", &q);
    let new = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let mut ctx = Ctx::default();
        ctx.settings.disabled = lyric_prefs::SOURCES
            .iter()
            .map(|(id, _)| (*id).to_owned())
            .collect();
        run_all(&ctx, "cancel_new_2", &query()).await
    };
    let (old, new) = tokio::join!(old, new);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "old lookup was not cancelled"
    );
    assert!(old.iter().all(|(_, o)| *o == Outcome::Failed));
    assert!(new.iter().all(|(_, o)| *o == Outcome::Disabled));
    slow.stop();
}

#[test]
fn entries_serialize_camel_case_with_explicit_nulls_and_the_words_flag() {
    let words = vec![lyrics::TimedWord {
        start_ms: 1000,
        end_ms: 1400,
        text: "Hi".into(),
    }];
    let found = Outcome::Found(Matched {
        text: "Hi".into(),
        lines: Some(vec![TimedLine {
            start_ms: 1000,
            text: "Hi".into(),
            words: Some(words),
        }]),
        words: true,
        timing_mismatch: false,
        bound: false,
    });
    let json = serde_json::to_value(entry(SourceId::LyricsPlus, &found)).unwrap();
    assert_eq!(json["id"], "lyricsplus");
    assert_eq!(json["label"], "LyricsPlus");
    assert_eq!(json["status"], "available");
    assert_eq!(json["words"], true);
    assert_eq!(json["synced"], true);
    assert_eq!(json["bound"], false);
    assert_eq!(json["lines"][0]["startMs"], 1000);
    assert_eq!(json["lines"][0]["words"][0]["endMs"], 1400);
    assert_eq!(json["attribution"], "Lyrics from LyricsPlus · word by word");
    // Plain lines serialize `words` as null, never omitted.
    let plain_line = TimedLine {
        start_ms: 1,
        text: "x".into(),
        words: None,
    };
    assert!(serde_json::to_value(plain_line).unwrap()["words"].is_null());
    for (outcome, status, available) in [
        (Outcome::NoMatch, "noMatch", false),
        (Outcome::Unreachable, "unreachable", false),
        (Outcome::Failed, "failed", false),
        (Outcome::Disabled, "disabled", false),
        (Outcome::Instrumental, "instrumental", false),
        (Outcome::Unavailable, "unavailable", false),
    ] {
        let json = serde_json::to_value(entry(SourceId::Kugou, &outcome)).unwrap();
        assert_eq!(
            (
                json["status"].as_str().unwrap(),
                json["available"].as_bool().unwrap()
            ),
            (status, available)
        );
        assert!(json["text"].is_null() && json["lines"].is_null() && json["attribution"].is_null());
        assert_eq!(json["words"], false);
    }
}
