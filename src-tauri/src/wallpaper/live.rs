// The live wallpaper: one webview window per monitor (`wallpaper-<n>`, the
// page wallpaper.html, numbered like wallpaper_monitors), behind the desktop
// icons, and a poll that tells each window when nobody can see it so the page
// stops drawing: a fullscreen app or a presentation on its monitor, the
// session locked, the display off, or the computer on battery.
//
// Nothing here runs unless live mode is on: the poll is one thread that wakes
// about once a second and ends with live mode. When Explorer restarts (or a
// monitor comes or goes) the windows are put back where they belong.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use super::{os, MonitorInfo, PAUSE_EVENT};
use crate::i18n::t;

pub const LABEL_PREFIX: &str = "wallpaper-";

/// How often the poll looks.
const TICK: Duration = Duration::from_millis(1000);

/// A monitor's rectangle, physical screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn of(m: &MonitorInfo) -> Rect {
        Rect { x: m.x, y: m.y, w: m.width as i32, h: m.height as i32 }
    }

    /// Whether this rectangle covers all of `other`.
    pub fn covers(&self, other: &Rect) -> bool {
        self.x <= other.x && self.y <= other.y && self.x + self.w >= other.x + other.w && self.y + self.h >= other.y + other.h
    }
}

/// What the system says right now, for every live window at once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signals {
    /// The session is locked (or a screen saver or another user's session is up).
    pub locked: bool,
    pub display_off: bool,
    pub on_battery: bool,
    /// Presentation mode: every monitor counts as taken.
    pub presentation: bool,
    /// Rectangles of fullscreen apps: a monitor they cover is hidden.
    pub fullscreen: Vec<Rect>,
    /// Monitors a maximized window fills: the wallpaper there can't be seen,
    /// so it stops drawing (its last frame stays) until the desktop shows again.
    pub covered: Vec<Rect>,
}

/// Why the window on `monitor` should not draw, if it shouldn't.
pub fn pause_reason(s: &Signals, monitor: Rect, pause_on_battery: bool) -> Option<&'static str> {
    if s.locked {
        Some("locked")
    } else if s.display_off {
        Some("displayOff")
    } else if s.presentation || s.fullscreen.iter().any(|r| r.covers(&monitor)) {
        Some("fullscreen")
    } else if s.covered.iter().any(|r| r.covers(&monitor)) {
        Some("covered")
    } else if s.on_battery && pause_on_battery {
        Some("battery")
    } else {
        None
    }
}

#[derive(Clone, Serialize)]
struct Pause {
    paused: bool,
    reason: Option<&'static str>,
}

struct LiveWindow {
    label: String,
    rect: Rect,
    reason: Option<&'static str>,
}

static LIVE: Mutex<Option<Vec<LiveWindow>>> = Mutex::new(None);
/// Bumped whenever live mode starts or stops: an older poll ends.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static POLL: Mutex<Option<std::thread::Thread>> = Mutex::new(None);
static PAUSE_ON_BATTERY: AtomicBool = AtomicBool::new(true);
/// Settings → Wallpapers → lock screen is on: the session is followed.
static FOLLOW_LOCK: AtomicBool = AtomicBool::new(false);

fn live() -> std::sync::MutexGuard<'static, Option<Vec<LiveWindow>>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_live() -> bool {
    live().is_some()
}

pub fn all_paused() -> bool {
    live().as_ref().is_some_and(|w| !w.is_empty() && w.iter().all(|w| w.reason.is_some()))
}

pub fn set_pause_on_battery(on: bool) {
    PAUSE_ON_BATTERY.store(on, Ordering::Relaxed);
    poke();
}

/// The session is followed while the lock screen is in use or live mode is on.
pub fn follow_session(app: &AppHandle, lock_screen: bool) {
    FOLLOW_LOCK.store(lock_screen, Ordering::Relaxed);
    refresh_session(app);
}

fn refresh_session(app: &AppHandle) {
    os::follow_session(app, FOLLOW_LOCK.load(Ordering::Relaxed) || is_live());
}

/// Look again now rather than at the next tick (the session or power changed).
pub fn poke() {
    if let Some(t) = POLL.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        t.unpark();
    }
}

// ── Windows ───────────────────────────────────────────────────────────────────

fn page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/wallpaper.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("wallpaper.html".into())
}

/// The page asks nothing at load: it is told whether to draw once it loaded.
fn current_reason(label: &str) -> Option<&'static str> {
    live().as_ref().and_then(|w| w.iter().find(|w| w.label == label).and_then(|w| w.reason))
}

fn build(app: &AppHandle, label: &str, rect: Rect) -> Result<WebviewWindow, String> {
    WebviewWindowBuilder::new(app, label, page_url(app))
        // Same browser arguments as every other window (see BROWSER_ARGS).
        .additional_browser_args(crate::BROWSER_ARGS)
        .title("Ciel")
        .decorations(false)
        .resizable(false)
        .shadow(false)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .disable_drag_drop_handler()
        .background_color(tauri::window::Color(8, 12, 28, 255))
        .inner_size(rect.w as f64, rect.h as f64)
        .visible(false)
        .on_page_load(|win, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let reason = current_reason(win.label());
                let _ = win.emit_to(win.label(), PAUSE_EVENT, Pause { paused: reason.is_some(), reason });
            }
        })
        .build()
        .map_err(|e| e.to_string())
}

/// Runs `f` on the main thread (window placement must) and waits for it.
pub(crate) fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

fn make(app: &AppHandle, label: &str, rect: Rect) -> Result<(), String> {
    let win = build(app, label, rect)?;
    let placed = {
        let win = win.clone();
        on_main(app, move || os::place_live(&win, rect)).unwrap_or_else(|| Err("timed out".into()))
    };
    if let Err(err) = placed {
        crate::log::line(format!("wallpaper: {label} could not go behind the icons: {err}"));
        let _ = win.destroy();
        return Err(t("Ciel couldn't put the live wallpaper behind the desktop icons."));
    }
    crate::log::line(format!("wallpaper: {label} live, {}", os::describe_live(&win)));
    Ok(())
}

/// Off the main thread: takes the window out of the desktop layer, destroys
/// it, and waits until its label is free again (destroying is asynchronous).
fn remove(app: &AppHandle, label: &str) {
    let Some(win) = app.get_webview_window(label) else { return };
    let w = win.clone();
    let _ = on_main(app, move || os::release_live(&w));
    let _ = win.destroy();
    for _ in 0..60 {
        if app.get_webview_window(label).is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Turns live mode on, or lays it out again for the monitors connected now.
/// Blocking: called off the main thread.
fn start_blocking(app: &AppHandle, monitors: Vec<MonitorInfo>) -> Result<(), String> {
    // Already live on these very monitors: only make sure every window is there.
    let same = live().as_ref().is_some_and(|w| {
        w.len() == monitors.len() && w.iter().zip(&monitors).all(|(w, m)| w.rect == Rect::of(m))
    });
    if same {
        heal(app);
        return Ok(());
    }
    let old: Vec<String> = live().take().map(|w| w.into_iter().map(|w| w.label).collect()).unwrap_or_default();
    for label in &old {
        remove(app, label);
    }
    let mut windows: Vec<LiveWindow> = Vec::new();
    for (n, monitor) in monitors.iter().enumerate() {
        let label = format!("{LABEL_PREFIX}{n}");
        let rect = Rect::of(monitor);
        if let Err(err) = make(app, &label, rect) {
            for made in &windows {
                remove(app, &made.label);
            }
            os::after_live_removed();
            return Err(err);
        }
        windows.push(LiveWindow { label, rect, reason: None });
    }
    *live() = Some(windows);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    refresh_session(app);
    spawn_poll(app.clone(), generation);
    Ok(())
}

pub async fn start(app: &AppHandle) -> Result<(), String> {
    if !os::live_supported(app).await {
        return Err(t("Live wallpapers aren't available on this desktop."));
    }
    let monitors = super::monitors(app).await?;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || start_blocking(&app, monitors)).await.map_err(|e| e.to_string())?
}

fn stop_blocking(app: &AppHandle) {
    let Some(windows) = live().take() else { return };
    GENERATION.fetch_add(1, Ordering::SeqCst);
    poke();
    for w in &windows {
        remove(app, &w.label);
    }
    // Let the desktop take the space back, then repaint the picture under it.
    std::thread::sleep(Duration::from_millis(300));
    os::after_live_removed();
    refresh_session(app);
    crate::log::line("wallpaper: live off");
}

pub async fn stop(app: &AppHandle) {
    let app = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || stop_blocking(&app)).await;
}

/// At exit, on the main thread.
pub fn shutdown(app: &AppHandle) {
    let Some(windows) = live().take() else { return };
    GENERATION.fetch_add(1, Ordering::SeqCst);
    for w in &windows {
        if let Some(win) = app.get_webview_window(&w.label) {
            os::release_live(&win);
            let _ = win.destroy();
        }
    }
    os::after_live_removed();
}

/// Explorer restarted: windows it took down with it are made again, the
/// others are put back behind the icons.
pub fn heal(app: &AppHandle) {
    let windows: Vec<(String, Rect)> = match live().as_ref() {
        Some(w) => w.iter().map(|w| (w.label.clone(), w.rect)).collect(),
        None => return,
    };
    for (label, rect) in windows {
        match app.get_webview_window(&label) {
            None => {
                let _ = make(app, &label, rect);
            }
            Some(win) if !os::live_attached(&win) => {
                let w = win.clone();
                match on_main(app, move || os::place_live(&w, rect)) {
                    Some(Ok(())) => crate::log::line(format!("wallpaper: {label} back behind the icons")),
                    other => crate::log::line(format!("wallpaper: {label} not back yet: {other:?}")),
                }
            }
            Some(_) => {}
        }
    }
}

/// Monitors came, went or moved: lay the windows out again, once things settle.
pub fn relayout_soon(app: &AppHandle) {
    if !is_live() {
        return;
    }
    let generation = GENERATION.load(Ordering::SeqCst);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if GENERATION.load(Ordering::SeqCst) != generation || !is_live() {
            return;
        }
        let Ok(monitors) = super::monitors(&app).await else { return };
        let a = app.clone();
        if let Ok(Err(err)) = tauri::async_runtime::spawn_blocking(move || start_blocking(&a, monitors)).await {
            crate::log::line(format!("wallpaper: live windows not laid out again: {err}"));
        }
    });
}

/// Explorer came back: put the windows back once it has made its desktop.
pub fn heal_soon(app: &AppHandle) {
    if !is_live() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1000));
        heal(&app);
    });
}

// ── The poll ──────────────────────────────────────────────────────────────────

fn spawn_poll(app: AppHandle, generation: u64) {
    let spawned = std::thread::Builder::new().name("ciel-wallpaper-poll".into()).spawn(move || {
        *POLL.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::thread::current());
        while GENERATION.load(Ordering::SeqCst) == generation {
            tick(&app);
            std::thread::park_timeout(TICK);
        }
    });
    if let Err(err) = spawned {
        crate::log::line(format!("wallpaper: no pause poll: {err}"));
    }
}

fn tick(app: &AppHandle) {
    let rects: Vec<Rect> = match live().as_ref() {
        Some(w) => w.iter().map(|w| w.rect).collect(),
        None => return,
    };
    let signals = os::signals(&rects);
    let on_battery = PAUSE_ON_BATTERY.load(Ordering::Relaxed);
    let mut changed = Vec::new();
    if let Some(windows) = live().as_mut() {
        for w in windows.iter_mut() {
            let reason = pause_reason(&signals, w.rect, on_battery);
            if reason != w.reason {
                w.reason = reason;
                changed.push((w.label.clone(), reason));
            }
        }
    }
    for (label, reason) in changed {
        let _ = app.emit_to(label.as_str(), PAUSE_EVENT, Pause { paused: reason.is_some(), reason });
    }
    // Explorer restarting takes the desktop layer down: checked every tick,
    // cheaply, besides the TaskbarCreated broadcast.
    let gone = live().as_ref().is_some_and(|w| {
        w.iter().any(|w| app.get_webview_window(&w.label).is_none_or(|win| !os::live_attached(&win)))
    });
    if gone && os::desktop_ready() {
        heal(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIN: Rect = Rect { x: 0, y: 0, w: 3440, h: 1440 };
    const LEFT: Rect = Rect { x: -1920, y: 865, w: 1920, h: 1200 };

    #[test]
    fn a_fullscreen_app_pauses_only_the_monitor_it_covers() {
        let game = Signals { fullscreen: vec![MAIN], ..Default::default() };
        assert_eq!(pause_reason(&game, MAIN, true), Some("fullscreen"));
        assert_eq!(pause_reason(&game, LEFT, true), None);
        // A borderless window a little larger than the monitor still covers it.
        let video = Signals { fullscreen: vec![Rect { x: -1928, y: 857, w: 1936, h: 1216 }], ..Default::default() };
        assert_eq!(pause_reason(&video, LEFT, true), Some("fullscreen"));
        assert_eq!(pause_reason(&video, MAIN, true), None);
        // A window that only covers part of it doesn't.
        let half = Signals { fullscreen: vec![Rect { x: 0, y: 0, w: 1720, h: 1440 }], ..Default::default() };
        assert_eq!(pause_reason(&half, MAIN, true), None);
    }

    #[test]
    fn a_maximized_window_pauses_the_monitor_it_fills_and_only_that_one() {
        let work = Signals { covered: vec![MAIN], ..Default::default() };
        assert_eq!(pause_reason(&work, MAIN, true), Some("covered"));
        assert_eq!(pause_reason(&work, LEFT, true), None);
        // A fullscreen app says so first; the battery comes after.
        let both = Signals { covered: vec![MAIN], fullscreen: vec![MAIN], on_battery: true, ..Default::default() };
        assert_eq!(pause_reason(&both, MAIN, true), Some("fullscreen"));
        let left = Signals { covered: vec![LEFT], on_battery: true, ..Default::default() };
        assert_eq!(pause_reason(&left, MAIN, true), Some("battery"));
        assert_eq!(pause_reason(&left, MAIN, false), None);
    }

    #[test]
    fn presentations_locks_and_a_dark_display_pause_every_monitor() {
        for (signals, reason) in [
            (Signals { presentation: true, ..Default::default() }, "fullscreen"),
            (Signals { locked: true, on_battery: true, ..Default::default() }, "locked"),
            (Signals { display_off: true, ..Default::default() }, "displayOff"),
        ] {
            assert_eq!(pause_reason(&signals, MAIN, true), Some(reason));
            assert_eq!(pause_reason(&signals, LEFT, false), Some(reason));
        }
    }

    #[test]
    fn the_battery_pauses_only_when_the_user_wants_it_to() {
        let battery = Signals { on_battery: true, ..Default::default() };
        assert_eq!(pause_reason(&battery, MAIN, true), Some("battery"));
        assert_eq!(pause_reason(&battery, MAIN, false), None);
        assert_eq!(pause_reason(&Signals::default(), MAIN, true), None);
    }
}
