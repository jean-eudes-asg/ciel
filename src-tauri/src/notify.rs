// Notifications from other apps, to the island (island/notifications.ts):
// what websites show in Chrome, Edge or Brave — the Ciel for Chrome extension
// catches them in the page and its bridge (`ciel-hook --chrome`) hands them
// over the pipe — and new mail in Gmail (gmail.rs).
//
// The island shows each one; Open goes back where it came from: to the tab
// in the browser (the bridge brings it forward and the page gets its click),
// or to a URL (a mail in Gmail). Nothing is written anywhere; what Open needs
// is kept in memory for the last few, and only while Ciel runs.
//
// A bridge keeps one connection open for Ciel's answers (pipe.rs hands it
// here): it learns from it that Ciel is there — the extension then skips the
// browser's own popup — and receives the clicks.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

/// What the island is given.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,
    /// "chrome" or "gmail".
    pub source: String,
    /// The app or site: "Gmail", "WhatsApp"…
    pub app: String,
    pub title: String,
    pub body: String,
    /// A data: URL, or none.
    pub icon: Option<String>,
    /// Unix ms.
    pub at: f64,
    pub tag: Option<String>,
}

/// What Open does for one notification.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Back to the browser: the bridge that sent it, and the extension's key for it.
    Bridge { bridge: String, key: String },
    /// A page to open (a mail in Gmail).
    Url(String),
}

/// The last notifications' targets, newest last.
const KEPT: usize = 64;
/// A browser's notification can't carry more than this into the island.
const MAX_ICON: usize = 200 * 1024;
const MAX_TEXT: usize = 2_000;

static TARGETS: LazyLock<Mutex<VecDeque<(String, Target)>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));
/// The bridges connected now: their id, and where to write to them.
static BRIDGES: LazyLock<Mutex<Vec<(String, mpsc::UnboundedSender<String>)>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static SEQ: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// A new id, for a notification from `source`.
pub fn next_id(source: &str) -> String {
    format!("{source}-{}", SEQ.fetch_add(1, Ordering::Relaxed))
}

/// Shows `n` in the island; Open will go to `target`.
pub fn show(app: &AppHandle, n: Notification, target: Target) {
    {
        let mut targets = TARGETS.lock().unwrap();
        targets.push_back((n.id.clone(), target));
        while targets.len() > KEPT {
            targets.pop_front();
        }
    }
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "notification", &n);
}

/// A notification from a browser bridge, as the pipe received it:
/// `{"ciel_notification": {key, app, title, body, icon, tag}, "ciel_bridge": "<id>"}`.
/// None when it is not one, or has nothing to show.
pub fn from_bridge(payload: &Value) -> Option<(Notification, Target)> {
    let n = payload.get("ciel_notification")?;
    let bridge = payload.get("ciel_bridge")?.as_str()?.to_string();
    let text = |k: &str| n.get(k).and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let (title, body) = (clip(text("title"), MAX_TEXT), clip(text("body"), MAX_TEXT));
    let key = text("key");
    if (title.is_empty() && body.is_empty()) || key.is_empty() || bridge.is_empty() {
        return None;
    }
    let icon = n
        .get("icon")
        .and_then(Value::as_str)
        .filter(|i| i.starts_with("data:image/") && i.len() <= MAX_ICON)
        .map(String::from);
    let app = match text("app") {
        "" => "Chrome".to_string(),
        a => clip(a, 80),
    };
    let tag = Some(text("tag")).filter(|t| !t.is_empty()).map(|t| clip(t, 200));
    let notification = Notification {
        id: next_id("chrome"),
        source: "chrome".into(),
        app,
        title,
        body,
        icon,
        at: now_ms(),
        tag,
    };
    Some((notification, Target::Bridge { bridge, key: clip(key, 300) }))
}

/// The island's Open or Dismiss.
#[tauri::command]
pub fn notification_action(id: String, action: String) {
    let target = TARGETS.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, t)| t.clone());
    let Some(target) = target else { return };
    match (target, action.as_str()) {
        (Target::Bridge { bridge, key }, "open") => tell_bridge(&bridge, json!({"type": "click", "key": key})),
        (Target::Bridge { bridge, key }, "dismiss") => tell_bridge(&bridge, json!({"type": "close", "key": key})),
        (Target::Url(url), "open") => crate::platform::open_url(&url),
        _ => {}
    }
}

fn tell_bridge(bridge: &str, message: Value) {
    let line = format!("{message}\n");
    if let Some((_, tx)) = BRIDGES.lock().unwrap().iter().find(|(id, _)| id == bridge) {
        let _ = tx.send(line);
    }
}

/// A bridge said hello (`{"ciel_listen": "chrome", "ciel_bridge": "<id>"}`):
/// it is told Ciel is there, then gets the clicks until it goes away. Returns
/// the receiver its connection writes from; the caller drops the bridge with
/// `forget` once the connection ends.
pub fn listen(bridge: &str) -> mpsc::UnboundedReceiver<String> {
    let (tx, rx) = mpsc::unbounded_channel();
    let _ = tx.send(format!("{}\n", json!({"type": "status", "ciel": true})));
    let mut bridges = BRIDGES.lock().unwrap();
    bridges.retain(|(id, _)| id != bridge);
    bridges.push((bridge.to_string(), tx));
    crate::log::line(format!("notifications: browser bridge connected ({} now)", bridges.len()));
    rx
}

pub fn forget(bridge: &str) {
    let mut bridges = BRIDGES.lock().unwrap();
    bridges.retain(|(id, _)| id != bridge);
    crate::log::line(format!("notifications: browser bridge gone ({} left)", bridges.len()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_browsers_notification_becomes_the_islands() {
        let payload = json!({
            "ciel_bridge": "b1",
            "ciel_notification": {"key": "12:abc", "app": "WhatsApp", "title": " Ana ", "body": "Lunch?", "icon": "data:image/png;base64,AAAA", "tag": "chat-1"},
        });
        let (n, target) = from_bridge(&payload).unwrap();
        assert_eq!(n.source, "chrome");
        assert_eq!(n.app, "WhatsApp");
        assert_eq!(n.title, "Ana");
        assert_eq!(n.body, "Lunch?");
        assert_eq!(n.icon.as_deref(), Some("data:image/png;base64,AAAA"));
        assert_eq!(n.tag.as_deref(), Some("chat-1"));
        assert!(n.id.starts_with("chrome-"));
        assert_eq!(target, Target::Bridge { bridge: "b1".into(), key: "12:abc".into() });
    }

    #[test]
    fn nothing_to_show_or_no_way_back_is_no_notification() {
        assert!(from_bridge(&json!({"hook_event_name": "Stop"})).is_none());
        assert!(from_bridge(&json!({"ciel_bridge": "b1", "ciel_notification": {"key": "k", "title": "", "body": " "}})).is_none());
        assert!(from_bridge(&json!({"ciel_notification": {"key": "k", "title": "Hi"}})).is_none(), "no bridge");
        assert!(from_bridge(&json!({"ciel_bridge": "b1", "ciel_notification": {"title": "Hi"}})).is_none(), "no key");
    }

    #[test]
    fn only_image_data_urls_come_through_as_icons_and_text_is_bounded() {
        let long = "x".repeat(5_000);
        let payload = json!({
            "ciel_bridge": "b1",
            "ciel_notification": {"key": "k", "title": long, "icon": "https://tracker.example/pixel.png"},
        });
        let (n, _) = from_bridge(&payload).unwrap();
        assert_eq!(n.icon, None);
        assert_eq!(n.title.len(), MAX_TEXT);
        assert_eq!(n.app, "Chrome");
    }

    #[test]
    fn a_bridge_hears_that_ciel_is_there_then_its_clicks() {
        let mut rx = listen("test-bridge");
        let next = |rx: &mut mpsc::UnboundedReceiver<String>| serde_json::from_str::<Value>(rx.try_recv().unwrap().trim()).unwrap();
        assert_eq!(next(&mut rx), json!({"type": "status", "ciel": true}));
        TARGETS.lock().unwrap().push_back(("chrome-x".into(), Target::Bridge { bridge: "test-bridge".into(), key: "7:t".into() }));
        notification_action("chrome-x".into(), "open".into());
        assert_eq!(next(&mut rx), json!({"type": "click", "key": "7:t"}));
        notification_action("chrome-x".into(), "dismiss".into());
        assert_eq!(next(&mut rx), json!({"type": "close", "key": "7:t"}));
        forget("test-bridge");
        notification_action("chrome-x".into(), "open".into());
        assert!(rx.try_recv().is_err(), "a bridge gone hears nothing");
    }
}
