//! One bounded radio session. The host's confirmed EOF is the only advance trigger.
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
use std::{collections::VecDeque, sync::Arc, time::Duration};
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
async fn session(
    engine: Arc<Mutex<PlaybackEngine>>,
    yt: Yt,
    cache: Arc<DiskCache>,
    audio: AudioControl,
    mut reports: watch::Receiver<u64>,
    state: Status,
) {
    let mut history = state.lock().map(|s| s.history.clone()).unwrap_or_default();
    let mode = |state: &Status| {
        state
            .lock()
            .map(|s| (s.repeat, s.radio))
            .unwrap_or(("off", true))
    };
    'outer: loop {
        let current = engine.lock().await.active_track().cloned();
        let Some(current) = current else { return };
        let (repeat, radio) = mode(&state);
        let seed = current.provider_track_id.clone();
        remember(&mut history, seed.clone());
        if let Ok(mut status) = state.lock() {
            status.history = history.clone();
        }
        status(&state, "finding", None);
        let mut queue = state.lock().map(|s| s.queue.clone()).unwrap_or_default();
        let (list, context) = state
            .lock()
            .map(|s| (s.list, s.context.clone()))
            .unwrap_or_default();
        // A playlist or album plays through first; related tracks come in only once it is used up.
        if radio && repeat != "one" && (queue.is_empty() || (!list && queue.len() < 10)) {
            queue = related(&yt, &seed, &history, queue).await;
        }
        if queue.is_empty() && repeat == "all" && !context.is_empty() {
            queue = context.iter().skip(1).cloned().collect();
            queue.insert(0, context[0].clone());
        }
        publish_queue(&state, &queue);
        let repeat_one = repeat == "one";
        if repeat_one {
            // The same track again: it is already cached, so this only keeps the file alive until Load takes it.
            queue = vec![offline_of(&current)];
        }
        let mut next = None;
        for _ in 0..3 {
            let Some(track) = queue.first().cloned() else {
                break;
            };
            status(&state, "preparing", Some(track.title.clone()));
            // Keep the file alive until Load acquires it, including when cache quota is tight. A long episode is not
            // downloaded ahead: it is ready as soon as it resolves, and Load streams it.
            if let Ok(Ok(playable)) = tokio::time::timeout(
                Duration::from_secs(90),
                cache.playable(&yt, track.clone(), audio.settings().quality),
            )
            .await
            {
                let file = match playable {
                    super::cache::Playable::File(file) => Some(file),
                    super::cache::Playable::Stream(_) => None,
                };
                next = Some((track, file));
                break;
            }
            queue.remove(0);
            publish_queue(&state, &queue);
        }
        let Some((next, _file)) = next else {
            eprintln!("autoplay stopped: no related song could be prepared");
            return;
        };
        status(&state, "ready", Some(next.title.clone()));
        // Best effort, after readiness is published; never awaited.
        super::track_info::prefetch_lyrics(&next.id, &next.title, &next.artists, next.duration_ms);
        loop {
            reports.borrow_and_update();
            // The user changed repeat or autoplay while this track plays: plan the ending again.
            if mode(&state) != (repeat, radio) {
                continue 'outer;
            }
            let mut player = engine.lock().await;
            match advance(&mut player, &seed, &next).await {
                Ok(true) => {
                    push_back(&state, current.clone());
                    if !repeat_one {
                        queue.remove(0);
                    }
                    publish_queue(&state, &queue);
                    break;
                }
                Ok(false) => {}
                Err(()) => return,
            }
            drop(player);
            if reports.changed().await.is_err() {
                return;
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
        if s.queue.is_empty() || s.queue.len() >= QUEUE_MAX {
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
    let mut track = TrackReference::new(
        ProviderId::YouTube,
        next.id.clone(),
        format!("https://www.youtube.com/watch?v={}", next.id),
        next.title.clone(),
    )
    .map_err(|_| ())?;
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
async fn advance(player: &mut PlaybackEngine, seed: &str, next: &OfflineTrack) -> Result<bool, ()> {
    if player
        .active_track()
        .is_none_or(|t| t.provider_track_id != seed)
        || matches!(
            player.state(),
            PlaybackState::Idle | PlaybackState::Error | PlaybackState::Blocked
        )
    {
        return Err(());
    }
    if player.state() != PlaybackState::Ended {
        return Ok(false);
    }
    let track = reference(next)?;
    player
        .dispatch(PlaybackCommand::Load(track))
        .await
        .map_err(|_| ())?;
    player
        .dispatch(PlaybackCommand::Play)
        .await
        .map_err(|_| ())?;
    Ok(true)
}
pub async fn run(
    engine: Arc<Mutex<PlaybackEngine>>,
    yt: Yt,
    cache: Arc<DiskCache>,
    audio: AudioControl,
    reports: watch::Receiver<u64>,
    state: Status,
) {
    session(engine, yt, cache, audio, reports, state.clone()).await;
    let radio = state.lock().map(|s| s.radio).unwrap_or(true);
    // With autoplay off, running out of tracks is the normal end, not a failure.
    status(&state, if radio { "unavailable" } else { "idle" }, None);
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
                Box::new(Host) as Box<dyn PlayerHost>,
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
    #[tokio::test]
    async fn advances_only_after_confirmed_eof_and_once() {
        let mut player = playing().await;
        assert_eq!(
            advance(&mut player, "r-G2pKr2_n8", &next()).await,
            Ok(false)
        );
        player.dispatch(PlaybackCommand::Pause).await.unwrap();
        assert_eq!(
            advance(&mut player, "r-G2pKr2_n8", &next()).await,
            Ok(false)
        );
        player.dispatch(PlaybackCommand::Resume).await.unwrap();
        player
            .observe(player.revision(), PlayerEvent::Ended)
            .unwrap();
        assert_eq!(advance(&mut player, "r-G2pKr2_n8", &next()).await, Ok(true));
        assert_eq!(player.state(), PlaybackState::Playing);
        assert_eq!(
            player.active_track().unwrap().provider_track_id,
            "aqz-KE-bpKQ"
        );
        assert_eq!(advance(&mut player, "r-G2pKr2_n8", &next()).await, Err(()));
    }
    #[tokio::test]
    async fn stopped_and_stale_sessions_cannot_restart_playback() {
        let mut player = playing().await;
        assert_eq!(advance(&mut player, "other-seed", &next()).await, Err(()));
        player.dispatch(PlaybackCommand::Stop).await.unwrap();
        assert_eq!(advance(&mut player, "r-G2pKr2_n8", &next()).await, Err(()));
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
