// Notifications from other apps, to the island (island/notifications.ts):
// what websites show in Chrome, Edge or Brave — the Ciel for Chrome extension
// catches them in the page and its bridge (`ciel-hook --chrome`) hands them
// over the pipe — new mail in Gmail (gmail.rs) and, on Linux, what the other
// apps of the desktop notify (`desktop` below).
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
    /// "chrome", "gmail", "desktop" or "alerts".
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
    /// A page of Ciel's settings (a new version of Ciel: General).
    Settings(String),
    /// Nothing to open: an alert that came without a page (alerts.rs).
    None,
    /// The app that notified: its desktop entry id, the process that sent it
    /// (whose window comes forward), and GNOME's own copy of it once known.
    #[cfg(target_os = "linux")]
    App { entry: String, pid: Option<u32>, shell: Option<ShellRef> },
}

/// GNOME Shell's copy of a notification, which its extension can open or
/// dismiss (gnome_ext.rs).
#[cfg(target_os = "linux")]
#[derive(Clone, Debug, PartialEq)]
pub enum ShellRef {
    /// Sent to org.freedesktop.Notifications: the id the server answered.
    Fdo(u32),
    /// Sent to org.gtk.Notifications: the app's id and its own.
    Gtk { app: String, id: String },
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

/// GNOME's id for a notification already shown, once the server has answered.
#[cfg(target_os = "linux")]
fn set_shell(id: &str, server: ShellRef) {
    if let Some((_, Target::App { shell, .. })) = TARGETS.lock().unwrap().iter_mut().find(|(i, _)| i == id) {
        *shell = Some(server);
    }
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
pub fn notification_action(app: AppHandle, id: String, action: String) {
    act(&id, &action, |page| crate::open_settings(&app, page));
}

/// Open or Dismiss on the notification `id`; `open_settings` shows a page of
/// Ciel's settings (its own notifications' Open).
fn act(id: &str, action: &str, open_settings: impl FnOnce(&str)) {
    let target = TARGETS.lock().unwrap().iter().find(|(i, _)| *i == id).map(|(_, t)| t.clone());
    let Some(target) = target else { return };
    match (target, action) {
        (Target::Bridge { bridge, key }, "open") => tell_bridge(&bridge, json!({"type": "click", "key": key})),
        (Target::Bridge { bridge, key }, "dismiss") => tell_bridge(&bridge, json!({"type": "close", "key": key})),
        (Target::Url(url), "open") => crate::platform::open_url(&url),
        (Target::Settings(page), "open") => open_settings(&page),
        // D-Bus, X11 and a launch: off the UI thread.
        #[cfg(target_os = "linux")]
        (Target::App { entry, pid, shell }, action) => {
            let open = action == "open";
            std::thread::spawn(move || desktop::act(&entry, pid, shell.as_ref(), open));
        }
        _ => {}
    }
}

pub(crate) fn tell_bridge(bridge: &str, message: Value) {
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

/// How many browsers have their bridge connected (YouTube Music's idle card).
pub fn bridge_count() -> usize {
    BRIDGES.lock().unwrap().len()
}

pub fn forget(bridge: &str) {
    let mut bridges = BRIDGES.lock().unwrap();
    bridges.retain(|(id, _)| id != bridge);
    crate::log::line(format!("notifications: browser bridge gone ({} left)", bridges.len()));
}

/// Linux: what the other apps of the desktop notify (Settings → Integrations →
/// Notifications), read off the session bus as a monitor: the calls to the
/// notification server (org.freedesktop.Notifications, where the portal sends
/// sandboxed apps' too) and to GNOME's own (org.gtk.Notifications). The desktop
/// keeps its own: the switch turns GNOME's banners off (Do Not Disturb), and
/// these still come.
#[cfg(target_os = "linux")]
pub mod desktop {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Duration;

    use dbus::arg::{PropMap, RefArg};
    use dbus::blocking::Connection;
    use dbus::message::MessageType;
    use dbus::Message;
    use gtk::gio::prelude::{AppInfoExt, IconExt};
    use gtk::gio::{AppLaunchContext, DesktopAppInfo};
    use tauri::AppHandle;

    use super::{clip, next_id, now_ms, show, Notification, ShellRef, Target, MAX_TEXT};

    const FREEDESKTOP: &str = "org.freedesktop.Notifications";
    const GTK: &str = "org.gtk.Notifications";
    // Matched by interface, whoever the call is addressed to: apps like Discord
    // send to the server's unique name, not the well-known one. GNOME's
    // notification daemon passes each call on to the shell, marked with
    // x-shell-sender: that copy is not a second notification.
    // The server's answers give the id GNOME's extension knows the
    // notification by.
    const RULES: [&str; 3] = [
        "type='method_call',interface='org.freedesktop.Notifications',member='Notify'",
        "type='method_call',interface='org.gtk.Notifications',member='AddNotification'",
        "type='method_return',sender='org.freedesktop.Notifications'",
    ];
    /// Calls waiting for the server's answer: the caller, the call's serial, our id.
    const PENDING: usize = 32;

    static ON: AtomicBool = AtomicBool::new(false);
    static GENERATION: AtomicU64 = AtomicU64::new(0);

    pub fn sync(app: &AppHandle, on: bool) {
        if ON.swap(on, Ordering::SeqCst) == on {
            return;
        }
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        if !on {
            return;
        }
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("ciel-notifications".into())
            .spawn(move || listen(&app, generation));
        if let Err(err) = spawned {
            crate::log::line(format!("notifications: no desktop listener: {err}"));
        }
    }

    fn current(generation: u64) -> bool {
        ON.load(Ordering::SeqCst) && GENERATION.load(Ordering::SeqCst) == generation
    }

    fn listen(app: &AppHandle, generation: u64) {
        let conn = match Connection::new_session() {
            Ok(c) => c,
            Err(err) => {
                crate::log::line(format!("notifications: no session bus: {err}"));
                return;
            }
        };
        let bus = conn.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_secs(5));
        let monitor: Result<(), _> = bus.method_call("org.freedesktop.DBus.Monitoring", "BecomeMonitor", (RULES.to_vec(), 0u32));
        if let Err(err) = monitor {
            crate::log::line(format!("notifications: the bus won't let Ciel listen: {err}"));
            return;
        }
        crate::log::line("notifications: listening to the desktop's");
        // A monitor can't ask anything: who sent a notification is asked on a second connection.
        let ask = Connection::new_session().ok();
        let mut pending: std::collections::VecDeque<(String, u32, String)> = std::collections::VecDeque::new();
        // ponytail: a monitor can't be woken, so once turned off the thread ends at its next wait (30 s at most).
        while current(generation) {
            match conn.channel().blocking_pop_message(Duration::from_secs(30)) {
                Ok(Some(msg)) if msg.msg_type() == MessageType::MethodReturn => {
                    let (Some(to), Some(serial)) = (msg.destination(), msg.get_reply_serial()) else { continue };
                    let Some(at) = pending.iter().position(|(d, s, _)| **d == *to && *s == serial) else { continue };
                    let (_, _, id) = pending.remove(at).unwrap();
                    if let Ok(server) = msg.read1::<u32>() {
                        super::set_shell(&id, ShellRef::Fdo(server));
                    }
                }
                Ok(Some(msg)) => {
                    if let Some((mut n, mut target, icons)) = from_bus(&msg) {
                        n.icon = icon_url(app, icons);
                        let sender = msg.sender().map(|s| s.to_string()).unwrap_or_default();
                        if let Target::App { pid, .. } = &mut target {
                            *pid = ask.as_ref().and_then(|c| pid_of(c, &sender));
                        }
                        if msg.interface().is_some_and(|i| &*i == FREEDESKTOP) {
                            pending.push_back((sender, msg.get_serial().unwrap_or_default(), n.id.clone()));
                            if pending.len() > PENDING {
                                pending.pop_front();
                            }
                        }
                        if current(generation) {
                            show(app, n, target);
                        }
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    crate::log::line(format!("notifications: bus lost: {err}"));
                    break;
                }
            }
        }
    }

    fn pid_of(conn: &Connection, name: &str) -> Option<u32> {
        let bus = conn.with_proxy("org.freedesktop.DBus", "/org/freedesktop/DBus", Duration::from_secs(1));
        let (pid,): (u32,) = bus.method_call("org.freedesktop.DBus", "GetConnectionUnixProcessID", (name,)).ok()?;
        Some(pid)
    }

    /// Where a notification's picture may come from, best first.
    #[derive(Debug, PartialEq)]
    pub(super) enum IconFrom {
        /// The spec's image-data: width, height, rowstride, alpha, bits, the bytes.
        Pixels { width: i32, height: i32, rowstride: i32, alpha: bool, bits: i32, data: Vec<u8> },
        /// A file (a path or a file:// URI) or an icon theme's name.
        Named(String),
    }

    /// A notification call seen on the bus, as the island's, with where its
    /// picture may come from; None for anything else.
    pub(super) fn from_bus(msg: &Message) -> Option<(Notification, Target, Vec<IconFrom>)> {
        if msg.msg_type() != MessageType::MethodCall {
            return None;
        }
        let text = |map: &PropMap, k: &str| map.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let mut icons = Vec::new();
        let mut shell = None;
        let (app, entry, title, body) = match &*msg.interface()? {
            FREEDESKTOP => {
                let mut args = msg.iter_init();
                let app: String = args.read().ok()?;
                let _replaces: u32 = args.read().ok()?;
                let app_icon: String = args.read().ok()?;
                let title: String = args.read().ok()?;
                let body: String = args.read().ok()?;
                let _actions: Vec<String> = args.read().ok()?;
                let hints: PropMap = args.read().ok()?;
                if hints.contains_key("x-shell-sender") {
                    return None;
                }
                // The spec's names, then the older ones.
                let hint = |keys: &[&str]| keys.iter().find_map(|k| hints.get(*k));
                icons.extend(hint(&["image-data", "image_data", "icon_data"]).and_then(|v| pixels(&*v.0)));
                icons.extend(hint(&["image-path", "image_path"]).and_then(|v| v.as_str()).map(|p| IconFrom::Named(p.to_string())));
                if !app_icon.trim().is_empty() {
                    icons.push(IconFrom::Named(app_icon.trim().to_string()));
                }
                (app, text(&hints, "desktop-entry"), title, plain(&body))
            }
            GTK => {
                let (entry, id, fields): (String, String, PropMap) = msg.read3().ok()?;
                shell = Some(ShellRef::Gtk { app: entry.clone(), id });
                (String::new(), entry, text(&fields, "title"), text(&fields, "body"))
            }
            _ => return None,
        };
        let (title, body) = (clip(title.trim(), MAX_TEXT), clip(body.trim(), MAX_TEXT));
        if title.is_empty() && body.is_empty() {
            return None;
        }
        // Sandboxed apps come without a name: their desktop entry has one,
        // and the app's own icon after the notification's.
        // No desktop entry said: the app's name may be one ("discord").
        let entry = if entry.is_empty() && app_info(&app.trim().to_lowercase()).is_some() { app.trim().to_lowercase() } else { entry };
        let info = app_info(&entry);
        icons.extend(info.as_ref().and_then(|i| i.icon()).and_then(|i| IconExt::to_string(&i)).map(|i| IconFrom::Named(i.to_string())));
        let app = match app.trim() {
            "" => info.map(|i| i.name().to_string()).unwrap_or_else(|| entry.clone()),
            a => a.to_string(),
        };
        let id = if entry.is_empty() { app.to_lowercase() } else { entry };
        let notification = Notification {
            id: next_id("desktop"),
            source: "desktop".into(),
            app: clip(&app, 80),
            title,
            body,
            icon: None,
            at: now_ms(),
            tag: None,
        };
        Some((notification, Target::App { entry: id, pid: None, shell }, icons))
    }

    /// image-data, a (iiibiiay): checked before GdkPixbuf reads it.
    fn pixels(v: &dyn RefArg) -> Option<IconFrom> {
        let mut it = v.as_iter()?;
        let width = it.next()?.as_i64()? as i32;
        let height = it.next()?.as_i64()? as i32;
        let rowstride = it.next()?.as_i64()? as i32;
        let alpha = it.next()?.as_i64()? != 0;
        let bits = it.next()?.as_i64()? as i32;
        let channels = it.next()?.as_i64()? as i32;
        let data: Vec<u8> = it.next()?.as_iter()?.filter_map(|b| b.as_u64()).map(|b| b as u8).collect();
        let fits = (1..=1024).contains(&width)
            && (1..=1024).contains(&height)
            && bits == 8
            && channels == if alpha { 4 } else { 3 }
            && rowstride >= width * channels
            && data.len() >= (rowstride * (height - 1) + width * channels) as usize;
        fits.then_some(IconFrom::Pixels { width, height, rowstride, alpha, bits, data })
    }

    /// The pixel size icons are drawn at: crisp in the list's 28 px on a 2x screen.
    const ICON_PX: i32 = 64;

    /// The first picture that can be drawn, as a PNG data URL. GTK's icon theme
    /// is the main thread's, so the drawing happens there.
    fn icon_url(app: &AppHandle, icons: Vec<IconFrom>) -> Option<String> {
        if icons.is_empty() {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        app.run_on_main_thread(move || {
            let _ = tx.send(icons.iter().find_map(render));
        })
        .ok()?;
        rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
    }

    fn render(from: &IconFrom) -> Option<String> {
        use base64::Engine as _;
        use gtk::gdk_pixbuf::{Colorspace, InterpType, Pixbuf};
        use gtk::prelude::IconThemeExt;
        let pixbuf = match from {
            IconFrom::Pixels { width, height, rowstride, alpha, bits, data } => {
                let full = Pixbuf::from_bytes(&gtk::glib::Bytes::from(data.as_slice()), Colorspace::Rgb, *alpha, *bits, *width, *height, *rowstride);
                let scale = ICON_PX as f64 / (*width).max(*height) as f64;
                let (w, h) = (((*width as f64 * scale).round() as i32).max(1), ((*height as f64 * scale).round() as i32).max(1));
                full.scale_simple(w, h, InterpType::Bilinear)?
            }
            IconFrom::Named(name) => {
                let name = name.strip_prefix("file://").unwrap_or(name);
                if name.starts_with('/') {
                    Pixbuf::from_file_at_size(name, ICON_PX, ICON_PX).ok()?
                } else {
                    gtk::IconTheme::default()?.load_icon(name, ICON_PX, gtk::IconLookupFlags::FORCE_SIZE).ok()??
                }
            }
        };
        let png = pixbuf.save_to_bufferv("png", &[]).ok()?;
        Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
    }

    /// The body's markup (the spec allows <b>, <i>, <a>...) as plain text.
    fn plain(body: &str) -> String {
        let mut out = String::with_capacity(body.len());
        let mut in_tag = false;
        for c in body.chars() {
            match c {
                '<' => in_tag = true,
                '>' if in_tag => in_tag = false,
                _ if !in_tag => out.push(c),
                _ => {}
            }
        }
        out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
    }

    fn app_info(id: &str) -> Option<DesktopAppInfo> {
        if id.is_empty() {
            return None;
        }
        // A snap is "snap.discord" to the portal, often just "discord" to
        // itself, and discord_discord.desktop on disk.
        let snap = id.strip_prefix("snap.").unwrap_or(id);
        DesktopAppInfo::new(&format!("{id}.desktop")).or_else(|| DesktopAppInfo::new(&format!("{snap}_{snap}.desktop")))
    }

    const BANNERS: [&str; 2] = ["org.gnome.desktop.notifications", "show-banners"];

    /// The switch, flipped: GNOME's banners go while Ciel shows the
    /// notifications, and come back after, unless they were off already. A
    /// file says Ciel was the one to turn them off. Also when Ciel quits
    /// (back on at its next start), and after a crash the file still says so.
    /// Elsewhere than GNOME, nothing happens.
    pub fn quiet_desktop(on: bool) {
        let marker = crate::platform::config_dir().join("banners-off-by-ciel");
        if on {
            if gsettings(&["get", BANNERS[0], BANNERS[1]]).as_deref() == Some("true")
                && gsettings(&["set", BANNERS[0], BANNERS[1], "false"]).is_some()
            {
                let _ = std::fs::write(&marker, "");
            }
        } else if marker.exists() {
            gsettings(&["set", BANNERS[0], BANNERS[1], "true"]);
            let _ = std::fs::remove_file(&marker);
        }
    }

    fn gsettings(args: &[&str]) -> Option<String> {
        let out = std::process::Command::new("gsettings").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Open, as GNOME's list does through Ciel's extension: the app goes to
    /// the very chat or page. Without it, the app's window comes forward, or
    /// the app starts. Dismiss takes GNOME's copy away too.
    pub fn act(entry: &str, pid: Option<u32>, shell: Option<&ShellRef>, open: bool) {
        if shell.is_some_and(|s| crate::gnome_ext::act(s, open)) || !open {
            return;
        }
        if pid.is_some_and(|p| crate::platform::focus_session_window(&[p], "")) {
            return;
        }
        launch(entry);
    }

    fn launch(id: &str) {
        let Some(info) = app_info(id) else {
            crate::log::line(format!("notifications: no app {id} to open"));
            return;
        };
        if let Err(err) = info.launch(&[], None::<&AppLaunchContext>) {
            crate::log::line(format!("notifications: could not open {id}: {err}"));
        }
    }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn a_desktop_apps_notification_becomes_the_islands() {
        use dbus::arg::{PropMap, RefArg, Variant};
        use dbus::Message;
        let call = |dest: &str| {
            let mut hints = PropMap::new();
            hints.insert("desktop-entry".into(), Variant(Box::new("thunderbird".to_string()) as Box<dyn RefArg>));
            if dest == ":1.9" {
                hints.insert("x-shell-sender".into(), Variant(Box::new(":1.42".to_string()) as Box<dyn RefArg>));
            }
            Message::new_method_call(dest, "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Notify")
                .unwrap()
                .append3("Thunderbird", 0u32, "")
                .append3("Ana", "Lunch <b>at 1</b> &amp; coffee?", Vec::<String>::new())
                .append2(hints, -1i32)
        };
        let (n, target, _) = desktop::from_bus(&call("org.freedesktop.Notifications")).unwrap();
        assert_eq!((n.source.as_str(), n.app.as_str(), n.title.as_str()), ("desktop", "Thunderbird", "Ana"));
        assert_eq!(n.body, "Lunch at 1 & coffee?");
        assert!(n.id.starts_with("desktop-"));
        assert_eq!(target, Target::App { entry: "thunderbird".into(), pid: None, shell: None });
        // Sent to the server's unique name, as Discord does: the same notification.
        assert_eq!(desktop::from_bus(&call(":1.42")).unwrap().0.title, "Ana");
        // GNOME's daemon handing it on to the shell is not a second one.
        assert!(desktop::from_bus(&call(":1.9")).is_none());

        let mut fields = PropMap::new();
        fields.insert("title".into(), Variant(Box::new("Meeting".to_string()) as Box<dyn RefArg>));
        let gtk = Message::new_method_call("org.gtk.Notifications", "/org/gtk/Notifications", "org.gtk.Notifications", "AddNotification")
            .unwrap()
            .append3("org.example.NoSuchCalendar", "n1", fields);
        let (n, target, _) = desktop::from_bus(&gtk).unwrap();
        assert_eq!((n.app.as_str(), n.title.as_str(), n.body.as_str()), ("org.example.NoSuchCalendar", "Meeting", ""));
        let shell = Some(ShellRef::Gtk { app: "org.example.NoSuchCalendar".into(), id: "n1".into() });
        assert_eq!(target, Target::App { entry: "org.example.NoSuchCalendar".into(), pid: None, shell });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_notifications_pictures_come_best_first_and_bad_pixels_are_refused() {
        use desktop::IconFrom;
        use dbus::arg::{PropMap, RefArg, Variant};
        use dbus::Message;
        let call = |data_len: usize| {
            let mut hints = PropMap::new();
            let image = (2i32, 1i32, 8i32, true, 8i32, 4i32, vec![7u8; data_len]);
            hints.insert("image-data".into(), Variant(Box::new(image) as Box<dyn RefArg>));
            hints.insert("image-path".into(), Variant(Box::new("/tmp/avatar.png".to_string()) as Box<dyn RefArg>));
            Message::new_method_call("org.freedesktop.Notifications", "/org/freedesktop/Notifications", "org.freedesktop.Notifications", "Notify")
                .unwrap()
                .append3("Chat", 0u32, "chat-app")
                .append3("Ana", "Hi", Vec::<String>::new())
                .append2(hints, -1i32)
        };
        let (_, _, icons) = desktop::from_bus(&call(8)).unwrap();
        assert_eq!(
            icons,
            [
                IconFrom::Pixels { width: 2, height: 1, rowstride: 8, alpha: true, bits: 8, data: vec![7; 8] },
                IconFrom::Named("/tmp/avatar.png".into()),
                IconFrom::Named("chat-app".into()),
            ]
        );
        // Fewer bytes than the picture says: never handed to GdkPixbuf.
        let (_, _, icons) = desktop::from_bus(&call(3)).unwrap();
        assert!(!icons.iter().any(|i| matches!(i, IconFrom::Pixels { .. })));
    }

    #[test]
    fn a_bridge_hears_that_ciel_is_there_then_its_clicks() {
        let mut rx = listen("test-bridge");
        let next = |rx: &mut mpsc::UnboundedReceiver<String>| serde_json::from_str::<Value>(rx.try_recv().unwrap().trim()).unwrap();
        assert_eq!(next(&mut rx), json!({"type": "status", "ciel": true}));
        TARGETS.lock().unwrap().push_back(("chrome-x".into(), Target::Bridge { bridge: "test-bridge".into(), key: "7:t".into() }));
        act("chrome-x", "open", |_| panic!("not a settings page"));
        assert_eq!(next(&mut rx), json!({"type": "click", "key": "7:t"}));
        act("chrome-x", "dismiss", |_| {});
        assert_eq!(next(&mut rx), json!({"type": "close", "key": "7:t"}));
        forget("test-bridge");
        act("chrome-x", "open", |_| {});
        assert!(rx.try_recv().is_err(), "a bridge gone hears nothing");
    }

    #[test]
    fn a_new_ciel_opens_settings_and_is_only_dismissed_otherwise() {
        TARGETS.lock().unwrap().push_back(("ciel-x".into(), Target::Settings("general".into())));
        let mut opened = None;
        act("ciel-x", "open", |page| opened = Some(page.to_string()));
        assert_eq!(opened.as_deref(), Some("general"));
        act("ciel-x", "dismiss", |_| panic!("a dismissal opens nothing"));
    }
}
