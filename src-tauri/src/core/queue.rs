use crate::core::domain::PlaylistEntry;
use crate::core::errors::{CoreError, QueueErrorKind};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueEngine {
    entries: Vec<PlaylistEntry>,
    current: Option<usize>,
}

impl QueueEngine {
    pub fn new() -> Self {
        Self::default()
    }

    // Entry ids are unique per queue (blueprint section 10); same track under a new id is allowed.
    pub fn add(&mut self, entry: PlaylistEntry) -> Result<(), CoreError> {
        if entry.id.trim().is_empty() {
            return Err(CoreError::Validation("empty entry id".into()));
        }
        if self.entries.iter().any(|e| e.id == entry.id) {
            return Err(CoreError::Validation("duplicate entry id".into()));
        }
        self.entries.push(entry);
        Ok(())
    }

    // Removes only the first matching entry; current follows the entry it pointed at.
    pub fn remove(&mut self, entry_id: &str) -> Result<(), CoreError> {
        if self.entries.is_empty() {
            return Err(CoreError::Queue(QueueErrorKind::Empty));
        }
        let idx = self
            .entries
            .iter()
            .position(|e| e.id == entry_id)
            .ok_or_else(|| CoreError::Validation("entry not found".into()))?;
        self.entries.remove(idx);
        self.current = match self.current {
            Some(c) if c == idx => None,
            Some(c) if c > idx => Some(c - 1),
            other => other,
        };
        Ok(())
    }

    pub fn reorder(&mut self, from: usize, to: usize) -> Result<(), CoreError> {
        let len = self.entries.len();
        for index in [from, to] {
            if index >= len {
                return Err(CoreError::Queue(QueueErrorKind::IndexOutOfRange {
                    index,
                    len,
                }));
            }
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        // Map the old current index through remove(from) then insert(to).
        self.current = self.current.map(|c| {
            if c == from {
                to
            } else {
                let after_remove = if c > from { c - 1 } else { c };
                if after_remove >= to {
                    after_remove + 1
                } else {
                    after_remove
                }
            }
        });
        Ok(())
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = None;
    }

    pub fn current(&self) -> Option<&PlaylistEntry> {
        self.current.and_then(|i| self.entries.get(i))
    }

    // Playback engine rule: no wrap-around; advance from no current starts at index 0.
    pub fn advance(&mut self) -> Option<&PlaylistEntry> {
        let next = self.current.map_or(0, |i| i + 1);
        if next >= self.entries.len() {
            return None;
        }
        self.current = Some(next);
        self.entries.get(next)
    }

    // No wrap-around: at index 0 or with no current entry, current is left unchanged.
    pub fn previous(&mut self) -> Option<&PlaylistEntry> {
        match self.current {
            None | Some(0) => None,
            Some(i) => {
                self.current = Some(i - 1);
                self.entries.get(i - 1)
            }
        }
    }

    // Read-only twins of advance/previous: the entry they would move to, without moving.
    pub fn peek_next(&self) -> Option<&PlaylistEntry> {
        self.entries.get(self.current.map_or(0, |i| i + 1))
    }

    pub fn peek_previous(&self) -> Option<&PlaylistEntry> {
        self.current
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| self.entries.get(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::{ProviderId, TrackReference};

    fn entry(id: &str, video: &str) -> PlaylistEntry {
        PlaylistEntry {
            id: id.into(),
            track: TrackReference::new(
                ProviderId::YouTube,
                video.into(),
                format!("https://youtu.be/{video}"),
                "t".into(),
            )
            .unwrap(),
            position: 0,
        }
    }

    fn queue(ids: &[&str]) -> QueueEngine {
        let mut q = QueueEngine::new();
        for id in ids {
            q.add(entry(id, id)).unwrap();
        }
        q
    }

    fn ids(q: &QueueEngine) -> Vec<&str> {
        q.entries.iter().map(|e| e.id.as_str()).collect()
    }

    #[test]
    fn advance_on_empty_returns_none() {
        let mut q = QueueEngine::new();
        assert!(q.advance().is_none());
        assert_eq!(q.current(), None);
    }

    #[test]
    fn advance_from_none_on_fresh_queue_returns_first_entry() {
        let mut q = queue(&["a", "b"]);
        assert_eq!(q.current, None);
        assert_eq!(q.advance().map(|e| e.id.as_str()), Some("a"));
        assert_eq!(q.current, Some(0));
    }

    #[test]
    fn advance_past_last_returns_none_and_keeps_current_at_last() {
        let mut q = queue(&["a", "b"]);
        assert_eq!(q.advance().map(|e| e.id.as_str()), Some("a"));
        assert_eq!(q.advance().map(|e| e.id.as_str()), Some("b"));
        assert!(q.advance().is_none());
        assert_eq!(q.current, Some(1));
        assert_eq!(q.current().map(|e| e.id.as_str()), Some("b"));
    }

    #[test]
    fn removing_current_entry_sets_current_to_none() {
        let mut q = queue(&["a", "b", "c"]);
        q.advance();
        q.advance();
        q.remove("b").unwrap();
        assert_eq!(q.current, None);
        assert_eq!(q.current(), None);
    }

    #[test]
    fn removing_current_entry_leaves_remaining_entries_in_order() {
        let mut q = queue(&["a", "b", "c"]);
        q.advance();
        q.advance();
        q.remove("b").unwrap();
        assert_eq!(q.current, None);
        assert_eq!(ids(&q), vec!["a", "c"]);
    }

    #[test]
    fn remove_on_empty_queue_is_queue_empty() {
        let mut q = QueueEngine::new();
        assert_eq!(q.remove("x"), Err(CoreError::Queue(QueueErrorKind::Empty)));
    }

    #[test]
    fn remove_unknown_id_on_nonempty_queue_is_validation() {
        let mut q = queue(&["a"]);
        assert_eq!(
            q.remove("zz"),
            Err(CoreError::Validation("entry not found".into()))
        );
        assert_eq!(q.entries.len(), 1);
    }

    #[test]
    fn duplicates_survive_removal_of_one() {
        let mut q = QueueEngine::new();
        q.add(entry("e1", "same")).unwrap();
        q.add(entry("e2", "same")).unwrap();
        q.remove("e1").unwrap();
        assert_eq!(q.entries.len(), 1);
        assert_eq!(q.entries[0].id, "e2");
    }

    #[test]
    fn add_same_track_with_distinct_id_is_allowed() {
        let mut q = QueueEngine::new();
        q.add(entry("e1", "same")).unwrap();
        assert!(q.add(entry("e2", "same")).is_ok());
        assert_eq!(ids(&q), vec!["e1", "e2"]);
    }

    #[test]
    fn add_empty_id_is_validation_and_leaves_queue_unchanged() {
        let mut q = queue(&["a"]);
        let before = q.clone();
        assert_eq!(
            q.add(entry("  ", "x")),
            Err(CoreError::Validation("empty entry id".into()))
        );
        assert_eq!(q, before);
    }

    #[test]
    fn add_duplicate_id_is_validation_and_leaves_queue_unchanged() {
        let mut q = queue(&["a"]);
        let before = q.clone();
        assert_eq!(
            q.add(entry("a", "other")),
            Err(CoreError::Validation("duplicate entry id".into()))
        );
        assert_eq!(q, before);
    }

    #[test]
    fn removing_entry_before_current_keeps_current_on_same_entry() {
        let mut q = queue(&["a", "b", "c"]);
        q.advance();
        q.advance();
        q.remove("a").unwrap();
        assert_eq!(q.current().map(|e| e.id.as_str()), Some("b"));
    }

    #[test]
    fn reorder_moving_current_entry_follows_it() {
        let mut q = queue(&["a", "b", "c"]);
        q.advance();
        q.reorder(0, 2).unwrap();
        assert_eq!(ids(&q), vec!["b", "c", "a"]);
        assert_eq!(q.current().map(|e| e.id.as_str()), Some("a"));
        assert_eq!(q.current, Some(2));
    }

    #[test]
    fn reorder_other_entry_keeps_current_on_same_entry() {
        let mut q = queue(&["a", "b", "c"]);
        q.advance();
        q.advance();
        q.reorder(2, 0).unwrap();
        assert_eq!(ids(&q), vec!["c", "a", "b"]);
        assert_eq!(q.current().map(|e| e.id.as_str()), Some("b"));
        assert_eq!(q.current, Some(2));
    }

    #[test]
    fn reorder_out_of_range_is_error_and_does_not_mutate() {
        let mut q = queue(&["a", "b"]);
        q.advance();
        let before = q.clone();
        assert_eq!(
            q.reorder(0, 5),
            Err(CoreError::Queue(QueueErrorKind::IndexOutOfRange {
                index: 5,
                len: 2
            }))
        );
        assert_eq!(q, before);
    }

    #[test]
    fn previous_on_empty_queue_is_none() {
        let mut q = QueueEngine::new();
        assert!(q.previous().is_none());
        assert_eq!(q.current, None);
    }

    #[test]
    fn previous_with_no_current_is_none_and_current_stays_none() {
        let mut q = queue(&["a", "b"]);
        assert!(q.previous().is_none());
        assert_eq!(q.current, None);
    }

    #[test]
    fn previous_at_index_zero_is_none_and_current_stays_zero() {
        let mut q = queue(&["a", "b"]);
        q.advance();
        assert!(q.previous().is_none());
        assert_eq!(q.current, Some(0));
    }

    #[test]
    fn previous_from_index_two_returns_entry_at_one() {
        let mut q = queue(&["a", "b", "c"]);
        q.advance();
        q.advance();
        q.advance();
        assert_eq!(q.previous().map(|e| e.id.as_str()), Some("b"));
        assert_eq!(q.current, Some(1));
    }

    #[test]
    fn advance_then_previous_round_trips() {
        let mut q = queue(&["a", "b"]);
        q.advance();
        q.advance();
        assert_eq!(q.previous().map(|e| e.id.as_str()), Some("a"));
        assert_eq!(q.current, Some(0));
    }

    #[test]
    fn clear_empties_entries_and_resets_current() {
        let mut q = queue(&["a", "b"]);
        q.advance();
        q.clear();
        assert!(q.entries.is_empty());
        assert_eq!(q.current, None);
    }
}
