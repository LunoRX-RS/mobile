use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRequest {
    pub(crate) query: String,
    // Empty means all providers.
    pub(crate) providers: Vec<ProviderId>,
    pub(crate) limit: u8,
}

impl SearchRequest {
    pub fn new(query: &str, providers: Vec<ProviderId>, limit: u8) -> Result<Self, CoreError> {
        let query = query.trim();
        // chars(), not len(): the 200 limit is in characters.
        let len = query.chars().count();
        if len == 0 {
            return Err(CoreError::Validation("empty query".into()));
        }
        if len > 200 {
            return Err(CoreError::Validation("query longer than 200 chars".into()));
        }
        if !(1..=10).contains(&limit) {
            return Err(CoreError::Validation("limit must be 1 to 10".into()));
        }
        Ok(Self {
            query: query.to_owned(),
            providers,
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_validation(r: &Result<SearchRequest, CoreError>) -> bool {
        matches!(r, Err(CoreError::Validation(_)))
    }

    #[test]
    fn whitespace_only_query_is_rejected() {
        assert!(is_validation(&SearchRequest::new(" ", vec![], 5)));
    }

    #[test]
    fn query_is_trimmed() {
        let r = SearchRequest::new("  rock  ", vec![], 5).unwrap();
        assert_eq!(r.query, "rock");
    }

    #[test]
    fn query_of_200_chars_is_accepted() {
        let q = "a".repeat(200);
        assert!(SearchRequest::new(&q, vec![], 5).is_ok());
    }

    #[test]
    fn query_of_201_chars_is_rejected() {
        let q = "a".repeat(201);
        assert!(is_validation(&SearchRequest::new(&q, vec![], 5)));
    }

    #[test]
    fn query_length_counts_chars_not_bytes() {
        // 200 two-byte chars: 400 bytes but 200 chars, must be accepted.
        let q = "é".repeat(200);
        assert!(SearchRequest::new(&q, vec![], 5).is_ok());
    }

    #[test]
    fn limit_zero_and_eleven_are_rejected() {
        assert!(is_validation(&SearchRequest::new("rock", vec![], 0)));
        assert!(is_validation(&SearchRequest::new("rock", vec![], 11)));
    }

    #[test]
    fn limit_one_and_ten_are_accepted() {
        assert!(SearchRequest::new("rock", vec![], 1).is_ok());
        assert!(SearchRequest::new("rock", vec![], 10).is_ok());
    }

    #[test]
    fn empty_providers_means_all_and_is_accepted() {
        let r = SearchRequest::new("rock", vec![], 5).unwrap();
        assert!(r.providers.is_empty());
    }

    #[test]
    fn providers_are_kept() {
        let r = SearchRequest::new("rock", vec![ProviderId::YouTube], 5).unwrap();
        assert_eq!(r.providers, vec![ProviderId::YouTube]);
    }
}
