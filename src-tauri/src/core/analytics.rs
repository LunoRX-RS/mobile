//! Per-play listening log and the reports built from it. The log stays on this device; only `PlayCount` totals can leave it, and only when cloud sync is on.
use super::library::LibraryError;
use super::stats::PlayCount;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write,
    path::PathBuf,
};

const MAX_EVENTS: usize = 20_000;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// A play that ends before this much listening counts as a skip.
const SKIP_BELOW_MS: u64 = 30_000;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayEvent {
    pub provider: String,
    pub id: String,
    pub at_ms: u64,
    pub listened_ms: u64,
    pub duration_ms: u64,
}

pub struct PlayLog {
    path: PathBuf,
    events: Vec<PlayEvent>,
    dirty: bool,
}

impl PlayLog {
    pub fn open(path: PathBuf) -> Result<Self, LibraryError> {
        let events = if path.exists() {
            let meta = fs::symlink_metadata(&path).map_err(|_| LibraryError::Storage)?;
            if !meta.file_type().is_file() || meta.len() > MAX_BYTES {
                return Err(LibraryError::Storage);
            }
            serde_json::from_slice::<Vec<PlayEvent>>(
                &fs::read(&path).map_err(|_| LibraryError::Storage)?,
            )
            .map_err(|_| LibraryError::Storage)?
        } else {
            Vec::new()
        };
        if events.len() > MAX_EVENTS {
            return Err(LibraryError::Storage);
        }
        Ok(Self {
            path,
            events,
            dirty: false,
        })
    }

    /// Starts a new play. The oldest events fall off once the log is full.
    pub fn begin(&mut self, provider: String, id: String, at_ms: u64, duration_ms: u64) {
        if self.events.len() >= MAX_EVENTS {
            self.events.remove(0);
        }
        self.events.push(PlayEvent {
            provider,
            id,
            at_ms,
            listened_ms: 0,
            duration_ms,
        });
        self.dirty = true;
    }

    /// Adds listening time to the play in progress, if it is still `id`.
    pub fn add_listened(&mut self, id: &str, ms: u64) {
        if let Some(e) = self.events.last_mut().filter(|e| e.id == id) {
            e.listened_ms = e.listened_ms.saturating_add(ms);
            self.dirty = true;
        }
    }

    pub fn clear(&mut self) -> Result<(), LibraryError> {
        self.events.clear();
        self.dirty = true;
        self.flush()
    }

    pub fn events(&self) -> &[PlayEvent] {
        &self.events
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes the log if it changed. Atomic: a crash leaves the old file.
    pub fn flush(&mut self) -> Result<(), LibraryError> {
        if !self.dirty {
            return Ok(());
        }
        let bytes = serde_json::to_vec(&self.events).map_err(|_| LibraryError::Storage)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(LibraryError::Full);
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|_| LibraryError::Storage)?;
        }
        let part = self
            .path
            .with_extension(format!("{:032x}.part", rand::random::<u128>()));
        let result = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&part).map_err(|_| LibraryError::Storage)?;
            file.write_all(&bytes).map_err(|_| LibraryError::Storage)?;
            file.sync_all().map_err(|_| LibraryError::Storage)?;
            fs::rename(&part, &self.path).map_err(|_| LibraryError::Storage)
        })();
        let _ = fs::remove_file(part);
        result?;
        self.dirty = false;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayBucket {
    /// Days since 1970-01-01 in the caller's time zone.
    pub day: i64,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRank {
    pub name: String,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackRank {
    pub provider: String,
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub plays: u32,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub plays: u32,
    pub listened_ms: u64,
    pub unique_tracks: u32,
    pub unique_artists: u32,
    /// Tracks whose first play ever (in the whole log) falls inside the range.
    pub new_tracks: u32,
    pub active_days: u32,
    pub skips: u32,
    /// Average share of each song that was heard, 0 to 100, for plays whose length is known.
    pub avg_completion: u32,
    /// Consecutive days with a play ending today (or yesterday, if nothing yet today).
    pub current_streak: u32,
    pub longest_streak: u32,
    pub by_day: Vec<DayBucket>,
    pub by_hour: [u32; 24],
    /// Sunday first.
    pub by_weekday: [u32; 7],
    pub top_artists: Vec<ArtistRank>,
    pub top_tracks: Vec<TrackRank>,
}

fn local_ms(at_ms: u64, tz_offset_min: i32) -> i64 {
    at_ms as i64 - i64::from(tz_offset_min) * 60_000
}

/// `tz_offset_min` is JavaScript's `Date.getTimezoneOffset()`: minutes UTC is ahead of local time.
pub fn report(
    events: &[PlayEvent],
    meta: &[PlayCount],
    tz_offset_min: i32,
    from_ms: u64,
    to_ms: u64,
    now_ms: u64,
) -> Report {
    let info: HashMap<(&str, &str), &PlayCount> = meta
        .iter()
        .map(|t| ((t.provider.as_str(), t.provider_track_id.as_str()), t))
        .collect();
    let mut first_seen: HashMap<(&str, &str), u64> = HashMap::new();
    let mut all_days: HashSet<i64> = HashSet::new();
    for e in events {
        let key = (e.provider.as_str(), e.id.as_str());
        first_seen
            .entry(key)
            .and_modify(|t| *t = (*t).min(e.at_ms))
            .or_insert(e.at_ms);
        all_days.insert(local_ms(e.at_ms, tz_offset_min).div_euclid(DAY_MS));
    }

    let mut r = Report::default();
    let mut days: BTreeMap<i64, (u32, u64)> = BTreeMap::new();
    let mut artists: HashMap<&str, (u32, u64)> = HashMap::new();
    let mut tracks: HashMap<(&str, &str), (u32, u64)> = HashMap::new();
    let (mut pct_sum, mut pct_n) = (0u64, 0u64);
    for e in events
        .iter()
        .filter(|e| e.at_ms >= from_ms && e.at_ms < to_ms)
    {
        let local = local_ms(e.at_ms, tz_offset_min);
        let day = local.div_euclid(DAY_MS);
        r.plays += 1;
        r.listened_ms += e.listened_ms;
        if e.listened_ms < SKIP_BELOW_MS {
            r.skips += 1;
        }
        if let Some(pct) = (e.listened_ms * 100).checked_div(e.duration_ms) {
            pct_sum += pct.min(100);
            pct_n += 1;
        }
        let d = days.entry(day).or_default();
        d.0 += 1;
        d.1 += e.listened_ms;
        r.by_hour[(local.rem_euclid(DAY_MS) / 3_600_000) as usize] += 1;
        r.by_weekday[(day + 4).rem_euclid(7) as usize] += 1;
        let key = (e.provider.as_str(), e.id.as_str());
        let t = tracks.entry(key).or_default();
        t.0 += 1;
        t.1 += e.listened_ms;
        if let Some(meta) = info.get(&key) {
            for a in &meta.artists {
                let x = artists.entry(a.as_str()).or_default();
                x.0 += 1;
                x.1 += e.listened_ms;
            }
        }
    }
    r.unique_tracks = tracks.len() as u32;
    r.unique_artists = artists.len() as u32;
    r.new_tracks = tracks
        .keys()
        .filter(|k| {
            first_seen
                .get(k)
                .is_some_and(|&t| t >= from_ms && t < to_ms)
        })
        .count() as u32;
    r.active_days = days.len() as u32;
    r.avg_completion = pct_sum.checked_div(pct_n).unwrap_or(0) as u32;
    r.by_day = days
        .into_iter()
        .map(|(day, (plays, listened_ms))| DayBucket {
            day,
            plays,
            listened_ms,
        })
        .collect();

    let mut sorted: Vec<i64> = all_days.iter().copied().collect();
    sorted.sort_unstable();
    let (mut run, mut longest) = (0u32, 0u32);
    for (i, d) in sorted.iter().enumerate() {
        run = if i > 0 && sorted[i - 1] + 1 == *d {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
    }
    r.longest_streak = longest;
    let today = local_ms(now_ms, tz_offset_min).div_euclid(DAY_MS);
    let mut day = if all_days.contains(&today) {
        today
    } else {
        today - 1
    };
    while all_days.contains(&day) {
        r.current_streak += 1;
        day -= 1;
    }

    let mut a: Vec<_> = artists.into_iter().collect();
    a.sort_by(|x, y| y.1.0.cmp(&x.1.0).then(y.1.1.cmp(&x.1.1)).then(x.0.cmp(y.0)));
    r.top_artists = a
        .into_iter()
        .take(10)
        .map(|(name, (plays, listened_ms))| ArtistRank {
            name: name.to_string(),
            plays,
            listened_ms,
        })
        .collect();
    let mut t: Vec<_> = tracks.into_iter().collect();
    t.sort_by(|x, y| {
        y.1.0
            .cmp(&x.1.0)
            .then(y.1.1.cmp(&x.1.1))
            .then(x.0.cmp(&y.0))
    });
    r.top_tracks = t
        .into_iter()
        .take(10)
        .map(|((provider, id), (plays, listened_ms))| {
            let m = info.get(&(provider, id));
            TrackRank {
                provider: provider.to_string(),
                provider_track_id: id.to_string(),
                title: m.map_or_else(|| id.to_string(), |m| m.title.clone()),
                artists: m.map(|m| m.artists.clone()).unwrap_or_default(),
                artwork_url: m.and_then(|m| m.artwork_url.clone()),
                plays,
                listened_ms,
            }
        })
        .collect();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u64 = 3_600_000;
    const D: u64 = 86_400_000;

    fn ev(id: &str, at: u64, listened: u64, duration: u64) -> PlayEvent {
        PlayEvent {
            provider: "YouTube".into(),
            id: id.into(),
            at_ms: at,
            listened_ms: listened,
            duration_ms: duration,
        }
    }

    fn meta(id: &str, artist: &str) -> PlayCount {
        PlayCount {
            provider: "YouTube".into(),
            provider_track_id: id.into(),
            title: format!("title {id}"),
            artists: vec![artist.into()],
            artwork_url: None,
            plays: 1,
            last_played_ms: 0,
        }
    }

    #[test]
    fn report_totals_skips_hours_and_rankings() {
        // 1970-01-05 is a Monday; 20:00 UTC with a UTC+7 zone (offset -420) is 03:00 the next day.
        let day4 = 4 * D;
        let events = vec![
            ev("aaaaaaaaaaa", day4 + 20 * H, 200_000, 200_000),
            ev("aaaaaaaaaaa", day4 + 21 * H, 10_000, 200_000),
            ev("bbbbbbbbbbb", day4 + 22 * H, 100_000, 200_000),
        ];
        let m = vec![meta("aaaaaaaaaaa", "Alpha"), meta("bbbbbbbbbbb", "Beta")];
        let r = report(&events, &m, -420, 0, u64::MAX, day4 + 23 * H);
        assert_eq!(
            (r.plays, r.skips, r.unique_tracks, r.unique_artists),
            (3, 1, 2, 2)
        );
        assert_eq!(r.listened_ms, 310_000);
        assert_eq!(r.by_hour[3], 1, "20:00 UTC is 03:00 at UTC+7");
        assert_eq!(r.by_hour[5], 1);
        assert_eq!(r.top_artists[0].name, "Alpha");
        assert_eq!(r.top_tracks[0].provider_track_id, "aaaaaaaaaaa");
        assert_eq!(r.avg_completion, (100 + 5 + 50) / 3);
        assert_eq!(r.active_days, 1);
        assert_eq!(r.new_tracks, 2);
    }

    #[test]
    fn streaks_count_back_from_today_or_yesterday() {
        let events: Vec<_> = [1u64, 2, 3, 5, 6, 7, 8]
            .iter()
            .map(|d| ev("aaaaaaaaaaa", d * D + 12 * H, 60_000, 0))
            .collect();
        let today_has = report(&events, &[], 0, 0, u64::MAX, 8 * D + 13 * H);
        assert_eq!((today_has.current_streak, today_has.longest_streak), (4, 4));
        let yesterday_only = report(&events, &[], 0, 0, u64::MAX, 9 * D + H);
        assert_eq!(yesterday_only.current_streak, 4);
        let broken = report(&events, &[], 0, 0, u64::MAX, 10 * D + H);
        assert_eq!(broken.current_streak, 0);
    }

    #[test]
    fn range_limits_the_counts_but_new_tracks_use_the_whole_log() {
        let events = vec![
            ev("aaaaaaaaaaa", D, 60_000, 0),
            ev("aaaaaaaaaaa", 10 * D, 60_000, 0),
            ev("bbbbbbbbbbb", 10 * D + H, 60_000, 0),
        ];
        let r = report(&events, &[], 0, 5 * D, 20 * D, 11 * D);
        assert_eq!((r.plays, r.new_tracks), (2, 1));
    }

    #[test]
    fn the_log_persists_is_bounded_and_clears() {
        let p =
            std::env::temp_dir().join(format!("lunorx-log-{:032x}.json", rand::random::<u128>()));
        let mut log = PlayLog::open(p.clone()).unwrap();
        log.begin("YouTube".into(), "aaaaaaaaaaa".into(), 1, 100);
        log.add_listened("aaaaaaaaaaa", 40);
        log.add_listened("other", 99);
        log.flush().unwrap();
        let again = PlayLog::open(p.clone()).unwrap();
        assert_eq!(again.events()[0].listened_ms, 40);
        let mut again = again;
        again.clear().unwrap();
        assert!(PlayLog::open(p.clone()).unwrap().events().is_empty());
        let _ = fs::remove_file(p);
    }
}
