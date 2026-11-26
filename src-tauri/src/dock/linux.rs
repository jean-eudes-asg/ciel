// The dock on GNOME: a window at the bottom of every screen, the room under
// each kept for it, and what the Shell says through Ciel's extension
// (gnome-extension/extension.js), on X11 and Wayland alike: the apps open and
// their windows, the one in front, a full-screen app, the speakers changing.
// Ciel's windows are X11 ones on GNOME (XWayland under Wayland, see
// platform::prepare_environment), so the dock is a Dock window with a strut.
//
// Its pins are GNOME's favourites, the ones the app grid pins too. It can turn
// GNOME's own dock off while it shows (the Ubuntu Dock, Dash to Dock), and
// turns it back on when it goes or Ciel quits, or at the next launch should
// Ciel have stopped without doing it (system-dock.json).
//
// The corner and the clock open Ciel's own panel over the dock clicked (the
// page dock-panel.html): Tools → Controls, and the month with the
// notifications. It takes the keyboard, and goes when another window does.
//
// While it is on, two threads wait on D-Bus: the session bus for the Shell's
// DockChanged, the system bus for NetworkManager and UPower. Off, nothing
// runs and its window is gone.

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use super::{DockState, Entry, Open, Pin, SystemIcons, BAR_H, GAP, HEAD, LABEL, SIDE, STATE_EVENT, SYSTEM_EVENT};
use tauri::{Monitor, WebviewWindow};
use crate::gnome_ext;
use crate::platform;

static APP: OnceLock<AppHandle> = OnceLock::new();
static ON: AtomicBool = AtomicBool::new(false);
static SYNCING: Mutex<()> = Mutex::new(());
/// What Settings said last, for the Shell's extension to be answered when it
/// starts again (it says "ready").
static PREFS: Mutex<Option<crate::settings::DockPrefs>> = Mutex::new(None);
/// Ciel quits: nothing turns GNOME's dock off again.
static QUITTING: AtomicBool = AtomicBool::new(false);
static HIDE_SYSTEM_DOCK: AtomicBool = AtomicBool::new(false);
/// Bumped each time the watch starts or stops: an older watch's threads end.
static WATCH: AtomicU64 = AtomicU64::new(0);
static WATCHING: AtomicBool = AtomicBool::new(false);
/// The bar's width at rest, CSS pixels (from the page).
static WIDTH: AtomicU64 = AtomicU64::new(0);
static SHOWN: AtomicBool = AtomicBool::new(false);
/// The screens a full-screen app is in front on, by their corner.
static FULLSCREEN: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());
/// The Shell's window in front.
static FOCUS: AtomicU64 = AtomicU64::new(0);
static APPS_SOON: AtomicBool = AtomicBool::new(false);
static SYSTEM_SOON: AtomicBool = AtomicBool::new(false);
static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static PUBLISHED: Mutex<Option<DockState>> = Mutex::new(None);
static ORDER: Mutex<Vec<String>> = Mutex::new(Vec::new());
static FAVORITES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static SYSTEM: Mutex<Option<SystemIcons>> = Mutex::new(None);
/// The .desktop apps seen so far, with their icons; None: no .desktop file.
static KNOWN: Mutex<Option<HashMap<String, Option<crate::quick::App>>>> = Mutex::new(None);

/// The panel over the dock: its window, what it shows ("controls" or
/// "clock", "" while closed) and where the pointer was (physical pixels).
const PANEL: &str = "dock-panel";
pub const PANEL_EVENT: &str = "dock-panel";
/// To the page as its window goes: it fades for FADE_MS before it is hidden.
const PANEL_OUT: &str = "dock-panel-out";
const FADE_MS: u64 = 150;
static PANEL_KIND: Mutex<&'static str> = Mutex::new("");
static PANEL_AT: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// When the panel last went because another window took the keyboard, and
/// which it was: the click on its own button that did it doesn't bring it back.
static PANEL_GONE: Mutex<Option<(std::time::Instant, &'static str)>> = Mutex::new(None);

/// The window that opens when the pointer rests on Ciel's tile: the same
/// page, showing the agents, their requests and the plan's usage. It never
/// takes the keyboard, and goes once the pointer has left both the tile and it.
const HOVER: &str = "dock-hover";
static HOVER_AT: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
/// Whether the pointer is on the tile, and on the window: while either is, it stays.
static HOVER_IN: Mutex<(bool, bool)> = Mutex::new((false, false));
/// Asked to show and not taken away since: the page's size may show it.
static HOVER_ON: Mutex<bool> = Mutex::new(false);
/// Long enough for the pointer to cross from the tile to the window above it.
const HOVER_GRACE_MS: u64 = 500;

/// Windows open and close in bursts: the row is read once they settle.
const APPS_MS: u64 = 120;
/// The volume changes many times a second while a slider moves.
const SYSTEM_MS: u64 = 60;

/// GNOME's own docks, which the dock can stand in for. Dash to Panel is the
/// whole top bar as well: never turned off.
const SYSTEM_DOCKS: [&str; 2] = ["ubuntu-dock@ubuntu.com", "dash-to-dock@micxgx.gmail.com"];
const SYSTEM_DOCK_FILE: &str = "system-dock.json";

fn page_url(app: &AppHandle, page: &str) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path(&format!("/{page}"));
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App(page.into())
}

pub fn setup(app: &AppHandle) {
    let _ = APP.set(app.clone());
    // Ciel stopped last time with GNOME's dock off: it comes back first.
    restore_system_dock();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_system_dock();
        previous(info);
    }));
}

/// Shows or takes away the dock, off the caller's thread (the main one at
/// launch and on a save): the Shell and gnome-extensions are asked first.
pub fn sync(app: &AppHandle, prefs: &crate::settings::DockPrefs) {
    let _ = APP.set(app.clone());
    *PREFS.lock().unwrap() = Some(prefs.clone());
    resync();
}

/// As Settings said last, on a thread of its own, one at a time.
fn resync() {
    std::thread::spawn(|| {
        let _one_at_a_time = SYNCING.lock().unwrap_or_else(|e| e.into_inner());
        let (Some(app), Some(prefs)) = (APP.get(), PREFS.lock().unwrap().clone()) else { return };
        apply(app, &prefs);
    });
}

fn apply(app: &AppHandle, prefs: &crate::settings::DockPrefs) {
    if QUITTING.load(Ordering::SeqCst) {
        return;
    }
    HIDE_SYSTEM_DOCK.store(prefs.hide_taskbar, Ordering::SeqCst);
    let ready = prefs.on && gnome_ext::version().is_some_and(|v| v >= gnome_ext::DOCK_VERSION);
    if prefs.on && !ready {
        crate::log::line("dock: waits for Ciel's GNOME extension, version 5 or later");
    }
    let on = prefs.on && ready;
    ON.store(on, Ordering::SeqCst);
    let handle = app.clone();
    if !on {
        let _ = app.run_on_main_thread(move || {
            for win in docks(&handle) {
                let _ = win.destroy();
            }
            close_panel(&handle, true);
        });
    }
    if prefs.on {
        // On, the Shell is heard even before the extension can answer: it
        // says "ready" once it runs, and the dock comes then.
        start_watch();
    } else {
        stop_watch();
    }
    if on {
        apply_system_dock();
        gnome_ext::dock_watch(true);
        read_system();
        refresh();
        place();
    } else {
        restore_system_dock();
        SHOWN.store(false, Ordering::SeqCst);
        *PUBLISHED.lock().unwrap() = None;
    }
}

pub fn shutdown() {
    QUITTING.store(true, Ordering::SeqCst);
    let _one_at_a_time = SYNCING.lock().unwrap_or_else(|e| e.into_inner());
    ON.store(false, Ordering::SeqCst);
    stop_watch();
    restore_system_dock();
}

/// The dock's windows, one a screen: "dock", "dock-1", "dock-2"…
fn docks(app: &AppHandle) -> Vec<WebviewWindow> {
    app.webview_windows().into_iter().filter(|(label, _)| is_dock(label)).map(|(_, w)| w).collect()
}

fn is_dock(label: &str) -> bool {
    label == LABEL || label.strip_prefix(LABEL).and_then(|n| n.strip_prefix('-')).is_some_and(|n| n.parse::<u32>().is_ok())
}

fn dock_label(screen: usize) -> String {
    if screen == 0 { LABEL.to_string() } else { format!("{LABEL}-{screen}") }
}

/// Every dock window hears it.
fn emit_docks<S: serde::Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    for win in docks(app) {
        let _ = app.emit_to(win.label(), event, payload.clone());
    }
}

fn create(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    let built = WebviewWindowBuilder::new(app, label, page_url(app, "dock.html"))
        .title("Ciel")
        .inner_size(600.0 + 2.0 * SIDE, BAR_H + HEAD)
        // GTK won't size a non-resizable window below its natural size, 200 px
        // (see island::apply_geometry): the bar would hang off the screen.
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .disable_drag_drop_handler()
        .visible(false)
        .build();
    match built {
        Ok(win) => {
            platform::prepare_dock_window(&win);
            Some(win)
        }
        Err(err) => {
            crate::log::line(format!("dock window failed: {err}"));
            None
        }
    }
}

pub fn state() -> DockState {
    PUBLISHED.lock().unwrap().clone().unwrap_or_default()
}

pub fn fit(width: f64) {
    if !width.is_finite() || width <= 0.0 {
        return;
    }
    let before = f64::from_bits(WIDTH.swap(width.to_bits(), Ordering::SeqCst));
    if (before - width).abs() >= 0.5 {
        place();
    }
}

// ── The watch ─────────────────────────────────────────────────────────────────

fn start_watch() {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let generation = WATCH.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = std::thread::Builder::new().name("ciel-dock-shell".into()).spawn(move || listen_shell(generation));
    let _ = std::thread::Builder::new().name("ciel-dock-system".into()).spawn(move || listen_system(generation));
}

fn stop_watch() {
    if !WATCHING.swap(false, Ordering::SeqCst) {
        return;
    }
    WATCH.fetch_add(1, Ordering::SeqCst);
    gnome_ext::dock_watch(false);
}

fn watching(generation: u64) -> bool {
    WATCH.load(Ordering::SeqCst) == generation
}

/// The Shell's DockChanged: "apps" (and "fullscreen"), "system", or "ready"
/// when Ciel's extension starts (again): asked to watch once more.
fn listen_shell(generation: u64) {
    use dbus::blocking::Connection;
    use dbus::message::MatchRule;
    let Ok(conn) = Connection::new_session() else { return };
    let (path, iface) = gnome_ext::SHELL;
    let rule = MatchRule::new_signal(iface, "DockChanged").with_path(path);
    let heard = conn.add_match(rule, |(what,): (String,), _, _| {
        match what.as_str() {
            "system" => soon(&SYSTEM_SOON, SYSTEM_MS, read_system),
            "ready" => resync(),
            "monitors" => place(),
            _ => soon(&APPS_SOON, APPS_MS, refresh),
        }
        true
    });
    if heard.is_err() {
        crate::log::line("dock: the Shell can't be heard, the row is read when the dock is clicked");
    }
    while watching(generation) {
        let _ = conn.process(Duration::from_secs(1));
    }
}

/// The network and the battery, as NetworkManager and UPower tell of them.
fn listen_system(generation: u64) {
    use dbus::blocking::Connection;
    use dbus::message::MatchRule;
    let Ok(conn) = Connection::new_system() else { return };
    let changed = "org.freedesktop.DBus.Properties";
    for (sender, path) in [
        ("org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager"),
        ("org.freedesktop.UPower", "/org/freedesktop/UPower/devices/DisplayDevice"),
    ] {
        let rule = MatchRule::new_signal(changed, "PropertiesChanged").with_sender(sender).with_path(path);
        let _ = conn.add_match(rule, |_: (), _, _| {
            soon(&SYSTEM_SOON, SYSTEM_MS, read_system);
            true
        });
    }
    while watching(generation) {
        let _ = conn.process(Duration::from_secs(1));
    }
}

/// `then` once, a moment after the first of a burst of changes.
fn soon(flag: &'static AtomicBool, ms: u64, then: fn()) {
    if flag.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        flag.store(false, Ordering::SeqCst);
        then();
    });
}

/// The speakers, the network and the battery, with Tools → Controls' readers.
fn read_system() {
    if !ON.load(Ordering::SeqCst) {
        return;
    }
    let (volume, muted) = crate::controls::volume().map_or((None, false), |(v, m)| (Some(v), m));
    let network = crate::controls::network();
    let battery = crate::controls::battery();
    let icons = SystemIcons {
        volume,
        muted,
        network: network.as_ref().map(|n| n.kind.clone()).unwrap_or_default(),
        network_name: network.map(|n| n.name).unwrap_or_default(),
        battery: battery.as_ref().map(|b| b.percent),
        charging: battery.is_some_and(|b| b.charging),
    };
    let changed = SYSTEM.lock().unwrap().replace(icons.clone()).as_ref() != Some(&icons);
    if !changed {
        return;
    }
    if let Some(state) = PUBLISHED.lock().unwrap().as_mut() {
        state.system = icons.clone();
    }
    if let Some(app) = APP.get() {
        emit_docks(app, SYSTEM_EVENT, icons);
    }
}

// ── Where it sits ─────────────────────────────────────────────────────────────

/// At the bottom of every screen, centred, with the room under it kept;
/// hidden on a screen while a full-screen app is in front there. A window
/// comes for a screen plugged in, and goes with it. Main thread.
fn place() {
    let Some(app) = APP.get() else { return };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || place_now(&handle));
}

fn place_now(app: &AppHandle) {
    let on = ON.load(Ordering::SeqCst);
    let screens = if on { app.available_monitors().unwrap_or_default() } else { Vec::new() };
    let fullscreen = FULLSCREEN.lock().unwrap().clone();
    let mut shown = false;
    for (i, screen) in screens.iter().enumerate() {
        let label = dock_label(i);
        let Some(win) = app.get_webview_window(&label).or_else(|| create(app, &label)) else { continue };
        let corner = (screen.position().x, screen.position().y);
        if fullscreen.contains(&corner) {
            let _ = win.hide();
            close_panel(app, false);
        } else {
            place_on(&win, screen);
            shown = true;
        }
    }
    for win in docks(app) {
        if !(0..screens.len()).any(|i| dock_label(i) == win.label()) {
            let _ = win.destroy();
        }
    }
    show(shown);
}

fn place_on(win: &WebviewWindow, screen: &Monitor) {
    let scale = screen.scale_factor();
    let (sx, sy) = (screen.position().x, screen.position().y);
    let (sw, sh) = (screen.size().width as i32, screen.size().height as i32);
    let px = |v: f64| (v * scale).round() as i32;
    let width = f64::from_bits(WIDTH.load(Ordering::SeqCst));
    let bar_w = px(if width > 0.0 { width } else { 600.0 }).clamp(1, sw - 2 * px(GAP));
    let bar_h = px(BAR_H);
    let (side, head) = (px(SIDE), px(HEAD));
    let x = sx + (sw - bar_w) / 2;
    let y = sy + sh - px(GAP) - bar_h;
    let _ = win.set_size(PhysicalSize::new((bar_w + 2 * side) as u32, (bar_h + head) as u32));
    let _ = win.set_position(PhysicalPosition::new(x - side, y - head));
    if !win.is_visible().unwrap_or(false) {
        let _ = win.show();
    }
    platform::set_dock_strut(win, sx, sw, sy + sh, px(BAR_H + 2.0 * GAP));
}

fn show(visible: bool) {
    if SHOWN.swap(visible, Ordering::SeqCst) != visible {
        publish(None);
    }
}

/// What takes clicks in a dock window: all of it while the pointer is on
/// the bar (the icons grown into the room above), else the bar alone.
pub fn region(win: &WebviewWindow, hover: bool) {
    let (Ok(size), Ok(scale)) = (win.inner_size(), win.scale_factor()) else { return };
    let (w, h) = (size.width as f64 / scale, size.height as f64 / scale);
    let rects = if hover { vec![(0.0, 0.0, w, h)] } else { vec![(SIDE, HEAD, (w - 2.0 * SIDE).max(0.0), (h - HEAD).max(0.0))] };
    platform::set_dock_input(win, rects);
}

// ── The row ───────────────────────────────────────────────────────────────────

/// What DockApps answers (extension.js Dock.apps).
#[derive(Deserialize, Default)]
struct ShellApps {
    apps: Vec<ShellApp>,
    favorites: Vec<String>,
    focus: u64,
    fullscreen: Vec<(i32, i32)>,
}

#[derive(Deserialize)]
struct ShellApp {
    id: String,
    name: String,
    /// Most recently used first.
    windows: Vec<u64>,
    attention: bool,
}

/// The .desktop apps of these ids, read once each.
fn known(ids: &[&String]) -> HashMap<String, Option<crate::quick::App>> {
    let mut cache = KNOWN.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    let missing: Vec<String> = ids.iter().filter(|id| !cache.contains_key(**id)).map(|id| (*id).clone()).collect();
    if !missing.is_empty() {
        let found = crate::quick::desktop_apps(&missing);
        for id in missing {
            let app = found.iter().find(|a| a.id == id).cloned();
            cache.insert(id, app);
        }
    }
    ids.iter().map(|id| ((*id).clone(), cache.get(*id).cloned().flatten())).collect()
}

/// Asks the Shell for the row again, and tells the page if it changed.
fn refresh() {
    if !ON.load(Ordering::SeqCst) {
        return;
    }
    let Some(json) = gnome_ext::dock_apps() else { return };
    let shell = match serde_json::from_str::<ShellApps>(&json) {
        Ok(shell) => shell,
        Err(err) => {
            // An older extension still running: GNOME Shell reads the new one when it starts.
            crate::log::line(format!("dock: the Shell's apps can't be read ({err}); restart GNOME Shell"));
            return;
        }
    };
    let apps = known(&shell.favorites.iter().chain(shell.apps.iter().map(|a| &a.id)).collect::<Vec<_>>());
    let entries = row(&shell, &apps, &mut ORDER.lock().unwrap());
    FOCUS.store(shell.focus, Ordering::SeqCst);
    *FAVORITES.lock().unwrap() = shell.favorites;
    publish(Some(entries));
    let moved = {
        let mut kept = FULLSCREEN.lock().unwrap();
        std::mem::replace(&mut *kept, shell.fullscreen) != *kept
    };
    if moved {
        place();
    }
}

/// The favourites with a .desktop file, then the apps open, as on Windows (merge).
fn row(shell: &ShellApps, apps: &HashMap<String, Option<crate::quick::App>>, order: &mut Vec<String>) -> Vec<Entry> {
    let pins: Vec<Pin> = shell
        .favorites
        .iter()
        .filter_map(|id| apps.get(id).cloned().flatten())
        .map(|a| Pin { id: a.id.clone(), name: a.name, icon: a.icon, target: Some(a.id) })
        .collect();
    super::merge(&pins, &open_apps(shell, apps), order)
}

fn open_apps(shell: &ShellApps, apps: &HashMap<String, Option<crate::quick::App>>) -> Vec<Open> {
    shell
        .apps
        .iter()
        .map(|a| {
            let windows: Vec<isize> = a.windows.iter().map(|w| *w as isize).collect();
            Open {
                key: a.id.clone(),
                path: a.id.clone(),
                name: a.name.clone(),
                icon: apps.get(&a.id).cloned().flatten().and_then(|d| d.icon),
                active: a.windows.contains(&shell.focus),
                windows,
                attention: a.attention,
            }
        })
        .collect()
}

fn publish(entries: Option<Vec<Entry>>) {
    let apps = {
        let mut kept = ENTRIES.lock().unwrap();
        if let Some(entries) = entries {
            *kept = entries;
        }
        kept.iter().map(|e| e.app.clone()).collect()
    };
    let Some(app) = APP.get() else { return };
    let state = DockState {
        apps,
        // Activities shows the top bar Ciel hides: no arrow for it.
        taskbar_hidden: false,
        shown: SHOWN.load(Ordering::SeqCst),
        system: SYSTEM.lock().unwrap().clone().unwrap_or_default(),
    };
    let changed = {
        let mut last = PUBLISHED.lock().unwrap();
        let changed = last.as_ref() != Some(&state);
        *last = Some(state.clone());
        changed
    };
    if changed {
        emit_docks(app, STATE_EVENT, state);
    }
}

// ── What the page asks ───────────────────────────────────────────────────────

fn entry(id: &str) -> Option<Entry> {
    ENTRIES.lock().unwrap().iter().find(|e| e.app.id == id).cloned()
}

pub fn open(id: &str, new_window: bool) -> bool {
    let Some(entry) = entry(id) else { return false };
    if new_window {
        return gnome_ext::dock_launch(&entry.launch, true);
    }
    match super::click(&entry.windows, FOCUS.load(Ordering::SeqCst) as isize) {
        super::Click::Launch => gnome_ext::dock_launch(&entry.launch, false),
        super::Click::Front(w) => gnome_ext::dock_window(w as u64, "front"),
        super::Click::Minimize(w) => gnome_ext::dock_window(w as u64, "minimize"),
    }
}

/// The app grid, as Show Apps on GNOME's dock.
/// The Shell's answer isn't waited for on the main thread (these are the
/// page's sync commands); the page doesn't look at it.
fn later(then: impl FnOnce() + Send + 'static) -> bool {
    std::thread::spawn(then);
    true
}

pub fn start_menu() -> bool {
    later(|| {
        gnome_ext::dock_shell("apps");
    })
}

/// What the Windows taskbar's buttons are on GNOME: the overview for task
/// view, Ciel's own panels for the corner and the clock. No widgets.
pub fn panel(what: &str) -> bool {
    let what: &'static str = match what {
        "taskView" => "overview",
        "desktop" => "desktop",
        "quickSettings" => return open_panel("controls"),
        "notifications" => return open_panel("clock"),
        _ => return false,
    };
    later(move || {
        gnome_ext::dock_shell(what);
    })
}

pub fn turn_volume(notches: f64) -> bool {
    if !notches.is_finite() {
        return false;
    }
    let Some((level, _)) = crate::controls::volume() else { return false };
    crate::controls::set_volume(super::volume_after(level, notches.clamp(-10.0, 10.0)))
}

/// An app's menu is GNOME's own (its actions, favourites, details); the bar's
/// is the dock's settings.
pub fn menu(id: Option<String>) {
    match id.and_then(|id| entry(&id)) {
        Some(e) => {
            later(move || {
                gnome_ext::dock_menu(&e.launch);
            });
        }
        None => {
            let Some(app) = APP.get() else { return };
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || crate::open_settings(&handle, "dock"));
        }
    }
}

/// An app dragged to `to` among the pins: GNOME's favourites change, and the
/// Shell tells the row. Only apps with a .desktop file can be favourites.
pub fn pin_moved(id: &str, to: usize) -> bool {
    let Some(e) = entry(id) else { return false };
    later(move || {
        if known(&[&e.launch]).get(&e.launch).cloned().flatten().is_some() {
            let pins = super::pin_at(&FAVORITES.lock().unwrap(), &e.launch, to);
            crate::quick::set_favorites(&pins);
        }
    })
}

/// No arrow on GNOME (publish): Activities shows the top bar.
pub fn reveal_taskbar() -> bool {
    false
}

// ── The panel over the dock ───────────────────────────────────────────────────

/// The corner or the clock: its panel over the dock clicked, or away if it
/// was showing.
fn open_panel(kind: &'static str) -> bool {
    let Some(app) = APP.get() else { return false };
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || toggle_panel(&handle, kind));
    true
}

fn toggle_panel(app: &AppHandle, kind: &'static str) {
    let just_gone = PANEL_GONE.lock().unwrap().take().is_some_and(|(at, k)| k == kind && at.elapsed() < Duration::from_millis(400));
    let showing = app.get_webview_window(PANEL).is_some_and(|w| w.is_visible().unwrap_or(false));
    if just_gone || (showing && *PANEL_KIND.lock().unwrap() == kind) {
        close_panel(app, false);
        return;
    }
    let Ok(at) = app.cursor_position() else { return };
    *PANEL_AT.lock().unwrap() = (at.x, at.y);
    *PANEL_KIND.lock().unwrap() = kind;
    if app.get_webview_window(PANEL).is_some() {
        let _ = app.emit_to(PANEL, PANEL_EVENT, kind);
    } else {
        // The page asks what to show once it has loaded (panel_kind).
        create_panel(app);
    }
}

fn create_panel(app: &AppHandle) {
    let built = WebviewWindowBuilder::new(app, PANEL, page_url(app, "dock-panel.html"))
        .title("Ciel")
        .inner_size(360.0, 400.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .maximizable(false)
        .minimizable(false)
        .disable_drag_drop_handler()
        .visible(false)
        .build();
    let win = match built {
        Ok(win) => win,
        Err(err) => {
            crate::log::line(format!("dock panel failed: {err}"));
            return;
        }
    };
    let handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            let kind = *PANEL_KIND.lock().unwrap();
            if !kind.is_empty() {
                *PANEL_GONE.lock().unwrap() = Some((std::time::Instant::now(), kind));
                close_panel(&handle, false);
            }
        }
    });
}

/// What the page shows; "" while closed. The hover window shows the agents.
pub fn panel_kind(label: &str) -> String {
    if label == HOVER {
        return if *HOVER_ON.lock().unwrap() { "agents".into() } else { String::new() };
    }
    PANEL_KIND.lock().unwrap().to_string()
}

/// The page drew it this size (CSS pixels): over the dock where the pointer
/// was, kept on that screen, then shown, the clicked panel with the
/// keyboard, the hover window without. Main thread.
pub fn panel_fit(app: &AppHandle, label: &str, width: f64, height: f64) {
    let hover = label == HOVER;
    let Some(win) = app.get_webview_window(if hover { HOVER } else { PANEL }) else { return };
    let open = if hover { *HOVER_ON.lock().unwrap() } else { !PANEL_KIND.lock().unwrap().is_empty() };
    if !open || !width.is_finite() || !height.is_finite() {
        return;
    }
    let (px, py) = if hover { *HOVER_AT.lock().unwrap() } else { *PANEL_AT.lock().unwrap() };
    let Ok(Some(screen)) = app.monitor_from_point(px, py) else { return };
    let scale = screen.scale_factor();
    let (sx, sy) = (screen.position().x, screen.position().y);
    let (sw, sh) = (screen.size().width as i32, screen.size().height as i32);
    let at = |v: f64| (v * scale).round() as i32;
    let (w, h) = (at(width).clamp(1, sw), at(height).clamp(1, sh));
    let x = (px.round() as i32 - w / 2).clamp(sx + at(GAP), (sx + sw - w - at(GAP)).max(sx));
    // The hover window just over the bar, for the pointer to reach it from Ciel's tile.
    let above = if hover { BAR_H + 2.0 * GAP } else { BAR_H + 3.0 * GAP };
    let y = (sy + sh - at(above) - h).max(sy);
    // Moved and resized at once: in two steps it would shrink from the top, then drop.
    platform::move_resize(&win, x, y, w, h);
    if !win.is_visible().unwrap_or(false) {
        let _ = win.show();
    }
    if !hover {
        let _ = win.set_focus();
    }
}

/// The pointer came onto or left `part`: "tile" (Ciel's, after resting
/// there) or "panel" (the hover window); "close": away at once (a click).
pub fn hover(part: &str, on: bool) {
    let Some(app) = APP.get() else { return };
    let handle = app.clone();
    let part = part.to_string();
    let _ = app.run_on_main_thread(move || hover_now(&handle, &part, on));
}

fn hover_now(app: &AppHandle, part: &str, on: bool) {
    {
        let mut inside = HOVER_IN.lock().unwrap();
        match part {
            "tile" => inside.0 = on,
            "panel" => inside.1 = on,
            "close" => *inside = (false, false),
            _ => return,
        }
    }
    if part == "close" {
        hide_hover(app);
    } else if !on {
        // Once the pointer has had its moment to cross from one to the other.
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(HOVER_GRACE_MS));
            let main = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                if *HOVER_IN.lock().unwrap() == (false, false) {
                    hide_hover(&main);
                }
            });
        });
    } else if part == "tile" && !*HOVER_ON.lock().unwrap() {
        // Never over a panel a click opened.
        if app.get_webview_window(PANEL).is_some_and(|w| w.is_visible().unwrap_or(false)) {
            return;
        }
        let Ok(at) = app.cursor_position() else { return };
        *HOVER_AT.lock().unwrap() = (at.x, at.y);
        *HOVER_ON.lock().unwrap() = true;
        if app.get_webview_window(HOVER).is_some() {
            let _ = app.emit_to(HOVER, PANEL_EVENT, "agents");
        } else {
            create_hover(app);
        }
    }
}

fn hide_hover(app: &AppHandle) {
    *HOVER_ON.lock().unwrap() = false;
    fade_out(app, HOVER, || !*HOVER_ON.lock().unwrap());
}

/// The page fades its card, then the window is hidden, unless it was asked
/// back meanwhile (`still_gone` says it wasn't). Main thread.
fn fade_out(app: &AppHandle, label: &'static str, still_gone: fn() -> bool) {
    if !app.get_webview_window(label).is_some_and(|w| w.is_visible().unwrap_or(false)) {
        return;
    }
    let _ = app.emit_to(label, PANEL_OUT, ());
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(FADE_MS));
        let main = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if still_gone() {
                if let Some(win) = main.get_webview_window(label) {
                    let _ = win.hide();
                }
            }
        });
    });
}

fn create_hover(app: &AppHandle) {
    let built = WebviewWindowBuilder::new(app, HOVER, page_url(app, "dock-panel.html"))
        .title("Ciel")
        .inner_size(360.0, 300.0)
        .resizable(true)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .maximizable(false)
        .minimizable(false)
        .disable_drag_drop_handler()
        .visible(false)
        .build();
    match built {
        Ok(win) => crate::platform::prepare_popup_window(&win),
        Err(err) => crate::log::line(format!("dock hover failed: {err}")),
    }
}

/// Away (Escape, a click elsewhere, its button again); `gone`: its window,
/// and the hover window's, too.
pub fn close_panel(app: &AppHandle, gone: bool) {
    if gone {
        *HOVER_ON.lock().unwrap() = false;
        *HOVER_IN.lock().unwrap() = (false, false);
        if let Some(win) = app.get_webview_window(HOVER) {
            let _ = win.destroy();
        }
    }
    *PANEL_KIND.lock().unwrap() = "";
    if gone {
        if let Some(win) = app.get_webview_window(PANEL) {
            let _ = win.destroy();
        }
    } else {
        fade_out(app, PANEL, || PANEL_KIND.lock().unwrap().is_empty());
    }
}

// ── GNOME's own dock ──────────────────────────────────────────────────────────

fn system_dock_file() -> std::path::PathBuf {
    crate::settings::config_dir().join(SYSTEM_DOCK_FILE)
}

fn gnome_extensions(args: &[&str]) -> Option<String> {
    // "State: ACTIVE" in every language.
    let out = Command::new("gnome-extensions")
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn apply_system_dock() {
    if HIDE_SYSTEM_DOCK.load(Ordering::SeqCst) {
        hide_system_dock();
    } else {
        restore_system_dock();
    }
}

/// GNOME's docks that run are turned off, as Extensions does it (they stay
/// installed); which ones is written down first, to turn them back on.
fn hide_system_dock() {
    if system_dock_file().exists() {
        return;
    }
    let running: Vec<String> = SYSTEM_DOCKS
        .iter()
        .filter(|uuid| gnome_extensions(&["info", uuid]).is_some_and(|info| info.contains("State: ACTIVE")))
        .map(|uuid| uuid.to_string())
        .collect();
    if running.is_empty() {
        return;
    }
    if let Err(err) = std::fs::write(system_dock_file(), serde_json::to_vec(&running).unwrap_or_default()) {
        crate::log::line(format!("dock: GNOME's dock left on, nowhere to note it: {err}"));
        return;
    }
    for uuid in &running {
        gnome_extensions(&["disable", uuid]);
    }
    crate::log::line(format!("dock: turned off {}", running.join(", ")));
}

fn restore_system_dock() {
    let Ok(bytes) = std::fs::read(system_dock_file()) else { return };
    let turned_off: Vec<String> = serde_json::from_slice(&bytes).unwrap_or_default();
    for uuid in &turned_off {
        gnome_extensions(&["enable", uuid]);
    }
    let _ = std::fs::remove_file(system_dock_file());
    crate::log::line(format!("dock: turned back on {}", turned_off.join(", ")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shells_apps_make_the_row_after_gnomes_favourites() {
        let json = r#"{"apps":[
            {"id":"org.gnome.Terminal.desktop","name":"Terminal","windows":[40,41],"attention":false},
            {"id":"window:7","name":"xeyes","windows":[50],"attention":true}],
            "favorites":["org.gnome.Nautilus.desktop","org.gnome.Terminal.desktop","gone.desktop"],
            "focus":40,"fullscreen":[[1920,0]]}"#;
        let shell: ShellApps = serde_json::from_str(json).unwrap();
        let app = |id: &str, name: &str| Some(crate::quick::App { id: id.into(), name: name.into(), icon: None });
        let apps = HashMap::from([
            ("org.gnome.Nautilus.desktop".to_string(), app("org.gnome.Nautilus.desktop", "Files")),
            ("org.gnome.Terminal.desktop".to_string(), app("org.gnome.Terminal.desktop", "Terminal")),
            // A favourite whose .desktop file is gone, and an app with none.
            ("gone.desktop".to_string(), None),
            ("window:7".to_string(), None),
        ]);
        let row = row(&shell, &apps, &mut Vec::new());
        let ids: Vec<&str> = row.iter().map(|e| e.app.id.as_str()).collect();
        assert_eq!(ids, ["org.gnome.Nautilus.desktop", "org.gnome.Terminal.desktop", "window:7"]);
        assert!(row[0].app.pinned && row[0].windows.is_empty());
        // The terminal's window in front: a click goes round to its other one.
        assert!(row[1].app.active && row[1].app.windows == 2);
        assert_eq!(super::super::click(&row[1].windows, shell.focus as isize), super::super::Click::Front(41));
        assert!(row[2].app.attention && !row[2].app.pinned && row[2].launch == "window:7");
    }
}
