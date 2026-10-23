use std::str::FromStr;

use crate::commands::dto::{IpcError, SearchQueryArgs};
use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;
use crate::core::search::SearchRequest;

// Byte length is safe: the ASCII check rejects any multi-byte character before the length matters.
pub fn validate_cancellation_id(id: &str) -> Result<(), IpcError> {
    let ok =
        (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(IpcError::Validation {
            message: "invalid cancellation id".into(),
        })
    }
}

// Unknown names fail the whole call; nothing is silently dropped.
pub fn parse_providers(names: &[String]) -> Result<Vec<ProviderId>, IpcError> {
    names
        .iter()
        .map(|n| {
            ProviderId::from_str(n).map_err(|_| IpcError::Validation {
                message: "unknown provider".into(),
            })
        })
        .collect()
}

pub fn build_request(args: &SearchQueryArgs) -> Result<SearchRequest, IpcError> {
    validate_cancellation_id(&args.cancellation_id)?;
    let providers = parse_providers(&args.providers)?;
    SearchRequest::new(&args.query, providers, args.limit).map_err(|e| match e {
        CoreError::Validation(message) => IpcError::Validation { message },
        _ => IpcError::Internal,
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::commands::dto::{IpcError, SearchQueryArgs};
    use crate::core::domain::ProviderId;
    use crate::core::search::SearchRequest;

    use super::*;

    fn args(id: &str, query: &str, providers: &[&str], limit: u8) -> SearchQueryArgs {
        SearchQueryArgs {
            query: query.into(),
            providers: providers.iter().map(|s| s.to_string()).collect(),
            limit,
            cancellation_id: id.into(),
        }
    }

    fn is_validation(r: &Result<(), IpcError>) -> bool {
        matches!(r, Err(IpcError::Validation { .. }))
    }

    #[test]
    fn empty_cancellation_id_is_rejected() {
        assert!(is_validation(&validate_cancellation_id("")));
    }

    #[test]
    fn cancellation_id_of_65_chars_is_rejected() {
        assert!(is_validation(&validate_cancellation_id(&"a".repeat(65))));
    }

    #[test]
    fn cancellation_id_of_64_chars_is_accepted() {
        assert!(validate_cancellation_id(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn cancellation_id_with_space_or_slash_is_rejected() {
        assert!(is_validation(&validate_cancellation_id("a b")));
        assert!(is_validation(&validate_cancellation_id("a/b")));
    }

    #[test]
    fn cancellation_id_allows_alphanumerics_and_hyphen() {
        assert!(validate_cancellation_id("Abc-123").is_ok());
    }

    #[test]
    fn lowercase_provider_is_unknown() {
        let r = parse_providers(&["spotify".into()]);
        assert!(
            matches!(r, Err(IpcError::Validation { ref message }) if message == "unknown provider")
        );
    }

    #[test]
    fn unknown_provider_is_not_dropped() {
        let r = parse_providers(&["Spotify".into(), "Deezer".into()]);
        assert!(
            matches!(r, Err(IpcError::Validation { ref message }) if message == "unknown provider")
        );
    }

    #[test]
    fn providers_parse_in_input_order() {
        let r = parse_providers(&["YouTube".into(), "Spotify".into()]).unwrap();
        assert_eq!(r, vec![ProviderId::YouTube, ProviderId::Spotify]);
    }

    #[test]
    fn empty_providers_give_empty_vec() {
        assert_eq!(parse_providers(&[]).unwrap(), Vec::<ProviderId>::new());
    }

    #[test]
    fn blank_query_is_rejected() {
        let r = build_request(&args("c1", " ", &[], 5));
        assert!(matches!(r, Err(IpcError::Validation { ref message }) if message == "empty query"));
    }

    #[test]
    fn query_of_201_chars_is_rejected() {
        let r = build_request(&args("c1", &"a".repeat(201), &[], 5));
        assert!(matches!(r, Err(IpcError::Validation { .. })));
    }

    #[test]
    fn limit_zero_and_eleven_are_rejected() {
        assert!(is_validation(
            &build_request(&args("c1", "rock", &[], 0)).map(|_| ())
        ));
        assert!(is_validation(
            &build_request(&args("c1", "rock", &[], 11)).map(|_| ())
        ));
    }

    #[test]
    fn valid_args_build_trimmed_request() {
        let r: SearchRequest = build_request(&args("c1", "  rock  ", &["YouTube"], 5)).unwrap();
        assert_eq!(r.query, "rock");
        assert_eq!(r.providers, vec![ProviderId::from_str("YouTube").unwrap()]);
        assert_eq!(r.limit, 5);
    }
}
