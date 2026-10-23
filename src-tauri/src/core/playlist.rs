use crate::core::domain::{PlaylistEntry, TrackReference, UnifiedPlaylist};
use crate::core::errors::{CoreError, PlaylistErrorKind};

// Every successful mutation bumps version by one and keeps positions equal to indexes.
// Mutators build the new state on a copy, so a failed bump leaves the caller's playlist untouched.
fn bump(p: &mut UnifiedPlaylist) -> Result<(), CoreError> {
    p.version = p
        .version
        .checked_add(1)
        .ok_or_else(|| CoreError::Validation("version overflow".into()))?;
    for (i, e) in p.entries.iter_mut().enumerate() {
        e.position = i as u32;
    }
    Ok(())
}

pub fn add_entry(
    p: &mut UnifiedPlaylist,
    entry_id: String,
    track: TrackReference,
) -> Result<(), CoreError> {
    if entry_id.trim().is_empty() {
        return Err(CoreError::Validation("empty entry id".into()));
    }
    if p.entries.iter().any(|e| e.id == entry_id) {
        return Err(CoreError::Validation("duplicate entry id".into()));
    }
    let mut next = p.clone();
    next.entries.push(PlaylistEntry {
        id: entry_id,
        track,
        position: 0,
    });
    bump(&mut next)?;
    *p = next;
    Ok(())
}

pub fn remove_entry(p: &mut UnifiedPlaylist, entry_id: &str) -> Result<(), CoreError> {
    let idx = p
        .entries
        .iter()
        .position(|e| e.id == entry_id)
        .ok_or(CoreError::Playlist(PlaylistErrorKind::NotFound))?;
    let mut next = p.clone();
    next.entries.remove(idx);
    bump(&mut next)?;
    *p = next;
    Ok(())
}

pub fn reorder(p: &mut UnifiedPlaylist, from: usize, to: usize) -> Result<(), CoreError> {
    let len = p.entries.len();
    for index in [from, to] {
        if index >= len {
            return Err(CoreError::Playlist(PlaylistErrorKind::IndexOutOfRange {
                index,
                len,
            }));
        }
    }
    let mut next = p.clone();
    let entry = next.entries.remove(from);
    next.entries.insert(to, entry);
    bump(&mut next)?;
    *p = next;
    Ok(())
}

pub fn rename(p: &mut UnifiedPlaylist, name: String) -> Result<(), CoreError> {
    if name.trim().is_empty() {
        return Err(CoreError::Validation("empty playlist name".into()));
    }
    let mut next = p.clone();
    next.name = name;
    bump(&mut next)?;
    *p = next;
    Ok(())
}

pub fn check_version(p: &UnifiedPlaylist, expected: u64) -> Result<(), CoreError> {
    if p.version != expected {
        return Err(CoreError::Playlist(PlaylistErrorKind::VersionConflict {
            expected,
            actual: p.version,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::ProviderId;

    fn track(id: &str) -> TrackReference {
        TrackReference::new(
            ProviderId::Spotify,
            id.into(),
            format!("https://open.spotify.com/track/{id}"),
            "t".into(),
        )
        .unwrap()
    }

    fn playlist() -> UnifiedPlaylist {
        UnifiedPlaylist::new("p1".into(), "Mix".into()).unwrap()
    }

    #[test]
    fn add_same_track_twice_yields_two_entries_with_distinct_ids() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        add_entry(&mut p, "e2".into(), track("a")).unwrap();
        assert_eq!(p.entries.len(), 2);
        assert_ne!(p.entries[0].id, p.entries[1].id);
        assert_eq!(p.entries[0].track, p.entries[1].track);
    }

    #[test]
    fn add_empty_entry_id_is_validation_error() {
        let mut p = playlist();
        let r = add_entry(&mut p, "  ".into(), track("a"));
        assert!(matches!(r, Err(CoreError::Validation(_))));
        assert!(p.entries.is_empty());
        assert_eq!(p.version, 0);
    }

    #[test]
    fn add_duplicate_entry_id_is_validation_error() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        let r = add_entry(&mut p, "e1".into(), track("b"));
        assert!(matches!(r, Err(CoreError::Validation(_))));
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.version, 1);
    }

    #[test]
    fn remove_one_of_duplicate_entries_keeps_the_other() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        add_entry(&mut p, "e2".into(), track("a")).unwrap();
        remove_entry(&mut p, "e1").unwrap();
        assert_eq!(p.entries.len(), 1);
        assert_eq!(p.entries[0].id, "e2");
        assert_eq!(p.entries[0].position, 0);
    }

    #[test]
    fn remove_unknown_entry_is_not_found() {
        let mut p = playlist();
        let r = remove_entry(&mut p, "nope");
        assert_eq!(r, Err(CoreError::Playlist(PlaylistErrorKind::NotFound)));
        assert_eq!(p.version, 0);
    }

    #[test]
    fn reorder_out_of_range_leaves_order_and_version_unchanged() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        add_entry(&mut p, "e2".into(), track("b")).unwrap();
        let before = p.clone();
        let r = reorder(&mut p, 0, 5);
        assert_eq!(
            r,
            Err(CoreError::Playlist(PlaylistErrorKind::IndexOutOfRange {
                index: 5,
                len: 2
            }))
        );
        assert_eq!(p, before);
    }

    #[test]
    fn reorder_from_out_of_range_leaves_order_and_version_unchanged() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        add_entry(&mut p, "e2".into(), track("b")).unwrap();
        let before = p.clone();
        let r = reorder(&mut p, 5, 0);
        assert_eq!(
            r,
            Err(CoreError::Playlist(PlaylistErrorKind::IndexOutOfRange {
                index: 5,
                len: 2
            }))
        );
        assert_eq!(p, before);
    }

    #[test]
    fn version_overflow_is_validation_and_leaves_playlist_unchanged() {
        let mut p = playlist();
        p.version = u64::MAX;
        let before = p.clone();
        let r = add_entry(&mut p, "e1".into(), track("a"));
        assert!(matches!(r, Err(CoreError::Validation(_))));
        assert_eq!(p.entries.len(), before.entries.len());
        assert_eq!(p.version, u64::MAX);
        assert_eq!(p, before);
    }

    #[test]
    fn reorder_swaps_and_reindexes_positions() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        add_entry(&mut p, "e2".into(), track("b")).unwrap();
        reorder(&mut p, 0, 1).unwrap();
        assert_eq!(p.entries[0].id, "e2");
        assert_eq!(p.entries[1].id, "e1");
        assert_eq!(p.entries[0].position, 0);
        assert_eq!(p.entries[1].position, 1);
    }

    #[test]
    fn every_successful_mutation_increments_version_by_exactly_one() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        assert_eq!(p.version, 1);
        add_entry(&mut p, "e2".into(), track("b")).unwrap();
        assert_eq!(p.version, 2);
        reorder(&mut p, 0, 1).unwrap();
        assert_eq!(p.version, 3);
        rename(&mut p, "Renamed".into()).unwrap();
        assert_eq!(p.version, 4);
        remove_entry(&mut p, "e1").unwrap();
        assert_eq!(p.version, 5);
    }

    #[test]
    fn check_version_with_stale_value_is_conflict() {
        let mut p = playlist();
        add_entry(&mut p, "e1".into(), track("a")).unwrap();
        assert_eq!(check_version(&p, 1), Ok(()));
        assert_eq!(
            check_version(&p, 0),
            Err(CoreError::Playlist(PlaylistErrorKind::VersionConflict {
                expected: 0,
                actual: 1
            }))
        );
    }

    #[test]
    fn rename_with_empty_name_is_validation_and_keeps_version() {
        let mut p = playlist();
        let r = rename(&mut p, "   ".into());
        assert!(matches!(r, Err(CoreError::Validation(_))));
        assert_eq!(p.version, 0);
        assert_eq!(p.name, "Mix");
    }
}
