//! The credential barrier for diagnostics text. Secret values are registered as fingerprints and never stored, and every
//! free-text value that may be logged or uploaded is masked here before it reaches disk. AGENTS.md rule 4: a password,
//! token, session cookie or provider credential must not appear in a log — this is what makes that true for free text.
use std::sync::{Mutex, OnceLock};

/// Shorter than this is ordinary prose: a fingerprint match on a short run would mask real words.
const MIN_VALUE: usize = 12;
/// Distinct fingerprints kept before the oldest is dropped; values rotate on sign-out and provider reconnect.
const MAX_TRACKED: usize = 96;
/// A run of token-shaped characters this long is not prose either, registered or not.
const LONG_RUN: usize = 40;
/// Markers whose following value is always withheld. Matched case-insensitively at a word boundary.
const MARKERS: [&[u8]; 20] = [
    b"bearer",
    b"authorization",
    b"proxy-authorization",
    b"x-goog-api-key",
    b"api_key",
    b"apikey",
    b"api-key",
    b"key",
    b"k",
    b"sig",
    b"signature",
    b"token",
    b"access_token",
    b"refresh_token",
    b"secret",
    b"password",
    b"passwd",
    b"cookie",
    b"sapisid",
    b"apisid",
];

/// The characters a credential can contain. A space, a Thai title or a quote ends a run, so ordinary text survives.
/// `@` belongs here so an address stays one run and can be recognised as one.
fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            b'+' | b'/' | b'=' | b'_' | b'-' | b'.' | b'%' | b':' | b'@'
        )
}

fn fingerprint(value: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in value {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[derive(Default)]
pub struct SecretRegistry {
    keys: Mutex<Vec<(u64, usize)>>,
}

static REGISTRY: OnceLock<SecretRegistry> = OnceLock::new();

/// Process-wide because secret material is: the keyring reader, the sync client, the share flow and the stream resolver
/// each hold values no single owner could hand down to the log layer.
pub fn registry() -> &'static SecretRegistry {
    REGISTRY.get_or_init(SecretRegistry::default)
}

impl SecretRegistry {
    /// Notes a live secret so its exact bytes can never appear in a log. Only fingerprints are kept.
    pub fn register(&self, value: &str) {
        let whole = value.trim();
        if whole.chars().count() < MIN_VALUE {
            return;
        }
        let mut candidates: Vec<&str> = vec![whole];
        // A cookie header is one value to us but a run of credentials inside it, and text is matched run by run.
        for part in whole.split([';', ',']) {
            let p = part.trim();
            if p.chars().count() >= MIN_VALUE {
                candidates.push(p);
            }
        }
        let mut keys = self.lock();
        for c in candidates {
            let key = (fingerprint(c.as_bytes()), c.len());
            if keys.contains(&key) {
                continue;
            }
            if keys.len() >= MAX_TRACKED {
                keys.remove(0);
            }
            keys.push(key);
        }
    }

    #[cfg(test)]
    fn tracked(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<(u64, usize)>> {
        // A poisoned lock must not stop masking; the data behind it is a fingerprint list.
        self.keys.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn mask_registered(&self, text: &str) -> (String, usize) {
        let keys = self.lock();
        if keys.is_empty() {
            return (text.to_string(), 0);
        }
        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut min = usize::MAX;
        let mut max = 0usize;
        for (_, len) in keys.iter() {
            min = min.min(*len);
            max = max.max(*len);
        }
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if !is_value_byte(bytes[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && is_value_byte(bytes[i]) {
                i += 1;
            }
            if i - start < min || i - start > max {
                continue;
            }
            if keys.contains(&(fingerprint(&bytes[start..i]), i - start)) {
                spans.push((start, i));
            }
        }
        splice(text, &spans)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scrubbed {
    pub text: String,
    pub masked: usize,
    pub truncated: bool,
}

/// Masks every credential-shaped value, then cuts the text to `max_chars`. Applied at record time, so what the user
/// reviews in Settings and what is uploaded are the same bytes.
pub fn scrub(text: &str, max_chars: usize) -> Scrubbed {
    let normalized = normalize(text);
    let (exact, mut masked) = registry().mask_registered(&normalized);
    let (shaped, n) = mask_shapes(&exact);
    masked += n;
    let (clamped, truncated) = clamp(&shaped, max_chars);
    Scrubbed {
        text: clamped,
        masked,
        truncated,
    }
}

fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c == '\r' {
            continue;
        }
        if c == '\n' || c == '\t' || !c.is_control() {
            out.push(c);
        }
    }
    out
}

fn clamp(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_string(), false);
    }
    let cut: String = text.chars().take(max_chars).collect();
    (cut, true)
}

fn mask_shapes(text: &str) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    while i < bytes.len() {
        if !is_value_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && is_value_byte(bytes[i]) {
            i += 1;
        }
        runs.push((start, i));
    }
    for (start, end) in &runs {
        let run = &bytes[*start..*end];
        if run.len() >= LONG_RUN {
            spans.push((*start, *end));
            continue;
        }
        if lower_contains(run, b"@") && run.contains(&b'.') {
            spans.push((*start, *end));
            continue;
        }
        if lower_contains(run, b"googlevideo.com") {
            spans.push((*start, *end));
            continue;
        }
        for m in MARKERS {
            let mut at = 0;
            while let Some(found) = find_ci(run, m, at) {
                let boundary = found == 0 || !run[found - 1].is_ascii_alphanumeric();
                let mut sep = found + m.len();
                // The value begins after `=`, `:` or a space, which is how these names reach a log line.
                while sep < run.len() && matches!(run[sep], b'=' | b':' | b' ') {
                    sep += 1;
                }
                let mut value = sep;
                while value < run.len() && is_value_byte(run[value]) {
                    value += 1;
                }
                if boundary && value - sep >= MIN_VALUE {
                    spans.push((start + sep, start + value));
                }
                at = found + m.len();
            }
        }
    }
    splice(text, &spans)
}

fn lower_contains(hay: &[u8], needle: &[u8]) -> bool {
    find_ci(hay, needle, 0).is_some()
}

fn find_ci(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < from + needle.len() {
        return None;
    }
    let mut i = from;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()]
            .iter()
            .zip(needle.iter())
            .all(|(h, n)| h.eq_ignore_ascii_case(n))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Replaces the given byte ranges with `<masked:NN>`. Ranges are ASCII-bounded, so a UTF-8 character is never split.
fn splice(text: &str, spans: &[(usize, usize)]) -> (String, usize) {
    if spans.is_empty() {
        return (text.to_string(), 0);
    }
    let mut merged: Vec<(usize, usize)> = spans.to_vec();
    merged.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut count = 0usize;
    for (start, end) in merged {
        if start < cursor {
            continue;
        }
        out.push_str(&text[cursor..start]);
        let hidden = text[start..end].chars().count();
        out.push_str(&format!("<masked:{hidden}>"));
        cursor = end;
        count += 1;
    }
    out.push_str(&text[cursor..]);
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> SecretRegistry {
        SecretRegistry::default()
    }

    #[test]
    fn a_registered_value_is_masked_wherever_it_appears() {
        let r = fresh();
        let token = "ya29.a0AfH6SMBx7KqLmZ0123456789abcdef";
        r.register(token);
        let (out, n) = r.mask_registered(&format!("request failed for header {token} at boot"));
        assert_eq!(n, 1);
        assert!(!out.contains(token));
        assert!(out.contains("<masked:37>"));
    }

    #[test]
    fn only_fingerprints_are_kept_and_a_short_value_is_refused() {
        let r = fresh();
        r.register("short");
        assert_eq!(r.tracked(), 0);
        r.register("0123456789abcdef");
        assert_eq!(r.tracked(), 1);
    }

    #[test]
    fn each_part_of_a_cookie_header_is_masked_on_its_own() {
        let r = fresh();
        r.register("__Secure-3PSID=abc123DEF456ghi789JKL012; SAPISID=z9X8w7V6u5T4s3R2q1P0");
        let (out, n) =
            r.mask_registered("cookie __Secure-3PSID=abc123DEF456ghi789JKL012 sent twice");
        assert!(n >= 1);
        assert!(!out.contains("abc123DEF456ghi789JKL012"));
    }

    #[test]
    fn shape_rules_mask_bearer_jwt_and_long_tokens_without_registration() {
        let scrubbed = scrub(
            "Authorization: Bearer eyJhbGciOiJSUzI1NiIs2V5IQSFlaW9yaXAua2VuMTIzNDU2 refused, SAPISID=z9X8w7V6u5T4s3R2q1P0 too",
            2000,
        );
        assert_eq!(scrubbed.masked, 2);
        assert!(
            !scrubbed
                .text
                .contains("eyJhbGciOiJSUzI1NiIs2V5IQSFlaW9yaXAua2VuMTIzNDU2")
        );
        assert!(!scrubbed.text.contains("z9X8w7V6u5T4s3R2q1P0"));
        let api = scrub(
            "https://music.youtube.com/watch?v=zz8aaaaaaaaaa&api_key=AIzaSyA1234567890abcdefghijklmnopqrstuvw",
            2000,
        );
        assert!(
            !api.text
                .contains("AIzaSyA1234567890abcdefghijklmnopqrstuvw")
        );
        assert!(api.text.contains("v=zz8aaaaaaaaaa"));
        let signed = scrub(
            "stream 403 from i1.googlevideo.com/videoplayback id=oABCDEFGH12345",
            2000,
        );
        assert!(!signed.text.contains("googlevideo.com"));
    }

    #[test]
    fn ordinary_text_titles_and_urls_survive_the_barrier() {
        let text = "Playlist Chill Tracks ลอย ล่อง opened https://www.youtube.com/watch?v=dQw4w9WgXcQ in 340 ms";
        let scrubbed = scrub(text, 2000);
        assert_eq!(scrubbed.masked, 0);
        assert_eq!(scrubbed.text, text);
    }

    #[test]
    fn an_email_and_a_key_query_are_withheld_and_the_text_is_clamped() {
        let scrubbed = scrub("notify admin@example.com about ?k=abcdefgh12345678", 30);
        assert!(!scrubbed.text.contains("admin@example.com"));
        assert!(!scrubbed.text.contains("abcdefgh12345678"));
        assert!(scrubbed.truncated);
        assert_eq!(scrubbed.text.chars().count(), 30);
    }

    #[test]
    fn control_characters_never_enter_a_stored_value() {
        let scrubbed = scrub("line one\nline two\0\u{1b}\tend", 200);
        assert!(!scrubbed.text.contains('\0'));
        assert!(!scrubbed.text.contains('\u{1b}'));
        assert!(scrubbed.text.contains('\n'));
        assert!(scrubbed.text.contains('\t'));
    }
}
