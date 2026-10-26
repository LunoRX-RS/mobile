//! Preload complete audio in RAM before exposing a private seekable URL to libmpv.
use super::extract::{ResolvedAudio, failed};
use crate::core::errors::{HostRejectKind, PlaybackErrorKind};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

const MAX_AUDIO_BYTES: u64 = 64 * 1024 * 1024;
type StreamError = Box<dyn std::error::Error + Send + Sync>;
pub struct AudioProxy {
    pub url: String,
    task: JoinHandle<()>,
}
impl Drop for AudioProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl AudioProxy {
    pub async fn start(audio: ResolvedAudio) -> Result<Self, PlaybackErrorKind> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| failed())?;
        let port = listener.local_addr().map_err(|_| failed())?.port();
        let path = format!("/{:032x}", rand::random::<u128>());
        let url = format!("http://127.0.0.1:{port}{path}");
        let signed = reqwest::Url::parse(&audio.url).map_err(|_| failed())?;
        let client = super::extract::bind_to_signed_family(reqwest::Client::builder(), &signed)
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() < 3
                    && attempt.url().scheme() == "https"
                    && attempt
                        .url()
                        .host_str()
                        .is_some_and(|h| h.ends_with(".googlevideo.com"))
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            }))
            .build()
            .map_err(|_| failed())?;
        if audio.size == 0 || audio.size > MAX_AUDIO_BYTES {
            return Err(failed());
        }
        // Never let the decoder start with a partial track that can fail mid-song.
        let audio = Arc::new(fetch_range(&client, &audio, 0, audio.size - 1).await?);
        eprintln!("audio preload complete: bytes={}", audio.len());
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((socket, _)) = accepted else { break; };
                        if connections.len() >= 4 { continue; }
                        let (audio, path) = (audio.clone(), path.clone());
                        connections.spawn(async move { let _ = serve(socket, &audio, &path).await; });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Ok(Self { url, task })
    }
}

fn byte_range(value: Option<&str>, size: u64) -> Option<(u64, u64)> {
    if size == 0 {
        return None;
    }
    let Some(value) = value else {
        return Some((0, size - 1));
    };
    let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().ok()?;
        return (suffix > 0).then(|| (size.saturating_sub(suffix), size - 1));
    }
    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<u64>().ok()?.min(size - 1)
    };
    (start <= end && start < size).then_some((start, end))
}

async fn write(socket: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> io::Result<()> {
    timeout(Duration::from_secs(15), socket.write_all(bytes)).await??;
    Ok(())
}

async fn serve(mut socket: TcpStream, audio: &[u8], path: &str) -> Result<(), StreamError> {
    let mut header = Vec::new();
    timeout(Duration::from_secs(5), async {
        while !header.ends_with(b"\r\n\r\n") {
            if header.len() >= 8192 {
                return Err(io::Error::other("header too large"));
            }
            header.push(socket.read_u8().await?);
        }
        Ok::<_, io::Error>(())
    })
    .await??;
    let text = std::str::from_utf8(&header)?;
    let mut lines = text.split("\r\n");
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let method = request.next().unwrap_or("");
    if !matches!(method, "GET" | "HEAD")
        || request.next() != Some(path)
        || request.next() != Some("HTTP/1.1")
        || request.next().is_some()
    {
        write(
            &mut socket,
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await?;
        return Ok(());
    }
    let mut range = None;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("invalid header")?;
        if name.eq_ignore_ascii_case("origin") {
            return Err("browser origin refused".into());
        }
        if name.eq_ignore_ascii_case("range") {
            if range.is_some() {
                return Err("duplicate range".into());
            }
            range = Some(value.trim());
        }
    }
    let Some((start, end)) = byte_range(range, audio.len() as u64) else {
        let header = format!(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            audio.len()
        );
        write(&mut socket, header.as_bytes()).await?;
        return Ok(());
    };
    let status = if range.is_some() {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let content_range = if range.is_some() {
        format!(
            "Content-Range: bytes {start}-{end}/{}\r\n",
            audio.len() as u64
        )
    } else {
        String::new()
    };
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n{content_range}Connection: close\r\n\r\n",
        end - start + 1
    );
    write(&mut socket, header.as_bytes()).await?;
    if method == "HEAD" {
        return Ok(());
    }
    write(&mut socket, &audio[start as usize..=end as usize]).await?;
    Ok(())
}

// Buffer the complete response; a rejected/truncated preload never reaches libmpv.
async fn fetch_range(
    client: &reqwest::Client,
    audio: &ResolvedAudio,
    start: u64,
    last: u64,
) -> Result<Vec<u8>, PlaybackErrorKind> {
    let url = reqwest::Url::parse(&audio.url).map_err(|_| failed())?;
    let expected = last - start + 1;
    for attempt in 0..3 {
        let mut upstream_status = 0;
        let result = async {
            // googlevideo 403s plain GETs; the byte span goes in the standard Range header.
            let mut response = client
                .get(url.clone())
                .header("range", format!("bytes={start}-{last}"))
                .send()
                .await
                .map_err(|_| failed())?;
            upstream_status = response.status().as_u16();
            if !response.status().is_success() {
                return Err(failed());
            }
            let mut bytes = Vec::with_capacity(expected as usize);
            while let Some(part) = response.chunk().await.map_err(|_| failed())? {
                if bytes.len() as u64 + part.len() as u64 > expected {
                    return Err(failed());
                }
                bytes.extend_from_slice(&part);
            }
            if bytes.len() as u64 != expected {
                return Err(failed());
            }
            Ok::<_, PlaybackErrorKind>(bytes)
        }
        .await;
        if let Ok(bytes) = result {
            return Ok(bytes);
        }
        eprintln!(
            "audio preload failed: bytes={start}-{last}, attempt={}, status={upstream_status}",
            attempt + 1
        );
        if matches!(upstream_status, 401 | 403 | 404 | 410) {
            return Err(PlaybackErrorKind::HostRejected(
                HostRejectKind::MediaRefused,
            ));
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(150 * (attempt + 1))).await;
        }
    }
    Err(failed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn complete_preload_survives_upstream_shutdown_and_serves_seeks() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let origin = format!("http://{}/audio", listener.local_addr().unwrap());
        let source: Vec<u8> = (0..2_000_000).map(|n| (n % 251) as u8).collect();
        let expected = source.clone();
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
            }
            let text = String::from_utf8(header).unwrap();
            // The byte span travels in the standard Range header; the URL itself is untouched.
            assert!(
                text.to_ascii_lowercase()
                    .contains("range: bytes=0-1999999\r\n")
            );
            let target = text.split_whitespace().nth(1).unwrap();
            let url = reqwest::Url::parse(&format!("http://localhost{target}")).unwrap();
            assert_eq!(url.query(), None);
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        source.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&source).await.unwrap();
        });
        let proxy = AudioProxy::start(ResolvedAudio {
            url: origin,
            size: expected.len() as u64,
        })
        .await
        .unwrap();
        upstream.await.unwrap();
        let client = reqwest::Client::new();
        let full = client
            .get(&proxy.url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(full.as_ref(), expected.as_slice());
        let seek = client
            .get(&proxy.url)
            .header("Range", "bytes=1048576-1049600")
            .send()
            .await
            .unwrap();
        assert_eq!(seek.status(), 206);
        assert_eq!(
            seek.bytes().await.unwrap().as_ref(),
            &expected[1048576..=1049600]
        );
        assert_eq!(
            client.head(&proxy.url).send().await.unwrap().headers()["content-length"],
            "2000000"
        );
        assert_eq!(
            client
                .get(format!("{}-wrong", proxy.url))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            client
                .get(&proxy.url)
                .header("Range", "bytes=2000000-")
                .send()
                .await
                .unwrap()
                .status(),
            416
        );
        assert!(
            client
                .get(&proxy.url)
                .header("Origin", "https://evil.test")
                .send()
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn rejected_and_incomplete_preloads_never_expose_a_player_url() {
        for (status, body, expected) in [
            (
                "403 Forbidden",
                Vec::new(),
                PlaybackErrorKind::HostRejected(HostRejectKind::MediaRefused),
            ),
            ("200 OK", vec![1, 2], failed()),
        ] {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let url = format!("http://{}/audio", listener.local_addr().unwrap());
            let upstream = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(socket.read_u8().await.unwrap());
                }
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                socket.write_all(&body).await.unwrap();
            });
            let error = AudioProxy::start(ResolvedAudio { url, size: 100 })
                .await
                .err()
                .expect("a rejected or truncated preload never yields a player URL");
            assert_eq!(error, expected, "status {status}");
            upstream.await.unwrap();
        }
    }

    #[tokio::test]
    async fn oversized_preload_is_rejected_without_network() {
        assert!(
            AudioProxy::start(ResolvedAudio {
                url: "https://rr1.googlevideo.com/unused".into(),
                size: MAX_AUDIO_BYTES + 1
            })
            .await
            .is_err()
        );
    }
}
