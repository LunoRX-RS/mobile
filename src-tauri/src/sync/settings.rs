//! Backend base URL setting for the desktop sync client.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    Empty,
    Scheme,
    Credentials,
    HostMissing,
    HostInvalid,
    InsecureRemote,
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Reason only: the input is never echoed, so a secret in the URL is never printed.
        let reason = match self {
            Self::Empty => "base URL is empty",
            Self::Scheme => "base URL scheme must be http or https",
            Self::Credentials => "base URL must not contain credentials",
            Self::HostMissing => "base URL has no host",
            Self::HostInvalid => "base URL host is invalid",
            Self::InsecureRemote => "base URL must use https unless the host is loopback",
        };
        f.write_str(reason)
    }
}

impl std::error::Error for SettingsError {}

/// The backend the app talks to: `LUNORX_BACKEND_URL` when set (checked by `parse_base_url` like any URL), else the local
/// development server.
pub fn default_base_url() -> String {
    std::env::var("LUNORX_BACKEND_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| env!("LUNORX_DEFAULT_BACKEND_URL").to_string())
}

/// Validates and normalizes a backend base URL. std-only: no url crate.
/// The website base that share links point at (`LUNORX_SHARE_BASE` at run time, else the build-time default).
pub fn share_base() -> String {
    std::env::var("LUNORX_SHARE_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| env!("LUNORX_DEFAULT_SHARE_BASE").to_string())
}

/// The link a recipient opens: `{base}/s/{token}`. The base is checked like the backend URL; `None` for a bad base or
/// token, never a link to somewhere else.
pub fn share_link(base: &str, token: &str) -> Option<String> {
    let base = parse_base_url(base).ok()?;
    let token_ok = token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    token_ok.then(|| format!("{}/s/{}", base.trim_end_matches('/'), token))
}

pub fn parse_base_url(input: &str) -> Result<String, SettingsError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(SettingsError::Empty);
    }
    let (scheme, rest) = trimmed.split_once("://").ok_or(SettingsError::Scheme)?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(SettingsError::Scheme);
    }
    // Any '@' is rejected, not only one in the authority: a secret must never reach the backend or a log.
    if trimmed.contains('@') {
        return Err(SettingsError::Credentials);
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        // IPv6 literal: "[addr]" or "[addr]:port". Anything else inside brackets is rejected.
        let (inner, after) = bracketed
            .split_once(']')
            .ok_or(SettingsError::HostInvalid)?;
        inner
            .parse::<Ipv6Addr>()
            .map_err(|_| SettingsError::HostInvalid)?;
        if !after.is_empty() && !after.starts_with(':') {
            return Err(SettingsError::HostInvalid);
        }
        inner
    } else {
        let host = authority.split(':').next().unwrap_or("");
        if host.is_empty() {
            return Err(SettingsError::HostMissing);
        }
        if host.contains(['[', ']']) {
            return Err(SettingsError::HostInvalid);
        }
        host
    };
    // Cleartext is allowed only to loopback, so credentials and tokens never cross a network in the clear.
    if scheme.eq_ignore_ascii_case("http") && !is_loopback(host) {
        return Err(SettingsError::InsecureRemote);
    }
    Ok(trimmed.strip_suffix('/').unwrap_or(trimmed).to_string())
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<Ipv4Addr>().is_ok_and(|a| a.is_loopback())
        || host.parse::<Ipv6Addr>().is_ok_and(|a| a.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_the_build_time_backend() {
        // The value depends on the build's .env (production by default, a local backend in development).
        assert!(parse_base_url(env!("LUNORX_DEFAULT_BACKEND_URL")).is_ok());
        assert_eq!(default_base_url(), env!("LUNORX_DEFAULT_BACKEND_URL"));
    }

    #[test]
    fn accepts_http_loopback() {
        assert_eq!(
            parse_base_url("http://127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080"
        );
    }

    #[test]
    fn rejects_cleartext_to_remote_host() {
        for input in [
            "http://example.test",
            "http://192.168.1.10:8080",
            "http://[2001:db8::1]",
        ] {
            assert_eq!(
                parse_base_url(input),
                Err(SettingsError::InsecureRemote),
                "{input}"
            );
        }
        assert!(parse_base_url("http://localhost:8080").is_ok());
        assert!(parse_base_url("http://[::1]:8080").is_ok());
    }

    #[test]
    fn accepts_https_host() {
        assert_eq!(
            parse_base_url("https://example.test").unwrap(),
            "https://example.test"
        );
    }

    #[test]
    fn trims_and_removes_trailing_slash() {
        assert_eq!(
            parse_base_url("  https://example.test/  ").unwrap(),
            "https://example.test"
        );
    }

    #[test]
    fn rejects_non_http_schemes() {
        for input in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/plain,x",
            "ftp://host",
        ] {
            assert_eq!(parse_base_url(input), Err(SettingsError::Scheme), "{input}");
        }
    }

    #[test]
    fn rejects_credentials_without_echoing_them() {
        let err = parse_base_url("https://user:pass@host").unwrap_err();
        assert_eq!(err, SettingsError::Credentials);
        let msg = err.to_string();
        assert!(!msg.contains("pass") && !msg.contains("user"), "{msg}");
    }

    #[test]
    fn rejects_empty_and_whitespace_only() {
        assert_eq!(parse_base_url(""), Err(SettingsError::Empty));
        assert_eq!(parse_base_url("   \t "), Err(SettingsError::Empty));
    }

    #[test]
    fn rejects_missing_host() {
        assert_eq!(parse_base_url("http://"), Err(SettingsError::HostMissing));
        assert_eq!(
            parse_base_url("http://:8080"),
            Err(SettingsError::HostMissing)
        );
    }

    #[test]
    fn accepts_bracketed_ipv6_literal() {
        assert_eq!(
            parse_base_url("http://[::1]:8080").unwrap(),
            "http://[::1]:8080"
        );
        assert_eq!(parse_base_url("https://[::1]").unwrap(), "https://[::1]");
    }

    #[test]
    fn rejects_malformed_bracket_hosts() {
        for input in [
            "http://[",
            "http://[::1",
            "http://]",
            "http://[]",
            "http://[not-an-ip]:8080",
            "http://[::1]x",
            "http://a]b",
        ] {
            assert_eq!(
                parse_base_url(input),
                Err(SettingsError::HostInvalid),
                "{input}"
            );
        }
    }

    #[test]
    fn error_display_never_echoes_input() {
        for input in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/plain,x",
            "ftp://host",
            "https://user:pass@host",
            "http://",
        ] {
            let msg = parse_base_url(input).unwrap_err().to_string();
            assert!(!msg.contains(input), "{msg}");
        }
    }

    #[test]
    fn the_build_time_default_is_a_valid_backend_url() {
        assert!(parse_base_url(env!("LUNORX_DEFAULT_BACKEND_URL")).is_ok());
    }

    #[test]
    fn share_links_point_at_the_website_page_for_the_token() {
        let token = "a".repeat(43);
        assert_eq!(
            share_link("https://play.lunorx.xyz", &token).as_deref(),
            Some(format!("https://play.lunorx.xyz/s/{token}").as_str())
        );
        // Trailing slash on the base does not double up.
        assert!(
            share_link("https://play.lunorx.xyz/", &token)
                .unwrap()
                .starts_with("https://play.lunorx.xyz/s/")
        );
        // A malformed token, or a base that is not an http(s) URL, gives no link at all.
        assert_eq!(share_link("https://lunorx.xyz", "short"), None);

        assert_eq!(share_link("ftp://lunorx.xyz", &token), None);
        assert_eq!(share_link("https://user@lunorx.xyz", &token), None);
    }
}
