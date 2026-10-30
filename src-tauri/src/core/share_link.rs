//! Reads share tokens from LunoRX links. Pure logic: no Tauri types, so it is tested without the app.
//!
//! Accepted forms:
//! - `lunorx://share/{token}` (deep link from the OS)
//! - `https://{host}/s/{token}` and `http://{host}/s/{token}` (web share page)
//! - `https://{host}/embed/{token}` and `https://{host}/og/{token}` (embed / card routes)
//! - a bare token of the right shape (so a user can paste just the token)

/// The token is 43 characters of URL-safe base64, the same shape the backend issues.
fn token_ok(token: &str) -> bool {
    token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The share token from a LunoRX link, or `None` for any other input.
pub fn token_from_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if token_ok(trimmed) {
        return Some(trimmed.to_string());
    }

    // Deep link: lunorx://share/{token}
    if let Ok(url) = url::Url::parse(trimmed) {
        if url.scheme() == "lunorx" && url.host_str() == Some("share") {
            let mut segments = url.path_segments()?;
            let token = segments.next()?;
            if segments.next().is_none() && token_ok(token) {
                return Some(token.to_string());
            }
            return None;
        }

        // Web share routes: /s/{token}, /embed/{token}, /og/{token}
        let segments: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
        if segments.len() >= 2 {
            let last = segments[segments.len() - 1];
            let prev = segments[segments.len() - 2];
            if matches!(prev, "s" | "embed" | "og") && token_ok(last) {
                return Some(last.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::token_from_url;

    const TOKEN: &str = "SIB1x3ILmH3J9aRSASqqls3ZR3BPfYhh2JC6OGu7UjU";

    #[test]
    fn reads_a_share_link() {
        assert_eq!(
            token_from_url(&format!("lunorx://share/{TOKEN}")).as_deref(),
            Some(TOKEN)
        );
    }

    #[test]
    fn reads_a_web_share_link() {
        assert_eq!(
            token_from_url(&format!("https://lunorx.xyz/s/{TOKEN}")).as_deref(),
            Some(TOKEN)
        );
        assert_eq!(
            token_from_url(&format!("https://example.com/embed/{TOKEN}")).as_deref(),
            Some(TOKEN)
        );
    }

    #[test]
    fn reads_a_bare_token() {
        assert_eq!(token_from_url(TOKEN).as_deref(), Some(TOKEN));
    }

    #[test]
    fn rejects_other_schemes_hosts_and_paths() {
        assert_eq!(token_from_url(&format!("https://share/{TOKEN}")), None);
        assert_eq!(token_from_url(&format!("lunorx://playlist/{TOKEN}")), None);
        assert_eq!(
            token_from_url(&format!("lunorx://share/{TOKEN}/extra")),
            None
        );
        assert_eq!(token_from_url("lunorx://share/"), None);
    }

    #[test]
    fn rejects_tokens_of_the_wrong_shape() {
        assert_eq!(token_from_url("short"), None);
        assert_eq!(
            token_from_url("https://x.test/s/SIB1x3ILmH3J9aRSASqqls3ZR3BPfYhh2JC6OGu7Uj%2F"),
            None
        );
        assert_eq!(token_from_url("not a url"), None);
    }
}
