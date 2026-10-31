//! One radio session. The host's confirmed EOF (or a failed track) is the only advance trigger. A session does not give up
//! on a failure that can pass: a network error, a refused stream, a track that cannot be loaded. It waits, tries the
//! next candidate and asks again, so music keeps playing for as long as the listener lets it.
use super::{
    audio::AudioControl,
    cache::{DiskCache, OfflineTrack},
    command::PlaybackCommand,
    engine::PlaybackEngine,
    extract::valid_id,
    state::PlaybackState,
};
use crate::core::{
    domain::{ProviderId, TrackReference},
    yt::Yt,
};
use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, watch};

/// How the end of the current track is handled. `repeat` is "off", "all" (play the list again once it is used up)
/// or "one" (play this track again). `radio` adds related tracks when the list runs out.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoplayStatus {
    pub state: &'static str,
    pub next_title: Option<String>,
    pub queue: Vec<OfflineTrack>,
    pub repeat: &'static str,
    pub radio: bool,
    #[serde(skip)]
    history: VecDeque<String>,
    /// Tracks already left, newest last; backs the Previous command.
    #[serde(skip)]
    back: Vec<TrackReference>,
    /// The list that was started (first track, then the tracks queued behind it); repeat-all plays it again.
    #[serde(skip)]
    context: Vec<OfflineTrack>,
    /// True when that list came from a playlist or album, so related tracks wait until it is used up.
    #[serde(skip)]
    list: bool,
}
impl Default for AutoplayStatus {
    fn default() -> Self {
        Self {
            state: "",
            next_title: None,
            queue: Vec::new(),
            repeat: "off",
            radio: true,
            history: VecDeque::new(),
            back: Vec::new(),
            context: Vec::new(),
            list: false,
        }
    }
}
impl AutoplayStatus {
    /// Like `Default`, but keeps the Previous stack and the user's repeat and autoplay choices across a manual load.
    pub fn reset(&mut self) {
        *self = Self {
            back: std::mem::take(&mut self.back),
            repeat: self.repeat,
            radio: self.radio,
            ..Default::default()
        };
    }
}
/// Sets repeat ("off" | "all" | "one") and autoplay. Anything else is refused.
pub fn set_mode(state: &Status, repeat: &str, radio: bool) -> bool {
    let repeat: &'static str = match repeat {
        "off" => "off",
        "all" => "all",
        "one" => "one",
        _ => return false,
    };
    if let Ok(mut s) = state.lock() {
        s.repeat = repeat;
        s.radio = radio;
        return true;
    }
    false
}
fn offline_of(t: &TrackReference) -> OfflineTrack {
    OfflineTrack {
        provider: "YouTube".into(),
        id: t.provider_track_id.clone(),
        title: t.title.clone(),
        artists: t.artists.clone(),
        artwork_url: t.artwork_url.clone(),
        duration_ms: t.duration_ms.and_then(|d| i64::try_from(d).ok()),
    }
}
/// Remembers the list a manual load started, for repeat-all.
pub fn set_context(state: &Status, first: &TrackReference, following: &[OfflineTrack]) {
    if let Ok(mut s) = state.lock() {
        let mut list = vec![offline_of(first)];
        list.extend(following.iter().cloned());
        s.context = list;
        s.list = !following.is_empty();
    }
}
/// Next with nothing queued and repeat-all on: starts the remembered list again.
pub fn restart_context(state: &Status) -> Option<TrackReference> {
    let mut s = state.lock().ok()?;
    if s.repeat != "all" || s.context.is_empty() {
        return None;
    }
    let first = reference(&s.context[0]).ok()?;
    s.queue = s.context.iter().skip(1).cloned().collect();
    s.next_title = None;
    s.state = "finding";
    Some(first)
}
pub fn push_back(state: &Status, track: TrackReference) {
    if let Ok(mut s) = state.lock() {
        if s.back
            .last()
            .is_some_and(|t| t.provider_track_id == track.provider_track_id)
        {
            return;
        }
        s.back.push(track);
        if s.back.len() > 50 {
            s.back.remove(0);
        }
    }
}
pub fn pop_back(state: &Status) -> Option<TrackReference> {
    state.lock().ok()?.back.pop()
}
/// How the user changes the up-next list by hand.
pub enum QueueEdit {
    /// Puts the track at the head, moving it up if it was already queued.
    Next(OfflineTrack),
    Remove(String),
    Move {
        from: usize,
        to: usize,
    },
    /// Puts the whole up-next list in a random order.
    Shuffle,
}
const QUEUE_MAX: usize = 500;
/// Returns false when the edit does not apply (unknown id, index out of range).
pub fn edit_queue(state: &Status, edit: QueueEdit) -> bool {
    let Ok(mut s) = state.lock() else {
        return false;
    };
    let head = s.queue.first().map(|t| t.id.clone());
    let ok = match edit {
        QueueEdit::Next(track) => {
            s.queue.retain(|t| t.id != track.id);
            s.queue.insert(0, track);
            s.queue.truncate(QUEUE_MAX);
            true
        }
        QueueEdit::Remove(id) => match s.queue.iter().position(|t| t.id == id) {
            Some(i) => {
                s.queue.remove(i);
                true
            }
            None => false,
        },
        QueueEdit::Shuffle => {
            for i in (1..s.queue.len()).rev() {
                let j = (rand::random::<u64>() % (i as u64 + 1)) as usize;
                s.queue.swap(i, j);
            }
            true
        }
        QueueEdit::Move { from, to } => {
            if from < s.queue.len() && to < s.queue.len() {
                let t = s.queue.remove(from);
                s.queue.insert(to, t);
                true
            } else {
                false
            }
        }
    };
    if ok && s.queue.first().map(|t| &t.id) != head.as_ref() {
        s.next_title = None;
    }
    ok
}
/// Head of the up-next list, consumed like a manual queue selection.
pub fn select_first(state: &Status) -> Option<TrackReference> {
    let id = state.lock().ok()?.queue.first()?.id.clone();
    select(state, &id)
}
pub type Status = Arc<std::sync::Mutex<AutoplayStatus>>;
fn status(target: &Status, state: &'static str, title: Option<String>) {
    if let Ok(mut value) = target.lock() {
        value.state = state;
        value.next_title = title;
    }
}

fn remember(history: &mut VecDeque<String>, id: String) {
    history.push_back(id);
    if history.len() > 100 {
        history.pop_front();
    }
}
fn eligible(id: &str, history: &VecDeque<String>) -> bool {
    valid_id(id) && !history.iter().any(|old| old == id)
}
/// Preparing the next song (downloading it ahead) is only a head start: the player can still load a song that was not
/// prepared, through the page player when the direct stream is refused, and a long episode needs no preparing at all.
/// So preparing never takes longer than this in all.
const PREPARE_BUDGET: Duration = Duration::from_secs(60);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(45);
/// The longest wait between two attempts after failures in a row, and the longest once the song is over.
const RETRY_CAP_SECS: u64 = 30;
#[cfg(not(test))]
const OVER_RETRY_CAP: Duration = Duration::from_secs(4);
#[cfg(test)]
const OVER_RETRY_CAP: Duration = Duration::from_millis(400);
/// A track that fails midway is reloaded from where it stopped by `commands::player` (up to twice). Only when it is
/// still in error after this long is it given up on and the next one played.
const ERROR_GRACE: Duration = Duration::from_secs(8);

/// 1 s after the first failure in a row, then 2, 4, 8, 16 and at most 30 s.
fn backoff(failures: u32) -> Duration {
    Duration::from_secs((1u64 << failures.saturating_sub(1).min(5)).min(RETRY_CAP_SECS))
}

/// Waits before the next attempt after `failures` in a row. While the song is still playing there is time to spare, so the
/// wait grows to the cap. If the song ends meanwhile the listener is waiting for sound, so what is left of the wait is
/// cut to [`OVER_RETRY_CAP`] from that moment. A change the caller must plan again for (`interrupted`) ends the wait
/// at once. The state is looked at every [`RETRY_POLL`], so progress reports cannot shorten the pacing.
async fn retry_pause(
    engine: &Mutex<PlaybackEngine>,
    failures: u32,
    interrupted: impl Fn() -> bool,
) {
    let mut until = Instant::now() + backoff(failures);
    let mut over_seen = false;
    loop {
        let now = Instant::now();
        if now >= until || interrupted() {
            return;
        }
        if !over_seen
            && matches!(
                engine.lock().await.state(),
                PlaybackState::Ended | PlaybackState::Error
            )
        {
            over_seen = true;
            until = until.min(now + OVER_RETRY_CAP);
        }
        tokio::time::sleep(RETRY_POLL.min(until.saturating_duration_since(now))).await;
    }
}
const RETRY_POLL: Duration = Duration::from_millis(250);

/// How a session ended. Neither is a failure of autoplay: the listener stopped, changed or ended the list.
#[derive(Debug, PartialEq, Eq)]
enum Ended {
    /// What was playing is gone (stopped, replaced, blocked) or the player went away.
    Gone,
    /// Autoplay is off and the list is used up: the normal end.
    Finished,
}

async fn session(
    engine: Arc<Mutex<PlaybackEngine>>,
    yt: Yt,
    cache: Arc<DiskCache>,
    audio: AudioControl,
    mut reports: watch::Receiver<u64>,
    state: Status,
) -> Ended {
    let mut history = state.lock().map(|s| s.history.clone()).unwrap_or_default();
    // Failures in a row (a candidate that would not load, nothing found to play); it paces the retries.
    let mut failures: u32 = 0;
    // When the current track was first seen in error, so the player's own reload gets its chance first.
    let mut errored_since: Option<Instant> = None;
    let mode = |state: &Status| {
        state
            .lock()
            .map(|s| (s.repeat, s.radio))
            .unwrap_or(("off", true))
    };
    'outer: loop {
        let current = engine.lock().await.active_track().cloned();
        let Some(current) = current else {
            return Ended::Gone;
        };
        let (repeat, radio_mode) = mode(&state);
        // Related music is YouTube's; after an episode the queue holds only what the listener chose.
        let radio = radio_mode && current.provider != ProviderId::Podcast;
        let seed = current.provider_track_id.clone();
        remember(&mut history, seed.clone());
        if let Ok(mut status) = state.lock() {
            status.history = history.clone();
        }
        status(&state, "finding", None);
        let mut queue = state.lock().map(|s| s.queue.clone()).unwrap_or_default();
        // What the copy above was made from: asking for related music takes seconds, and an edit made meanwhile must
        // not be overwritten when this pass publishes its list.
        let cloned: Vec<String> = queue.iter().map(|t| t.id.clone()).collect();
        let (list, context) = state
            .lock()
            .map(|s| (s.list, s.context.clone()))
            .unwrap_or_default();
        // A playlist or album plays through first; related tracks come in only once it is used up.
        if radio && repeat != "one" && (queue.is_empty() || (!list && queue.len() < 10)) {
            queue = related(&yt, &seed, &history, queue).await;
            if queue_edited(&state, &cloned) {
                continue 'outer;
            }
        }
        if queue.is_empty() && repeat == "all" && !context.is_empty() {
            queue = context.iter().skip(1).cloned().collect();
            queue.insert(0, context[0].clone());
        }
        let mut published: Vec<String> = Vec::new();
        publish(&state, &queue, &mut published);
        let repeat_one = repeat == "one";
        if repeat_one {
            // The same track again: it is already cached, so this only keeps the file alive until Load takes it.
            queue = vec![offline_of(&current)];
        }
        // Prefer a song that could be prepared among the first few; if none could, the first one is tried as it is.
        let deadline = Instant::now() + PREPARE_BUDGET;
        let mut prepared = None;
        for (index, track) in queue.iter().take(3).cloned().enumerate() {
            // An episode is read from its host as it plays: nothing to prepare ahead.
            if track.provider == "Podcast" {
                prepared = Some((index, track, None));
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            status(&state, "preparing", Some(track.title.clone()));
            // Keep the file alive until Load acquires it, including when cache quota is tight. A long episode is not
            // downloaded ahead: it is ready as soon as it resolves, and Load streams it.
            if let Ok(Ok(playable)) = tokio::time::timeout(
                left.min(PREPARE_TIMEOUT),
                cache.playable(&yt, track.clone(), audio.settings().quality),
            )
            .await
            {
                let file = match playable {
                    super::cache::Playable::File(file) => Some(file),
                    super::cache::Playable::Stream(_) => None,
                };
                prepared = Some((index, track, file));
                break;
            }
        }
        // The list may have been edited while songs were being prepared (an edit wakes the session): plan again from it.
        if queue_edited(&state, &published) {
            continue 'outer;
        }
        let (next, _file) = match prepared {
            Some((skipped, track, file)) => {
                // The ones passed over could not be prepared and are dropped from the list.
                queue.drain(..skipped);
                if skipped > 0 {
                    publish(&state, &queue, &mut published);
                }
                (track, file)
            }
            None => match queue.first().cloned() {
                Some(track) => (track, None),
                None if !radio => return Ended::Finished,
                None => {
                    // Related music is on but nothing came back (the service or the network failed): ask again.
                    failures = failures.saturating_add(1);
                    status(&state, "finding", None);
                    retry_pause(&engine, failures, || {
                        mode(&state) != (repeat, radio_mode) || queue_edited(&state, &published)
                    })
                    .await;
                    continue 'outer;
                }
            },
        };
        status(&state, "ready", Some(next.title.clone()));
        // Best effort, after readiness is published; never awaited. Lyrics are looked up by song, not by episode.
        if next.provider != "Podcast" {
            super::track_info::prefetch_lyrics(
                &next.id,
                &next.title,
                &next.artists,
                next.duration_ms,
            );
        }
        loop {
            reports.borrow_and_update();
            // The user changed repeat or autoplay while this track plays: plan the ending again.
            if mode(&state) != (repeat, radio_mode) {
                continue 'outer;
            }
            // The listener edited the up-next list: the song prepared for the end of this one may no longer be next.
            if queue_edited(&state, &published) {
                continue 'outer;
            }
            let mut player = engine.lock().await;
            let errored = player.state() == PlaybackState::Error;
            if errored {
                errored_since.get_or_insert_with(Instant::now);
            } else {
                errored_since = None;
            }
            let given_up = errored_since.is_some_and(|since| since.elapsed() >= ERROR_GRACE);
            match advance(&mut player, &seed, &next, given_up).await {
                Advance::Played => {
                    failures = 0;
                    errored_since = None;
                    push_back(&state, current.clone());
                    if !repeat_one {
                        queue.remove(0);
                    }
                    publish(&state, &queue, &mut published);
                    break;
                }
                Advance::Stalled => {
                    drop(player);
                    errored_since = None;
                    push_back(&state, current.clone());
                    if !repeat_one {
                        queue.remove(0);
                    }
                    publish(&state, &queue, &mut published);
                    failures = failures.saturating_add(1);
                    retry_pause(&engine, failures, || {
                        mode(&state) != (repeat, radio_mode) || queue_edited(&state, &published)
                    })
                    .await;
                    continue 'outer;
                }
                Advance::Waiting => {}
                Advance::Gone => return Ended::Gone,
                Advance::Failed => {
                    drop(player);
                    // This one cannot be loaded (even through the page player): it leaves the list and the next one
                    // gets a turn, after a pause that grows while failures keep coming.
                    failures = failures.saturating_add(1);
                    queue.retain(|t| t.id != next.id);
                    publish(&state, &queue, &mut published);
                    status(&state, "finding", None);
                    retry_pause(&engine, failures, || {
                        mode(&state) != (repeat, radio_mode) || queue_edited(&state, &published)
                    })
                    .await;
                    continue 'outer;
                }
            }
            drop(player);
            if errored {
                // No report may come while a track sits in error: look again when the grace is up.
                if let Ok(Err(_)) = tokio::time::timeout(ERROR_GRACE, reports.changed()).await {
                    return Ended::Gone;
                }
            } else if reports.changed().await.is_err() {
                return Ended::Gone;
            }
        }
    }
}
const QUEUE_SIZE: usize = 50;

fn offline_from_radio(t: innerlight::music::Track) -> OfflineTrack {
    OfflineTrack {
        provider: "YouTube".into(),
        id: t.id,
        title: t.title,
        artists: t.artists.into_iter().map(|a| a.name).collect(),
        artwork_url: t
            .thumbnails
            .into_iter()
            .max_by_key(|c| c.width)
            .and_then(|c| super::track_info::artwork_url(&c.url)),
        duration_ms: t.duration_ms.and_then(|d| i64::try_from(d).ok()),
    }
}

/// Appends the radio of a recently queued song to the up-next list, so the list keeps growing as the user scrolls.
/// The newest songs are tried first, then earlier ones, so one empty answer does not end the list. Returns how many
/// songs were added: 0 when the list is empty or full, or the service gave nothing new.
pub async fn load_more(state: &Status, yt: &Yt) -> usize {
    let (seeds, history, known) = {
        let Ok(s) = state.lock() else {
            return 0;
        };
        // More-like-this is a YouTube radio; a queue of episodes has no such thing.
        if s.queue.is_empty()
            || s.queue.len() >= QUEUE_MAX
            || s.queue.iter().any(|t| t.provider == "Podcast")
        {
            return 0;
        }
        (
            s.queue
                .iter()
                .rev()
                .take(5)
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
            s.history.clone(),
            s.queue.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        )
    };
    // The newest songs are asked together; the first answer with something new wins and the rest are dropped.
    let mut asking = tokio::task::JoinSet::new();
    for seed in seeds.into_iter().take(3) {
        let yt = yt.clone();
        asking.spawn(async move {
            let music = yt.music();
            tokio::time::timeout(Duration::from_secs(8), music.radio(&seed)).await
        });
    }
    let mut fresh = Vec::new();
    while let Some(done) = asking.join_next().await {
        let Ok(Ok(Ok(mut page))) = done else {
            continue;
        };
        append(
            &mut fresh,
            std::mem::take(&mut page.queue)
                .into_iter()
                .map(offline_from_radio)
                .filter(|t| !known.contains(&t.id)),
            &history,
        );
        if !fresh.is_empty() {
            break;
        }
    }
    let Ok(mut s) = state.lock() else {
        return 0;
    };
    let before = s.queue.len();
    for track in fresh {
        if s.queue.len() >= QUEUE_MAX {
            break;
        }
        if !s.queue.iter().any(|old| old.id == track.id) {
            s.queue.push(track);
        }
    }
    s.queue.len() - before
}
fn publish_queue(state: &Status, queue: &[OfflineTrack]) {
    if let Ok(mut status) = state.lock() {
        status.queue = queue.to_vec();
    }
}
/// Publishes `queue` and remembers what was published, so a later edit by the listener can be told apart.
fn publish(state: &Status, queue: &[OfflineTrack], published: &mut Vec<String>) {
    publish_queue(state, queue);
    *published = queue.iter().map(|t| t.id.clone()).collect();
}
/// True when the up-next list is no longer what the session last published: the listener edited it (or more songs
/// were added) meanwhile. The session then plans again from the list as it is, instead of overwriting it with its own
/// older copy.
fn queue_edited(state: &Status, published: &[String]) -> bool {
    state
        .lock()
        .map(|s| {
            s.queue.len() != published.len()
                || s.queue.iter().zip(published).any(|(t, id)| &t.id != id)
        })
        .unwrap_or(false)
}
fn append(
    queue: &mut Vec<OfflineTrack>,
    tracks: impl IntoIterator<Item = OfflineTrack>,
    history: &VecDeque<String>,
) {
    for track in tracks {
        if queue.len() >= QUEUE_SIZE {
            break;
        }
        if eligible(&track.id, history)
            && !track.title.trim().is_empty()
            && track.title.len() <= 512
            && track.artists.len() <= 30
            && track.artists.iter().all(|a| a.len() <= 512)
            && !queue.iter().any(|old| old.id == track.id)
        {
            queue.push(track);
        }
    }
}
/// Bounded radio pagination; returns partial results when the source is exhausted or unavailable.
/// The radio answer has no next-page handle, so each further page is the radio of the last queued track not used as a seed yet.
pub async fn related(
    yt: &Yt,
    seed: &str,
    history: &VecDeque<String>,
    mut queue: Vec<OfflineTrack>,
) -> Vec<OfflineTrack> {
    queue.truncate(QUEUE_SIZE);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let music = yt.music();
    let Ok(Ok(mut page)) = tokio::time::timeout(Duration::from_secs(14), music.radio(seed)).await
    else {
        return queue;
    };
    let mut seeds = std::collections::HashSet::from([seed.to_owned()]);
    for page_index in 0..5 {
        let items = std::mem::take(&mut page.queue)
            .into_iter()
            .map(offline_from_radio);
        append(&mut queue, items, history);
        if queue.len() >= QUEUE_SIZE || page_index == 4 {
            break;
        }
        let remaining = deadline
            .saturating_duration_since(tokio::time::Instant::now())
            .min(Duration::from_secs(10));
        if remaining.is_zero() {
            break;
        }
        let Some(seed) = queue
            .iter()
            .rev()
            .find(|t| !seeds.contains(&t.id))
            .map(|t| t.id.clone())
        else {
            break;
        };
        seeds.insert(seed.clone());
        match tokio::time::timeout(remaining, music.radio(&seed)).await {
            Ok(Ok(next)) => page = next,
            _ => break,
        }
    }
    queue
}
fn reference(next: &OfflineTrack) -> Result<TrackReference, ()> {
    let (provider, url) = if next.provider == "Podcast" {
        // An episode's id is the address of its audio.
        (ProviderId::Podcast, next.id.clone())
    } else {
        (
            ProviderId::YouTube,
            format!("https://www.youtube.com/watch?v={}", next.id),
        )
    };
    let mut track =
        TrackReference::new(provider, next.id.clone(), url, next.title.clone()).map_err(|_| ())?;
    track.artists = next.artists.clone();
    track.artwork_url = next.artwork_url.clone();
    Ok(track)
}
pub fn select(state: &Status, id: &str) -> Option<TrackReference> {
    let mut state = state.lock().ok()?;
    let index = state.queue.iter().position(|t| t.id == id)?;
    let track = reference(&state.queue[index]).ok()?;
    state.queue.drain(..=index);
    state.next_title = None;
    state.state = "finding";
    Some(track)
}
/// What [`advance`] did.
#[derive(Debug, PartialEq, Eq)]
enum Advance {
    /// The track is still playing (or paused, or being reloaded after an error): nothing to do yet.
    Waiting,
    /// The next track is loaded and playing.
    Played,
    /// The next track is loaded but would not start. It is the active one now; being errored it is over like any
    /// other track, so the session goes on from it, at the pace of a failure.
    Stalled,
    /// The listener stopped, replaced or was blocked on the track: this session has no business going on.
    Gone,
    /// The track ended (or failed for good) and the next one could not be loaded.
    Failed,
}

/// Moves to `next` once `seed` is over: it ended, or it failed and `given_up` says the player's own reload did not
/// bring it back. Anything else is left alone, and a listener's own stop or change ends the session.
async fn advance(
    player: &mut PlaybackEngine,
    seed: &str,
    next: &OfflineTrack,
    given_up: bool,
) -> Advance {
    let state = player.state();
    if player
        .active_track()
        .is_none_or(|t| t.provider_track_id != seed)
        || matches!(state, PlaybackState::Idle | PlaybackState::Blocked)
    {
        return Advance::Gone;
    }
    let over = state == PlaybackState::Ended || (state == PlaybackState::Error && given_up);
    if !over {
        return Advance::Waiting;
    }
    let Ok(track) = reference(next) else {
        return Advance::Failed;
    };
    if player.dispatch(PlaybackCommand::Load(track)).await.is_err() {
        return Advance::Failed;
    }
    if player.dispatch(PlaybackCommand::Play).await.is_err() {
        return Advance::Stalled;
    }
    Advance::Played
}
pub async fn run(
    engine: Arc<Mutex<PlaybackEngine>>,
    yt: Yt,
    cache: Arc<DiskCache>,
    audio: AudioControl,
    reports: watch::Receiver<u64>,
    state: Status,
) {
    // A session ends only when the listener's choice ends it (stop, a new track, autoplay off with the list used up).
    // Failures that can pass never end it, so there is no "unavailable" outcome any more.
    let _ended = session(engine, yt, cache, audio, reports, state.clone()).await;
    status(&state, "idle", None);
}
#[cfg(test)]
mod tests {
    use super::super::host::{HostFuture, HostOutcome, HostRequest, PlayerEvent, PlayerHost};
    use super::*;
    use crate::core::{
        capabilities::{CapabilityStatus, ProviderCapabilities},
        registry::ProviderRegistry,
    };
    struct Host;
    impl PlayerHost for Host {
        fn provider(&self) -> ProviderId {
            ProviderId::YouTube
        }
        fn prepare<'a>(&'a self, r: HostRequest) -> HostFuture<'a> {
            Box::pin(async move {
                HostOutcome::Confirmed {
                    request_id: r.request_id,
                    revision: r.revision,
                }
            })
        }
    }
    fn engine() -> PlaybackEngine {
        engine_with(Host)
    }
    fn engine_with(host: impl PlayerHost + 'static) -> PlaybackEngine {
        let mut registry = ProviderRegistry::new();
        registry
            .register(
                ProviderId::YouTube,
                ProviderCapabilities {
                    playback: CapabilityStatus::Supported,
                    ..ProviderCapabilities::unverified()
                },
            )
            .unwrap();
        PlaybackEngine::new(
            registry,
            std::collections::HashMap::from([(
                ProviderId::YouTube,
                Box::new(host) as Box<dyn PlayerHost>,
            )]),
            Duration::from_secs(1),
        )
    }
    fn track(id: &str) -> OfflineTrack {
        OfflineTrack {
            provider: "YouTube".into(),
            id: id.into(),
            title: id.into(),
            artists: vec![],
            artwork_url: None,
            duration_ms: None,
        }
    }
    #[test]
    fn mode_is_validated_and_survives_a_manual_load() {
        let state: Status = Default::default();
        assert!(!set_mode(&state, "forever", true));
        assert!(set_mode(&state, "all", false));
        state.lock().unwrap().reset();
        let s = state.lock().unwrap();
        assert_eq!((s.repeat, s.radio), ("all", false));
    }
    #[test]
    fn repeat_all_restarts_the_remembered_list_only_when_asked() {
        let state: Status = Default::default();
        let first = reference(&track("aqz-KE-bpKQ")).unwrap();
        set_context(&state, &first, &[track("r-G2pKr2_n8")]);
        assert!(restart_context(&state).is_none(), "repeat is off");
        assert!(set_mode(&state, "all", true));
        let again = restart_context(&state).unwrap();
        assert_eq!(again.provider_track_id, "aqz-KE-bpKQ");
        let queue = state.lock().unwrap().queue.clone();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].id, "r-G2pKr2_n8");
    }
    #[test]
    fn the_up_next_list_can_be_edited_by_hand() {
        let state: Status = Default::default();
        state.lock().unwrap().queue = vec![
            track("aaaaaaaaaaa"),
            track("bbbbbbbbbbb"),
            track("ccccccccccc"),
        ];
        let ids = |s: &Status| {
            s.lock()
                .unwrap()
                .queue
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>()
        };
        assert!(edit_queue(&state, QueueEdit::Next(track("ccccccccccc"))));
        assert_eq!(ids(&state), ["ccccccccccc", "aaaaaaaaaaa", "bbbbbbbbbbb"]);
        assert!(edit_queue(&state, QueueEdit::Move { from: 2, to: 0 }));
        assert_eq!(ids(&state), ["bbbbbbbbbbb", "ccccccccccc", "aaaaaaaaaaa"]);
        assert!(!edit_queue(&state, QueueEdit::Move { from: 0, to: 3 }));
        assert!(edit_queue(&state, QueueEdit::Remove("ccccccccccc".into())));
        assert!(!edit_queue(&state, QueueEdit::Remove("zzzzzzzzzzz".into())));
        assert_eq!(ids(&state), ["bbbbbbbbbbb", "aaaaaaaaaaa"]);
        // Shuffling keeps exactly the same songs, whatever the order.
        state.lock().unwrap().queue = (0..20).map(|i| track(&format!("{i:011}"))).collect();
        let mut before = ids(&state);
        assert!(edit_queue(&state, QueueEdit::Shuffle));
        let mut after = ids(&state);
        before.sort();
        after.sort();
        assert_eq!(before, after);
    }
    #[tokio::test]
    async fn loading_more_needs_a_queue_with_room_and_sends_nothing_otherwise() {
        use innerlight::{Client, FixtureTransport};
        let transport = Arc::new(FixtureTransport::new());
        let client = Client::builder()
            .transport(transport.clone())
            .build()
            .unwrap();
        let yt = Yt::from_client(client);
        let state: Status = Default::default();
        assert_eq!(load_more(&state, &yt).await, 0, "empty list has no seed");
        state.lock().unwrap().queue = (0..QUEUE_MAX).map(|i| track(&format!("{i:011}"))).collect();
        assert_eq!(load_more(&state, &yt).await, 0, "full list stays bounded");
        assert!(transport.requests().is_empty());
    }
    fn next() -> OfflineTrack {
        OfflineTrack {
            provider: "YouTube".into(),
            id: "aqz-KE-bpKQ".into(),
            title: "Next".into(),
            artists: vec![],
            artwork_url: None,
            duration_ms: None,
        }
    }
    async fn playing() -> PlaybackEngine {
        let mut player = engine();
        let track = TrackReference::new(
            ProviderId::YouTube,
            "r-G2pKr2_n8".into(),
            "https://www.youtube.com/watch?v=r-G2pKr2_n8".into(),
            "Seed".into(),
        )
        .unwrap();
        player.dispatch(PlaybackCommand::Load(track)).await.unwrap();
        player.dispatch(PlaybackCommand::Play).await.unwrap();
        player
    }
    const SEED: &str = "r-G2pKr2_n8";
    #[tokio::test]
    async fn advances_only_after_confirmed_eof_and_once() {
        let mut player = playing().await;
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Waiting
        );
        player.dispatch(PlaybackCommand::Pause).await.unwrap();
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Waiting
        );
        player.dispatch(PlaybackCommand::Resume).await.unwrap();
        player
            .observe(player.revision(), PlayerEvent::Ended)
            .unwrap();
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Played
        );
        assert_eq!(player.state(), PlaybackState::Playing);
        assert_eq!(
            player.active_track().unwrap().provider_track_id,
            "aqz-KE-bpKQ"
        );
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Gone
        );
    }
    #[tokio::test]
    async fn stopped_and_stale_sessions_cannot_restart_playback() {
        let mut player = playing().await;
        assert_eq!(
            advance(&mut player, "other-seed", &next(), false).await,
            Advance::Gone
        );
        player.dispatch(PlaybackCommand::Stop).await.unwrap();
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Gone
        );
    }
    #[tokio::test]
    async fn a_failed_track_waits_for_the_players_reload_then_the_music_goes_on() {
        let mut player = playing().await;
        player
            .observe(player.revision(), PlayerEvent::Failed)
            .unwrap();
        assert_eq!(player.state(), PlaybackState::Error);
        // The player reloads a failed track itself first; autoplay does not jump in over it.
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Waiting
        );
        // Once that has not brought it back, the next track plays.
        assert_eq!(
            advance(&mut player, SEED, &next(), true).await,
            Advance::Played
        );
        assert_eq!(player.state(), PlaybackState::Playing);
    }
    /// Refuses to load one track, and to start another, the way a host does when a stream is refused.
    struct Picky {
        unloadable: &'static str,
        unstartable: bool,
        loaded: std::sync::Mutex<String>,
    }
    impl PlayerHost for Picky {
        fn provider(&self) -> ProviderId {
            ProviderId::YouTube
        }
        fn prepare<'a>(&'a self, r: HostRequest) -> HostFuture<'a> {
            Box::pin(async move {
                let refuse = match &r.command {
                    PlaybackCommand::Load(t) => {
                        *self.loaded.lock().unwrap() = t.provider_track_id.clone();
                        t.provider_track_id == self.unloadable
                    }
                    PlaybackCommand::Play => {
                        self.unstartable && *self.loaded.lock().unwrap() == "aqz-KE-bpKQ"
                    }
                    _ => false,
                };
                if refuse {
                    HostOutcome::Rejected {
                        request_id: r.request_id,
                        revision: r.revision,
                        kind: crate::core::errors::PlaybackErrorKind::HostRejected(
                            crate::core::errors::HostRejectKind::Failed,
                        ),
                    }
                } else {
                    HostOutcome::Confirmed {
                        request_id: r.request_id,
                        revision: r.revision,
                    }
                }
            })
        }
    }
    fn picky(unloadable: &'static str, unstartable: bool) -> Picky {
        Picky {
            unloadable,
            unstartable,
            loaded: std::sync::Mutex::new(String::new()),
        }
    }
    async fn ended_with(host: Picky) -> PlaybackEngine {
        let mut player = engine_with(host);
        let seed = TrackReference::new(
            ProviderId::YouTube,
            SEED.into(),
            format!("https://www.youtube.com/watch?v={SEED}"),
            "Seed".into(),
        )
        .unwrap();
        player.dispatch(PlaybackCommand::Load(seed)).await.unwrap();
        player.dispatch(PlaybackCommand::Play).await.unwrap();
        player
            .observe(player.revision(), PlayerEvent::Ended)
            .unwrap();
        player
    }
    #[tokio::test]
    async fn a_next_track_that_cannot_load_fails_and_another_can_still_follow() {
        let mut player = ended_with(picky("aqz-KE-bpKQ", false)).await;
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Failed
        );
        // The failed load left the player in Error with the seed still active: the next candidate is tried, which
        // `given_up` allows because the player's own reload has nothing to reload here.
        assert_eq!(player.state(), PlaybackState::Error);
        let other = OfflineTrack {
            id: "abcdefghijk".into(),
            ..next()
        };
        assert_eq!(
            advance(&mut player, SEED, &other, true).await,
            Advance::Played
        );
        assert_eq!(player.state(), PlaybackState::Playing);
    }
    #[tokio::test]
    async fn a_track_that_loads_but_will_not_start_is_reported_as_stalled() {
        let mut player = ended_with(picky("none", true)).await;
        assert_eq!(
            advance(&mut player, SEED, &next(), false).await,
            Advance::Stalled
        );
        assert_eq!(player.state(), PlaybackState::Error);
        assert_eq!(
            player.active_track().unwrap().provider_track_id,
            "aqz-KE-bpKQ"
        );
        let other = OfflineTrack {
            id: "abcdefghijk".into(),
            ..next()
        };
        assert_eq!(
            advance(&mut player, "aqz-KE-bpKQ", &other, true).await,
            Advance::Played
        );
    }
    #[tokio::test]
    async fn a_retry_wait_ends_at_once_when_the_plan_changes() {
        let engine = Mutex::new(playing().await);
        let started = Instant::now();
        retry_pause(&engine, 6, || true).await;
        assert!(started.elapsed() < Duration::from_millis(100));
    }
    #[tokio::test]
    async fn a_retry_wait_is_cut_short_when_the_song_ends_during_it() {
        let engine = Mutex::new(playing().await);
        let ending = async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut player = engine.lock().await;
            let revision = player.revision();
            player.observe(revision, PlayerEvent::Ended).unwrap();
        };
        let started = Instant::now();
        // A 30 s wait that would otherwise leave silence after the song ends.
        tokio::join!(retry_pause(&engine, 6, || false), ending);
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(600), "{waited:?}");
        assert!(waited < Duration::from_secs(3), "{waited:?}");
    }
    #[tokio::test]
    async fn a_retry_wait_keeps_its_pacing_while_the_song_plays() {
        let engine = Mutex::new(playing().await);
        let started = Instant::now();
        retry_pause(&engine, 1, || false).await;
        assert!(started.elapsed() >= Duration::from_millis(950));
    }
    #[test]
    fn an_edit_of_the_up_next_list_is_told_from_what_the_session_published() {
        let state: Status = Default::default();
        let mut published = Vec::new();
        let queue = vec![track("aaaaaaaaaaa"), track("bbbbbbbbbbb")];
        publish(&state, &queue, &mut published);
        assert!(!queue_edited(&state, &published));
        assert!(edit_queue(&state, QueueEdit::Next(track("ccccccccccc"))));
        assert!(queue_edited(&state, &published));
        // Planning again publishes the list as it is, and that is the new baseline.
        let now = state.lock().unwrap().queue.clone();
        publish(&state, &now, &mut published);
        assert!(!queue_edited(&state, &published));
        assert!(edit_queue(&state, QueueEdit::Remove("ccccccccccc".into())));
        assert!(queue_edited(&state, &published));
    }
    #[test]
    fn retries_slow_down_and_stop_at_the_cap() {
        let waits: Vec<u64> = (1..=8).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(backoff(0).as_secs(), 1);
        assert_eq!(backoff(u32::MAX).as_secs(), 30);
    }
    #[test]
    fn queue_is_unique_bounded_and_excludes_history() {
        let history = VecDeque::from(["00000000000".into()]);
        let tracks = (0..100).flat_map(|i| {
            let mut t = next();
            t.id = format!("{i:011}");
            [t.clone(), t]
        });
        let mut queue = vec![];
        append(&mut queue, tracks, &history);
        assert_eq!(queue.len(), 50);
        assert_eq!(queue[0].id, "00000000001");
        assert_eq!(
            queue
                .iter()
                .map(|t| &t.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            50
        );
    }
    #[test]
    fn selecting_a_queue_song_keeps_the_following_order() {
        let state = Status::default();
        let mut queue = vec![];
        for i in 0..5 {
            let mut t = next();
            t.id = format!("{i:011}");
            queue.push(t);
        }
        publish_queue(&state, &queue);
        status(&state, "ready", Some("Next".into()));
        assert_eq!(state.lock().unwrap().queue.len(), 5);
        assert!(select(&state, "not-in-queue").is_none());
        assert_eq!(state.lock().unwrap().queue.len(), 5);
        let selected = select(&state, "00000000002").unwrap();
        assert_eq!(selected.provider_track_id, "00000000002");
        let queue = &state.lock().unwrap().queue;
        assert_eq!(
            queue.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["00000000003", "00000000004"]
        );
    }
    #[test]
    fn excludes_recent_tracks_and_bounds_history() {
        let mut history = VecDeque::new();
        remember(&mut history, "r-G2pKr2_n8".into());
        assert!(!eligible("r-G2pKr2_n8", &history));
        assert!(!eligible("invalid", &history));
        assert!(eligible("aqz-KE-bpKQ", &history));
        for i in 0..120 {
            remember(&mut history, format!("{i:011}"));
        }
        assert_eq!(history.len(), 100);
        assert!(eligible("r-G2pKr2_n8", &history));
    }
}
