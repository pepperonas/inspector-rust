//! Spotify integration (macOS): a local play history of the Spotify desktop
//! app plus a few control commands. No Web API, no OAuth, nothing leaves the
//! machine.
//!
//! Layout:
//! * this file — the pure core: types, the play-history state machine
//!   (`Tracker`) and the listened/partial/skipped rules. Deterministic and
//!   fully unit-tested; it never talks to Spotify itself.
//! * `store` — the `spotify_plays` table in `history.db`.
//! * `export` — JSON / CSV / Markdown (pure).
//! * `macos` — the edge: Spotify's playback notification, AppleScript,
//!   the Automation-permission probe and the recorder thread.
//!
//! Reference: docs/spotify.md.

// The commands that use the core are macOS-only; on Linux/Windows only the
// table is created, so the rest would be dead code there.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

pub mod export;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod store;

use serde::{Deserialize, Serialize};

/// Bundle id of the Spotify desktop app.
pub const BUNDLE_ID: &str = "com.spotify.client";

// ── The rules ───────────────────────────────────────────────────────────────
//
// One place for every threshold, so "why is this a skip?" has one answer.

/// Played at least this share of the track → `listened`.
pub const LISTENED_FRACTION: f64 = 0.5;
/// …or at least this long, whatever the track length (Last.fm's rule:
/// half the track or four minutes, whichever comes first).
pub const LISTENED_MIN_MS: u64 = 240_000;
/// Played less than this (and not `listened`) → `skipped`.
pub const SKIPPED_MAX_MS: u64 = 30_000;
/// Plays shorter than this never become history — clicking through a
/// playlist is not listening.
pub const MIN_RECORD_MS: u64 = 5_000;
/// A track paused longer than this is closed; resuming it later starts a new
/// entry. Shorter pauses stay one entry.
pub const PAUSE_CLOSE_MS: i64 = 30 * 60_000;
/// One accounting step never adds more than this. Ticks run every 15 s, so a
/// larger gap means the Mac slept or the recorder stalled — that time was not
/// heard.
pub const MAX_STEP_MS: i64 = 60_000;
/// An entry left open by a previous run (Inspector quit or crashed) is
/// continued when the same track is still playing this soon afterwards.
pub const RESUME_MS: i64 = 3 * 60_000;
/// Repeat-one: the same track jumping back to its start counts as a new
/// play only when the previous position was this close to the end…
pub const REPLAY_END_SLACK_MS: u64 = 20_000;
/// …and the new position is near the start.
pub const REPLAY_START_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerState {
    Playing,
    Paused,
    Stopped,
    /// Spotify isn't running.
    NotRunning,
}

impl PlayerState {
    /// Parse Spotify's own spelling (AppleScript `playing`, notification
    /// `Playing`, …). Unknown text is `Stopped` — the safe reading.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "playing" => Self::Playing,
            "paused" => Self::Paused,
            _ => Self::Stopped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// `spotify:track:…`, `spotify:episode:…`, `spotify:local:…`. Empty when
    /// Spotify gave none — then `key()` falls back to the metadata.
    pub uri: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    /// Cover on Spotify's CDN (AppleScript only). Shown only if enabled.
    pub artwork_url: Option<String>,
}

impl Track {
    /// What makes two observations "the same track".
    pub fn key(&self) -> String {
        if self.uri.is_empty() {
            format!("{}\u{1f}{}\u{1f}{}", self.name, self.artist, self.album)
        } else {
            self.uri.clone()
        }
    }

    /// Ads are not music history.
    pub fn is_ad(&self) -> bool {
        self.uri.starts_with("spotify:ad:")
    }

    /// Nothing to identify it by — skip rather than store an empty row.
    pub fn is_blank(&self) -> bool {
        self.uri.is_empty() && self.name.trim().is_empty()
    }
}

/// One reading of the player, from a notification, an AppleScript query or
/// the recorder noticing that Spotify quit.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    pub at_ms: i64,
    pub state: PlayerState,
    pub track: Option<Track>,
    pub position_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayStatus {
    Listened,
    Partial,
    Skipped,
    /// Still open (now playing / paused).
    Playing,
}

impl PlayStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listened => "listened",
            Self::Partial => "partial",
            Self::Skipped => "skipped",
            Self::Playing => "playing",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "listened" => Self::Listened,
            "skipped" => Self::Skipped,
            "playing" => Self::Playing,
            _ => Self::Partial,
        }
    }
}

/// The listened/partial/skipped rule. Order matters: a long listen of a
/// short track is `listened` even if it was under 30 s of a 50-s jingle.
pub fn classify(played_ms: u64, duration_ms: Option<u64>) -> PlayStatus {
    let half = duration_ms
        .filter(|d| *d > 0)
        .map(|d| played_ms as f64 >= d as f64 * LISTENED_FRACTION)
        .unwrap_or(false);
    if half || played_ms >= LISTENED_MIN_MS {
        PlayStatus::Listened
    } else if played_ms < SKIPPED_MAX_MS {
        PlayStatus::Skipped
    } else {
        PlayStatus::Partial
    }
}

/// One play of one track — what becomes a history row.
#[derive(Debug, Clone, PartialEq)]
pub struct Play {
    /// Database row, once persisted.
    pub row_id: Option<i64>,
    pub track: Track,
    pub started_at: i64,
    /// When it stopped being heard (last playing moment); while open, the
    /// latest accounted moment.
    pub ended_at: i64,
    pub played_ms: u64,
    pub state: PlayerState,
    /// Last accounted moment (time is added from here on while playing).
    pub last_at: i64,
    pub paused_since: Option<i64>,
    pub position_ms: Option<u64>,
}

impl Play {
    fn new(track: Track, obs: &Observation) -> Self {
        Self {
            row_id: None,
            track,
            started_at: obs.at_ms,
            ended_at: obs.at_ms,
            played_ms: 0,
            state: obs.state,
            last_at: obs.at_ms,
            paused_since: (obs.state == PlayerState::Paused).then_some(obs.at_ms),
            position_ms: obs.position_ms,
        }
    }

    /// Count time up to `now` if it was playing. Steps beyond
    /// `MAX_STEP_MS` are sleep / stalls and are not counted past the cap.
    fn advance(&mut self, now: i64) {
        if now <= self.last_at {
            return;
        }
        if self.state == PlayerState::Playing {
            let step = (now - self.last_at).min(MAX_STEP_MS);
            self.played_ms += step as u64;
            self.ended_at = self.last_at + step;
        }
        self.last_at = now;
    }

    pub fn status(&self, open: bool) -> PlayStatus {
        if open {
            PlayStatus::Playing
        } else {
            classify(self.played_ms, self.track.duration_ms)
        }
    }
}

/// What the store has to do after an observation or tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// The open play moved on (persist if long enough).
    Update(Play),
    /// The play ended (finalise, or drop if it never reached
    /// `MIN_RECORD_MS`).
    Finish(Play),
}

/// The play-history state machine. Feed it observations and periodic ticks;
/// it decides when a play starts, continues and ends. Pause/resume of the
/// same track stays one play; a different track, Spotify stopping or
/// quitting, or a long pause ends it.
#[derive(Debug, Default)]
pub struct Tracker {
    current: Option<Play>,
    /// An entry a previous run left open, waiting to see whether the same
    /// track is still playing (`RESUME_MS`).
    pending_resume: Option<Play>,
}

impl Tracker {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start with the entry a previous run left open (if any).
    pub fn with_open(open: Option<Play>) -> Self {
        Self {
            current: None,
            pending_resume: open,
        }
    }

    pub fn current(&self) -> Option<&Play> {
        self.current.as_ref()
    }

    /// Tell the tracker which row the store gave the current play.
    pub fn set_row_id(&mut self, id: i64) {
        if let Some(p) = self.current.as_mut() {
            p.row_id = Some(id);
        }
    }

    fn finish_current(&mut self, out: &mut Vec<Change>) {
        if let Some(mut p) = self.current.take() {
            if let Some(since) = p.paused_since {
                p.ended_at = p.ended_at.min(since).max(p.started_at);
            }
            out.push(Change::Finish(p));
        }
    }

    fn settle_resume(&mut self, obs: Option<&Observation>, now: i64, out: &mut Vec<Change>) {
        let Some(open) = self.pending_resume.take() else {
            return;
        };
        let fresh = now - open.last_at <= RESUME_MS;
        let same = obs
            .and_then(|o| o.track.as_ref().map(|t| (o, t)))
            .filter(|(o, _)| matches!(o.state, PlayerState::Playing | PlayerState::Paused))
            .is_some_and(|(_, t)| t.key() == open.track.key());
        if fresh && same {
            // Continue it; the gap while Inspector was down is not counted.
            let mut p = open;
            p.last_at = now;
            self.current = Some(p);
        } else if fresh && obs.is_none() {
            // Nothing seen yet — keep waiting.
            self.pending_resume = Some(open);
        } else {
            let mut p = open;
            p.state = PlayerState::Stopped;
            out.push(Change::Finish(p));
        }
    }

    fn is_replay(play: &Play, position: Option<u64>) -> bool {
        let (Some(prev), Some(now), Some(dur)) = (play.position_ms, position, play.track.duration_ms)
        else {
            return false;
        };
        now <= REPLAY_START_MS && prev + REPLAY_END_SLACK_MS >= dur && prev > now
    }

    /// Feed one reading of the player.
    pub fn observe(&mut self, obs: &Observation) -> Vec<Change> {
        let mut out = Vec::new();
        self.settle_resume(Some(obs), obs.at_ms, &mut out);
        if let Some(p) = self.current.as_mut() {
            p.advance(obs.at_ms);
        }
        let track = obs
            .track
            .as_ref()
            .filter(|t| !t.is_ad() && !t.is_blank());
        match (obs.state, track) {
            (PlayerState::Playing | PlayerState::Paused, Some(track)) => {
                let same = self
                    .current
                    .as_ref()
                    .is_some_and(|p| p.track.key() == track.key());
                let replay = same
                    && self
                        .current
                        .as_ref()
                        .is_some_and(|p| Self::is_replay(p, obs.position_ms));
                if !same || replay {
                    self.finish_current(&mut out);
                    self.current = Some(Play::new(track.clone(), obs));
                } else if let Some(p) = self.current.as_mut() {
                    // Keep what we knew; take what's new (e.g. the cover only
                    // AppleScript provides).
                    if p.track.artwork_url.is_none() {
                        p.track.artwork_url = track.artwork_url.clone();
                    }
                    if p.track.duration_ms.is_none() {
                        p.track.duration_ms = track.duration_ms;
                    }
                    match (p.state, obs.state) {
                        (PlayerState::Playing, PlayerState::Paused) => p.paused_since = Some(obs.at_ms),
                        (_, PlayerState::Playing) => p.paused_since = None,
                        _ => {}
                    }
                    p.state = obs.state;
                    if obs.position_ms.is_some() {
                        p.position_ms = obs.position_ms;
                    }
                }
                if let Some(p) = self.current.as_ref() {
                    out.push(Change::Update(p.clone()));
                }
            }
            // Playing/paused without a usable track (an ad): end the music
            // play, but an ad is not a stop of listening — nothing new opens.
            (PlayerState::Playing | PlayerState::Paused, None) => self.finish_current(&mut out),
            (PlayerState::Stopped | PlayerState::NotRunning, _) => self.finish_current(&mut out),
        }
        out
    }

    /// Periodic tick (every 15 s): account time, close long pauses and
    /// resolve a stale entry from a previous run.
    pub fn tick(&mut self, now: i64) -> Vec<Change> {
        let mut out = Vec::new();
        self.settle_resume(None, now, &mut out);
        let long_pause = self
            .current
            .as_ref()
            .and_then(|p| p.paused_since)
            .is_some_and(|since| now - since >= PAUSE_CLOSE_MS);
        if long_pause {
            self.finish_current(&mut out);
        } else if let Some(p) = self.current.as_mut() {
            p.advance(now);
            out.push(Change::Update(p.clone()));
        }
        out
    }
}

// ── Parsing what Spotify says ───────────────────────────────────────────────

/// Field separator in the AppleScript answer (ASCII unit separator — it
/// cannot appear in a track name).
pub const SEP: char = '\u{1f}';

/// Parse the answer of `macos::QUERY_SCRIPT`:
/// `state SEP name SEP artist SEP album SEP uri SEP duration_ms SEP
/// position_ms SEP volume SEP artwork`. Numbers come pre-multiplied to
/// integers (AppleScript would format decimals with the locale's comma).
pub fn parse_query(answer: &str, at_ms: i64) -> Option<(Observation, Option<u8>)> {
    let answer = answer.trim_end_matches(['\n', '\r']);
    if answer.trim() == "not_running" {
        return Some((
            Observation {
                at_ms,
                state: PlayerState::NotRunning,
                track: None,
                position_ms: None,
            },
            None,
        ));
    }
    let f: Vec<&str> = answer.split(SEP).collect();
    if f.len() < 9 {
        return None;
    }
    let num = |s: &str| s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
    let state = PlayerState::parse(f[0]);
    let track = Track {
        uri: f[4].trim().to_string(),
        name: f[1].to_string(),
        artist: f[2].to_string(),
        album: f[3].to_string(),
        duration_ms: num(f[5]).map(|v| v as u64).filter(|v| *v > 0),
        artwork_url: Some(f[8].trim().to_string()).filter(|s| s.starts_with("https://")),
    };
    let track = (!track.is_blank()).then_some(track);
    Some((
        Observation {
            at_ms,
            state: if track.is_none() && state != PlayerState::Stopped {
                PlayerState::Stopped
            } else {
                state
            },
            track,
            position_ms: num(f[6]).map(|v| v as u64),
        },
        num(f[7]).map(|v| v.min(100.0) as u8),
    ))
}

/// A value from Spotify's notification `userInfo`.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Text(String),
    Number(f64),
}

/// Build an observation from the `com.spotify.client.PlaybackStateChanged`
/// notification. Keys as Spotify sends them: `Player State`, `Name`,
/// `Artist`, `Album`, `Track ID`, `Duration` (ms), `Playback Position` (s).
pub fn observation_from_notification(
    get: impl Fn(&str) -> Option<Field>,
    at_ms: i64,
) -> Observation {
    let text = |k: &str| match get(k) {
        Some(Field::Text(s)) => s,
        Some(Field::Number(n)) => n.to_string(),
        None => String::new(),
    };
    let number = |k: &str| match get(k) {
        Some(Field::Number(n)) if n.is_finite() && n >= 0.0 => Some(n),
        Some(Field::Text(s)) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0),
        _ => None,
    };
    let state = PlayerState::parse(&text("Player State"));
    let track = Track {
        uri: text("Track ID"),
        name: text("Name"),
        artist: text("Artist"),
        album: text("Album"),
        duration_ms: number("Duration").map(|d| d as u64).filter(|d| *d > 0),
        artwork_url: None,
    };
    let track = (!track.is_blank()).then_some(track);
    Observation {
        at_ms,
        state,
        track,
        position_ms: number("Playback Position").map(|s| (s * 1000.0) as u64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(uri: &str, dur: Option<u64>) -> Track {
        Track {
            uri: uri.into(),
            name: format!("Name {uri}"),
            artist: "Artist".into(),
            album: "Album".into(),
            duration_ms: dur,
            artwork_url: None,
        }
    }

    fn obs(at: i64, state: PlayerState, t: Option<Track>, pos: Option<u64>) -> Observation {
        Observation {
            at_ms: at,
            state,
            track: t,
            position_ms: pos,
        }
    }

    fn finished(changes: &[Change]) -> Vec<Play> {
        changes
            .iter()
            .filter_map(|c| match c {
                Change::Finish(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    /// Feed observations at their time and ticks every 15 s in between, like
    /// the recorder, ending with a tick at `until`.
    fn run(tracker: &mut Tracker, events: &[Observation], until: i64) -> Vec<Play> {
        let mut done = Vec::new();
        let mut next_tick = events.first().map(|e| e.at_ms).unwrap_or(0);
        let mut i = 0;
        loop {
            let event_at = events.get(i).map(|e| e.at_ms).unwrap_or(i64::MAX);
            if event_at <= next_tick && event_at <= until {
                done.extend(finished(&tracker.observe(&events[i])));
                i += 1;
            } else if next_tick < until {
                done.extend(finished(&tracker.tick(next_tick)));
                next_tick += 15_000;
            } else {
                done.extend(finished(&tracker.tick(until)));
                return done;
            }
        }
    }

    const S: i64 = 1_000;
    const A: &str = "spotify:track:a";
    const B: &str = "spotify:track:b";

    #[test]
    fn classification_rules() {
        // Half the track → listened, even if short.
        assert_eq!(classify(150_000, Some(272_000)), PlayStatus::Listened);
        assert_eq!(classify(261_000, Some(272_000)), PlayStatus::Listened);
        // Four minutes of a very long track → listened.
        assert_eq!(classify(240_000, Some(1_800_000)), PlayStatus::Listened);
        // Under 30 s → skipped.
        assert_eq!(classify(18_000, Some(312_000)), PlayStatus::Skipped);
        // In between → partial.
        assert_eq!(classify(60_000, Some(312_000)), PlayStatus::Partial);
        // Unknown length: only the absolute rules apply.
        assert_eq!(classify(100_000, None), PlayStatus::Partial);
        assert_eq!(classify(10_000, None), PlayStatus::Skipped);
        assert_eq!(classify(250_000, None), PlayStatus::Listened);
        // Zero length must not make everything "listened".
        assert_eq!(classify(1_000, Some(0)), PlayStatus::Skipped);
    }

    #[test]
    fn a_full_play_then_a_track_change() {
        let mut t = Tracker::new();
        let a = track(A, Some(272_000));
        let done = run(
            &mut t,
            &[
                obs(0, PlayerState::Playing, Some(a.clone()), Some(0)),
                obs(272 * S, PlayerState::Playing, Some(track(B, Some(300_000))), Some(0)),
            ],
            300 * S,
        );
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].track.uri, A);
        assert_eq!(done[0].played_ms, 272_000);
        assert_eq!(done[0].started_at, 0);
        assert_eq!(done[0].ended_at, 272 * S);
        assert_eq!(done[0].status(false), PlayStatus::Listened);
        assert_eq!(t.current().unwrap().track.uri, B);
    }

    #[test]
    fn a_skip_is_short_and_classified_skipped() {
        let mut t = Tracker::new();
        let done = run(
            &mut t,
            &[
                obs(0, PlayerState::Playing, Some(track(A, Some(312_000))), Some(0)),
                obs(18 * S, PlayerState::Playing, Some(track(B, None)), Some(0)),
            ],
            20 * S,
        );
        assert_eq!(done[0].played_ms, 18_000);
        assert_eq!(done[0].status(false), PlayStatus::Skipped);
    }

    #[test]
    fn pause_and_resume_stay_one_play_and_pauses_are_not_counted() {
        let mut t = Tracker::new();
        let a = track(A, Some(300_000));
        let done = run(
            &mut t,
            &[
                obs(0, PlayerState::Playing, Some(a.clone()), Some(0)),
                obs(60 * S, PlayerState::Paused, Some(a.clone()), Some(60_000)),
                obs(600 * S, PlayerState::Playing, Some(a.clone()), Some(60_000)),
                obs(660 * S, PlayerState::Paused, Some(a.clone()), Some(120_000)),
                obs(661 * S, PlayerState::Playing, Some(a.clone()), Some(120_000)),
                obs(841 * S, PlayerState::Stopped, None, None),
            ],
            900 * S,
        );
        assert_eq!(done.len(), 1, "one play for pause/resume");
        assert_eq!(done[0].played_ms, 300_000);
        assert_eq!(done[0].started_at, 0);
        assert_eq!(done[0].ended_at, 841 * S);
    }

    #[test]
    fn a_long_pause_closes_the_play_at_the_moment_it_paused() {
        let mut t = Tracker::new();
        let a = track(A, Some(300_000));
        let done = run(
            &mut t,
            &[
                obs(0, PlayerState::Playing, Some(a.clone()), Some(0)),
                obs(100 * S, PlayerState::Paused, Some(a.clone()), Some(100_000)),
            ],
            100 * S + PAUSE_CLOSE_MS + 15 * S,
        );
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].played_ms, 100_000);
        assert_eq!(done[0].ended_at, 100 * S);
        assert!(t.current().is_none());
        // Resuming afterwards is a new play.
        let changes = t.observe(&obs(100 * S + PAUSE_CLOSE_MS + 60 * S, PlayerState::Playing, Some(a), Some(100_000)));
        assert!(matches!(changes.as_slice(), [Change::Update(p)] if p.played_ms == 0));
    }

    #[test]
    fn spotify_quitting_ends_the_play() {
        let mut t = Tracker::new();
        let done = run(
            &mut t,
            &[
                obs(0, PlayerState::Playing, Some(track(A, Some(400_000))), Some(0)),
                obs(95 * S, PlayerState::NotRunning, None, None),
            ],
            200 * S,
        );
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].played_ms, 95_000);
        assert!(t.current().is_none());
    }

    #[test]
    fn repeated_observations_of_the_same_state_never_duplicate() {
        let mut t = Tracker::new();
        let a = track(A, Some(300_000));
        let mut events = vec![];
        for i in 0..10 {
            events.push(obs(i * 10 * S, PlayerState::Playing, Some(a.clone()), Some((i * 10_000) as u64)));
        }
        let done = run(&mut t, &events, 100 * S);
        assert!(done.is_empty());
        assert_eq!(t.current().unwrap().played_ms, 100_000);
        assert_eq!(t.current().unwrap().started_at, 0);
    }

    #[test]
    fn sleep_gaps_are_not_counted_as_listening() {
        let mut t = Tracker::new();
        let a = track(A, Some(300_000));
        t.observe(&obs(0, PlayerState::Playing, Some(a), Some(0)));
        t.tick(15 * S);
        // The Mac slept for an hour; next tick is way later.
        t.tick(3_615 * S);
        assert_eq!(t.current().unwrap().played_ms, 15_000 + MAX_STEP_MS as u64);
    }

    #[test]
    fn repeat_one_counts_each_loop_but_a_seek_back_does_not() {
        let mut t = Tracker::new();
        let a = track(A, Some(200_000));
        t.observe(&obs(0, PlayerState::Playing, Some(a.clone()), Some(0)));
        t.observe(&obs(100 * S, PlayerState::Playing, Some(a.clone()), Some(100_000)));
        // Seeking back to the start mid-track is the same play.
        assert!(finished(&t.observe(&obs(110 * S, PlayerState::Playing, Some(a.clone()), Some(1_000)))).is_empty());
        t.observe(&obs(300 * S, PlayerState::Playing, Some(a.clone()), Some(195_000)));
        // Jumping from the end back to the start is a new loop.
        let done = finished(&t.observe(&obs(306 * S, PlayerState::Playing, Some(a), Some(2_000))));
        assert_eq!(done.len(), 1);
        assert_eq!(t.current().unwrap().started_at, 306 * S);
    }

    #[test]
    fn ads_are_not_history_and_end_the_music_play() {
        let mut t = Tracker::new();
        t.observe(&obs(0, PlayerState::Playing, Some(track(A, None)), None));
        let changes = t.observe(&obs(60 * S, PlayerState::Playing, Some(track("spotify:ad:1", None)), None));
        assert_eq!(finished(&changes).len(), 1);
        assert!(t.current().is_none());
        assert!(t.tick(90 * S).is_empty());
    }

    #[test]
    fn an_entry_left_open_is_continued_when_the_track_is_still_playing() {
        let mut open = Play::new(track(A, Some(300_000)), &obs(0, PlayerState::Playing, None, None));
        open.row_id = Some(7);
        open.played_ms = 120_000;
        open.last_at = 120 * S;
        // Same track, 60 s later: continued, the downtime is not counted.
        let mut t = Tracker::with_open(Some(open.clone()));
        let changes = t.observe(&obs(180 * S, PlayerState::Playing, Some(track(A, Some(300_000))), None));
        assert!(finished(&changes).is_empty());
        assert_eq!(t.current().unwrap().row_id, Some(7));
        assert_eq!(t.current().unwrap().played_ms, 120_000);
        // A different track: the old entry is finished, a new one starts.
        let mut t = Tracker::with_open(Some(open.clone()));
        let done = finished(&t.observe(&obs(180 * S, PlayerState::Playing, Some(track(B, None)), None)));
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].row_id, Some(7));
        assert_eq!(t.current().unwrap().track.uri, B);
        // Too late: finished even for the same track.
        let mut t = Tracker::with_open(Some(open.clone()));
        let done = finished(&t.observe(&obs(120 * S + RESUME_MS + S, PlayerState::Playing, Some(track(A, None)), None)));
        assert_eq!(done.len(), 1);
        assert!(t.current().unwrap().row_id.is_none());
        // Nothing observed at all: a tick past the window finishes it.
        let mut t = Tracker::with_open(Some(open));
        assert!(t.tick(130 * S).is_empty());
        assert_eq!(finished(&t.tick(120 * S + RESUME_MS + S)).len(), 1);
    }

    #[test]
    fn query_answers_parse_with_and_without_a_track() {
        let s = SEP;
        let answer = format!("playing{s}Runaway{s}Kanye West{s}MBDTF{s}spotify:track:3DK{s}547733{s}246908{s}69{s}https://i.scdn.co/image/x\n");
        let (o, vol) = parse_query(&answer, 5).unwrap();
        assert_eq!(o.state, PlayerState::Playing);
        assert_eq!(o.position_ms, Some(246_908));
        assert_eq!(vol, Some(69));
        let t = o.track.unwrap();
        assert_eq!((t.name.as_str(), t.artist.as_str()), ("Runaway", "Kanye West"));
        assert_eq!(t.duration_ms, Some(547_733));
        assert_eq!(t.artwork_url.as_deref(), Some("https://i.scdn.co/image/x"));
        assert_eq!(parse_query("not_running", 1).unwrap().0.state, PlayerState::NotRunning);
        // Stopped with no track left.
        let empty = format!("stopped{s}{s}{s}{s}{s}0{s}0{s}50{s}missing value");
        let (o, _) = parse_query(&empty, 1).unwrap();
        assert_eq!(o.state, PlayerState::Stopped);
        assert!(o.track.is_none());
        assert!(parse_query("garbage", 1).is_none());
    }

    #[test]
    fn notifications_parse_and_tolerate_missing_fields() {
        let o = observation_from_notification(
            |k| match k {
                "Player State" => Some(Field::Text("Paused".into())),
                "Name" => Some(Field::Text("Wicked Games".into())),
                "Artist" => Some(Field::Text("Parra for Cuva".into())),
                "Track ID" => Some(Field::Text("spotify:track:w".into())),
                "Duration" => Some(Field::Number(277_000.0)),
                "Playback Position" => Some(Field::Number(12.5)),
                _ => None,
            },
            9,
        );
        assert_eq!(o.state, PlayerState::Paused);
        assert_eq!(o.position_ms, Some(12_500));
        let t = o.track.unwrap();
        assert_eq!(t.album, "");
        assert_eq!(t.duration_ms, Some(277_000));
        let empty = observation_from_notification(|k| (k == "Player State").then(|| Field::Text("Stopped".into())), 1);
        assert_eq!(empty.state, PlayerState::Stopped);
        assert!(empty.track.is_none());
    }
}
