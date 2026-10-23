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

pub fn default_base_url() -> String {
    "http://127.0.0.1:8080".to_string()
}

/// Validates and normalizes a backend base URL. std-only: no url crate.
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
    fn default_is_local_backend() {
        assert_eq!(default_base_url(), "http://127.0.0.1:8080");
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
}
