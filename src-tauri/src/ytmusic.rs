// YouTube Music's player, for its pill (src/core/ytmusic.ts). Ciel for Chrome
// reads it in the music.youtube.com tab (extension/media.js) and its bridge
// hands it over the pipe as `ciel_media`, as it hands notifications over
// (notify.rs); the pill's buttons go back the same way, to that tab, where
// they do what the media keys do. The page gets it in Spotify's shape
// (spotify.rs PlayerState), so the same card shows it.
//
// Several tabs: the pill follows the one that plays, else the one heard from
// last. On Windows, while the pill has a player, the music pill leaves the
// browsers' media sessions to it, so a song doesn't show twice.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::spotify::{PlayerState, Track};

pub const PILL_ID: &str = "integration_youtube_music";
const EVENT: &str = "ytmusic";
/// The cover is a small JPEG made by the extension; a bigger one is no cover.
const MAX_ART: usize = 64 * 1024;
const MAX_TEXT: usize = 300;

/// What a tab last said.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub bridge: String,
    /// The tab, as the extension knows it.
    pub key: String,
    /// The tab closed, or went elsewhere than YouTube Music.
    pub gone: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// A data URL.
    pub art: Option<String>,
    /// Seconds; 0 when it has no length.
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
    /// When it was said (Unix ms): while playing, the position runs on from there.
    pub at: f64,
}

static PLAYER: Mutex<Option<Report>> = Mutex::new(None);
/// The pill is declared (Settings → Pills).
static PILL_ON: AtomicBool = AtomicBool::new(false);

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// A bridge's `{"ciel_media": {…}, "ciel_bridge": id}`, or None for anything else.
pub fn from_bridge(payload: &Value) -> Option<Report> {
    let m = payload.get("ciel_media")?;
    let bridge = payload.get("ciel_bridge")?.as_str()?.trim().to_string();
    let key = m.get("key")?.as_str()?.trim().to_string();
    if bridge.is_empty() || key.is_empty() {
        return None;
    }
    let text = |k: &str| clip(m.get(k).and_then(Value::as_str).unwrap_or_default().trim(), MAX_TEXT);
    let seconds = |k: &str| m.get(k).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0).unwrap_or(0.0);
    let art = m
        .get("art")
        .and_then(Value::as_str)
        .filter(|a| a.len() <= MAX_ART && (a.starts_with("data:image/jpeg;base64,") || a.starts_with("data:image/png;base64,")))
        .map(String::from);
    Some(Report {
        bridge,
        key,
        gone: m.get("gone").and_then(Value::as_bool) == Some(true),
        title: text("title"),
        artist: text("artist"),
        album: text("album"),
        art,
        duration: seconds("duration"),
        position: seconds("position"),
        playing: m.get("playing").and_then(Value::as_bool) == Some(true),
        at: now_ms(),
    })
}

/// The player the pill follows after `next`: the tab that plays, else the one
/// heard from last; a tab that says it is gone takes its player with it.
pub fn follow(current: Option<Report>, next: Report) -> Option<Report> {
    let same = |p: &Report| p.bridge == next.bridge && p.key == next.key;
    if next.gone {
        return current.filter(|p| !same(p));
    }
    if next.title.is_empty() {
        return current;
    }
    match current {
        Some(p) if !same(&p) && p.playing && !next.playing => Some(p),
        _ => Some(next),
    }
}

/// The page's copy: Spotify's shape, with what a browser tab can do.
pub fn state(player: Option<&Report>, installed: bool) -> PlayerState {
    let Some(p) = player else {
        return PlayerState { installed, ..PlayerState::default() };
    };
    PlayerState {
        running: true,
        installed: true,
        track: Some(Track {
            id: format!("ytm:{}\u{1f}{}\u{1f}{}", p.title, p.artist, p.album),
            title: p.title.clone(),
            artist: p.artist.clone(),
            album: p.album.clone(),
            duration: p.duration,
            art_url: p.art.clone(),
            object_path: String::new(),
        }),
        playing: p.playing,
        position: p.position,
        position_at: p.at,
        shuffle: false,
        repeat: false,
        volume: 0,
        // The pill's own name: nothing to add under the title.
        player: String::new(),
        spotify: false,
        can_seek: p.duration > 0.0,
        can_shuffle: false,
        can_repeat: false,
        has_volume: false,
    }
}

fn current_state() -> PlayerState {
    state(PLAYER.lock().unwrap().as_ref(), crate::notify::bridge_count() > 0)
}

/// Tells the island, and Windows' music pill when the browsers changed hands.
fn emit(app: &AppHandle) {
    let _ = app.emit_to(crate::island::WINDOW_LABEL, EVENT, current_state());
}

/// What a tab said.
pub fn update(app: &AppHandle, report: Report) {
    let (had, has) = {
        let mut player = PLAYER.lock().unwrap();
        let had = player.is_some();
        *player = follow(player.take(), report);
        (had, player.is_some())
    };
    emit(app);
    if had != has {
        crate::spotify::players_changed();
    }
}

/// A browser bridge came or went: whether the extension is there is part of
/// the idle card, and a gone bridge takes its tabs' player with it.
pub fn bridges_changed(app: &AppHandle, gone: Option<&str>) {
    if let Some(bridge) = gone {
        let mut player = PLAYER.lock().unwrap();
        if player.as_ref().is_some_and(|p| p.bridge == bridge) {
            *player = None;
            drop(player);
            crate::spotify::players_changed();
        }
    }
    emit(app);
}

/// The pill is declared or not (at launch, after every settings save).
pub fn sync(active_integrations: &[String]) {
    let on = active_integrations.iter().any(|id| id == PILL_ID);
    if PILL_ON.swap(on, Ordering::Relaxed) != on {
        crate::spotify::players_changed();
    }
}

/// Windows' music pill leaves the browsers' sessions to this one meanwhile.
pub fn claims_browsers() -> bool {
    PILL_ON.load(Ordering::Relaxed) && PLAYER.lock().unwrap().is_some()
}

#[tauri::command]
pub fn ytmusic_state() -> PlayerState {
    current_state()
}

/// A button on the card or the pill, to the tab: playPause, next, previous,
/// seek (to `value` seconds), or focus (the tab comes forward).
#[tauri::command]
pub fn ytmusic_control(action: String, value: Option<f64>) {
    const ACTIONS: [&str; 5] = ["playPause", "next", "previous", "seek", "focus"];
    if !ACTIONS.contains(&action.as_str()) {
        return;
    }
    let Some((bridge, key)) = PLAYER.lock().unwrap().as_ref().map(|p| (p.bridge.clone(), p.key.clone())) else { return };
    let value = value.filter(|v| v.is_finite() && *v >= 0.0);
    crate::notify::tell_bridge(&bridge, json!({"type": "media-control", "key": key, "action": action, "value": value}));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(bridge: &str, key: &str, title: &str, playing: bool) -> Report {
        Report {
            bridge: bridge.into(),
            key: key.into(),
            gone: false,
            title: title.into(),
            artist: "Ana".into(),
            album: String::new(),
            art: None,
            duration: 200.0,
            position: 3.0,
            playing,
            at: 1000.0,
        }
    }

    #[test]
    fn a_tabs_player_arrives_whole_and_nothing_else_passes() {
        let payload = json!({"ciel_bridge": "b1", "ciel_media": {"key": "12", "title": " Song ", "artist": "Ana",
            "duration": 213.5, "position": -4, "playing": true, "art": "https://evil.example/x.png"}});
        let r = from_bridge(&payload).unwrap();
        assert_eq!((r.bridge.as_str(), r.key.as_str(), r.title.as_str(), r.duration, r.position, r.playing), ("b1", "12", "Song", 213.5, 0.0, true));
        assert_eq!(r.art, None, "only a small data URL is a cover");
        let ok = json!({"ciel_bridge": "b1", "ciel_media": {"key": "12", "title": "S", "art": "data:image/jpeg;base64,AAAA"}});
        assert_eq!(from_bridge(&ok).unwrap().art.as_deref(), Some("data:image/jpeg;base64,AAAA"));
        assert!(from_bridge(&json!({"ciel_bridge": "b1", "ciel_media": {"title": "no key"}})).is_none());
        assert!(from_bridge(&json!({"ciel_notification": {}})).is_none());
    }

    #[test]
    fn the_pill_follows_the_tab_that_plays_else_the_last_one() {
        let a = follow(None, said("b", "1", "A", true));
        assert_eq!(a.as_ref().unwrap().key, "1");
        // Another tab, paused: the playing one stays.
        let still = follow(a.clone(), said("b", "2", "B", false));
        assert_eq!(still.as_ref().unwrap().key, "1");
        // The same tab pauses: it is still followed.
        let paused = follow(still, said("b", "1", "A", false));
        assert!(!paused.as_ref().unwrap().playing);
        // Now the other one plays: it takes over.
        let other = follow(paused, said("b", "2", "B", true));
        assert_eq!(other.as_ref().unwrap().key, "2");
        // A gone tab that isn't followed changes nothing; the followed one ends it.
        let mut gone = said("b", "1", "", false);
        gone.gone = true;
        assert_eq!(follow(other.clone(), gone.clone()).unwrap().key, "2");
        gone.key = "2".into();
        assert_eq!(follow(other, gone), None);
        // Nothing loaded yet: no player made of nothing.
        assert_eq!(follow(None, said("b", "3", "", true)), None);
    }

    #[test]
    fn the_page_gets_spotifys_shape_with_what_a_tab_can_do() {
        let idle = state(None, true);
        assert!(!idle.running && idle.installed && idle.track.is_none());
        let mut r = said("b", "1", "Song", true);
        r.art = Some("data:image/jpeg;base64,AAAA".into());
        let s = state(Some(&r), false);
        assert!(s.running && s.installed && s.playing && s.can_seek);
        assert!(!s.spotify && !s.can_shuffle && !s.can_repeat && !s.has_volume);
        assert_eq!(s.player, "", "the pill's own name, nothing added under the title");
        let t = s.track.unwrap();
        assert_eq!((t.title.as_str(), t.duration, t.art_url.as_deref()), ("Song", 200.0, Some("data:image/jpeg;base64,AAAA")));
        r.duration = 0.0;
        assert!(!state(Some(&r), true).can_seek, "a live stream can't be sought");
    }
}
