// Linux, best effort:
//   * still desktop pictures on GNOME (and the desktops that share its
//     settings: Budgie, Pantheon, Unity) through gsettings, and on KDE Plasma
//     through plasma-apply-wallpaperimage. Both show one picture on every
//     monitor, so only the primary monitor's picture is applied;
//   * the lock screen on GNOME, where org.gnome.desktop.screensaver still has
//     its picture-uri;
//   * live windows where the compositor has layer-shell (KDE, COSMIC,
//     wlroots): a background-layer surface per monitor, under every window
//     and the icons. Not on GNOME or X11.
// What this desktop can't do, wallpaper_status says.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
use tauri::{AppHandle, WebviewWindow};

use super::live::{Rect, Signals};
use super::original::{self, LockScreen, Original, Picture, Setting};
use super::{MonitorInfo, Screen, Supported};
use crate::i18n::{t, tf};

const BACKGROUND: &str = "org.gnome.desktop.background";
const SCREENSAVER: &str = "org.gnome.desktop.screensaver";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Desk {
    Gnome,
    Plasma,
    Other,
}

fn desk() -> Desk {
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let names: Vec<&str> = current.split(':').collect();
    if names.contains(&"kde") {
        Desk::Plasma
    } else if names.iter().any(|d| matches!(*d, "gnome" | "ubuntu" | "unity" | "budgie" | "pantheon" | "gnome-classic")) {
        Desk::Gnome
    } else {
        Desk::Other
    }
}

fn gsettings(args: &[&str]) -> Option<String> {
    let out = Command::new("gsettings").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn writable(schema: &str, key: &str) -> bool {
    gsettings(&["writable", schema, key]).is_some_and(|v| v == "true")
}

fn set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    let out = Command::new("gsettings").args(["set", schema, key, value]).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// A GVariant string: `'file:///…'`.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// The URI inside a printed GVariant string (`'file:///…'`).
fn unquoted(v: &str) -> String {
    v.trim().trim_matches('\'').replace("\\'", "'").replace("\\\\", "\\")
}

fn gnome_ok() -> bool {
    crate::platform::find_on_path("gsettings").is_some() && writable(BACKGROUND, "picture-uri")
}

fn plasma_tool() -> Option<PathBuf> {
    crate::platform::find_on_path("plasma-apply-wallpaperimage")
}

fn desktop_ok() -> bool {
    match desk() {
        Desk::Gnome => gnome_ok(),
        Desk::Plasma => plasma_tool().is_some(),
        Desk::Other => false,
    }
}

/// GNOME draws its lock screen from the desktop's background: Ciel's picture
/// shows there only through Ciel's extension (gnome-extension/), which draws
/// it from org.gnome.desktop.screensaver instead.
fn lock_ok() -> bool {
    lock_settable() && extension_draws_lock()
}

fn lock_settable() -> bool {
    desk() == Desk::Gnome && crate::platform::find_on_path("gsettings").is_some() && writable(SCREENSAVER, "picture-uri")
}

fn extension_draws_lock() -> bool {
    crate::gnome_ext::version().is_some_and(|v| v >= crate::gnome_ext::LOCK_SCREEN_VERSION)
}

/// CIEL_LAYER_SHELL=0 turns layer-shell off, as for the island.
fn layer_wanted() -> bool {
    std::env::var("CIEL_LAYER_SHELL").map(|v| v != "0").unwrap_or(true)
}

pub async fn supported(app: &AppHandle) -> Supported {
    let live = live_supported(app).await;
    tauri::async_runtime::spawn_blocking(move || {
        let settable = lock_settable();
        let drawn = settable && extension_draws_lock();
        Supported { desktop: desktop_ok(), lock_screen: drawn, live, lock_needs_extension: settable && !drawn }
    })
        .await
        .unwrap_or(Supported { desktop: false, lock_screen: false, live: false, lock_needs_extension: false })
}

pub async fn live_supported(app: &AppHandle) -> bool {
    if !layer_wanted() {
        return false;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        super::live::on_main(&app, || unsafe { layer::gtk_layer_is_supported() } != 0).unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

pub fn desktop_ready() -> bool {
    true
}

pub fn monitors(screens: &[Screen]) -> Result<Vec<MonitorInfo>, String> {
    let ids: Vec<(String, (i32, i32, i32, i32))> = screens
        .iter()
        .map(|s| (s.name.clone(), (s.x, s.y, s.x + s.width as i32, s.y + s.height as i32)))
        .collect();
    Ok(super::match_monitors(&ids, screens))
}

// ── Saving what the user had ─────────────────────────────────────────────────

fn setting(dir: &Path, schema: &str, key: &str, stem: &str, kept: &mut Vec<(PathBuf, String)>) -> Option<Setting> {
    let value = gsettings(&["get", schema, key])?;
    let picture = original::path_from_file_uri(&unquoted(&value)).filter(|p| p.is_file()).and_then(|p| {
        let copy = original::keep_copy(dir, &p, stem, kept).ok()?;
        Some(Picture { path: p.to_string_lossy().into_owned(), copy: Some(copy) })
    });
    Some(Setting { schema: schema.into(), key: key.into(), value, picture })
}

/// The picture Plasma shows, from its desktop configuration.
fn plasma_picture() -> Option<String> {
    let home = crate::platform::home_dir();
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let text = std::fs::read_to_string(config.join("plasma-org.kde.plasma.desktop-appletsrc")).ok()?;
    let mut in_image = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_image = line.ends_with("[Wallpaper][org.kde.image][General]");
        } else if in_image {
            if let Some(value) = line.strip_prefix("Image=") {
                let value = value.trim();
                return Some(original::path_from_file_uri(value).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| value.to_string()));
            }
        }
    }
    None
}

fn save_desktop(dir: &Path) -> Result<original::Desktop, String> {
    let mut kept = Vec::new();
    let mut desktop = original::Desktop::default();
    match desk() {
        Desk::Gnome => {
            for key in ["picture-uri", "picture-uri-dark", "picture-options"] {
                if let Some(s) = setting(dir, BACKGROUND, key, "desktop", &mut kept) {
                    desktop.settings.push(s);
                }
            }
            if desktop.settings.is_empty() {
                return Err(super::not_saved("gsettings"));
            }
        }
        Desk::Plasma => {
            let path = plasma_picture().ok_or_else(|| super::not_saved("plasma-org.kde.plasma.desktop-appletsrc"))?;
            let copy = Path::new(&path)
                .is_file()
                .then(|| original::keep_copy(dir, Path::new(&path), "desktop", &mut kept))
                .transpose()
                .map_err(super::not_saved)?;
            desktop.plasma = Some(Picture { path, copy });
        }
        Desk::Other => return Err(t("Ciel can't change the wallpaper on this system.")),
    }
    Ok(desktop)
}

// ── Applying ──────────────────────────────────────────────────────────────────

/// The picture Ciel last gave Plasma (it has no setting to read it back from).
static PLASMA_NOW: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_desktop(id: &str, png: &[u8], screens: &[Screen]) -> Result<(), String> {
    if !desktop_ok() {
        return Err(t("Ciel can't change the wallpaper on this system."));
    }
    if !screens.iter().any(|s| s.name == id) {
        return Err(t("That screen isn't connected any more."));
    }
    // One picture for every monitor: the primary's.
    let primary = screens.iter().find(|s| s.primary).or(screens.first()).map(|s| s.name.as_str());
    if primary != Some(id) {
        return Ok(());
    }
    let dir = super::dir();
    let mut record = original::load(&dir).unwrap_or_default();
    if record.desktop.is_none() {
        record.desktop = Some(save_desktop(&dir)?);
        original::save(&dir, &record).map_err(super::not_saved)?;
    }
    let stem = format!("desktop-{}", super::safe_id(id));
    match desk() {
        Desk::Gnome => {
            let now = gsettings(&["get", BACKGROUND, "picture-uri"]).and_then(|v| original::path_from_file_uri(&unquoted(&v)));
            let path = super::next_name(&dir, &stem, now.as_deref());
            super::write_atomic(&path, png).map_err(super::not_written)?;
            let uri = quoted(&original::file_uri(&path));
            set(BACKGROUND, "picture-uri", &uri).map_err(super::desktop_refused)?;
            if writable(BACKGROUND, "picture-uri-dark") {
                set(BACKGROUND, "picture-uri-dark", &uri).map_err(super::desktop_refused)?;
            }
            set(BACKGROUND, "picture-options", "'zoom'").map_err(super::desktop_refused)
        }
        Desk::Plasma => {
            let mut now = PLASMA_NOW.lock().unwrap_or_else(|e| e.into_inner());
            let path = super::next_name(&dir, &stem, now.as_deref());
            super::write_atomic(&path, png).map_err(super::not_written)?;
            apply_plasma(&path).map_err(super::desktop_refused)?;
            *now = Some(path);
            Ok(())
        }
        Desk::Other => Err(t("Ciel can't change the wallpaper on this system.")),
    }
}

fn apply_plasma(path: &Path) -> Result<(), String> {
    let tool = plasma_tool().ok_or("plasma-apply-wallpaperimage")?;
    let out = Command::new(tool).arg(path).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn set_lock_screen(png: &[u8]) -> Result<(), String> {
    if !lock_ok() {
        return Err(t("Ciel can't change the lock screen on this system."));
    }
    let dir = super::dir();
    let mut record = original::load(&dir).unwrap_or_default();
    if record.lock_screen.is_none() {
        let mut kept = Vec::new();
        let saved = setting(&dir, SCREENSAVER, "picture-uri", "lockscreen", &mut kept)
            .ok_or_else(|| super::not_saved("gsettings"))?;
        record.lock_screen = Some(LockScreen { settings: vec![saved], ..Default::default() });
        original::save(&dir, &record).map_err(super::not_saved)?;
    }
    let now = gsettings(&["get", SCREENSAVER, "picture-uri"]).and_then(|v| original::path_from_file_uri(&unquoted(&v)));
    let path = super::next_name(&dir, "lockscreen", now.as_deref());
    super::write_atomic(&path, png).map_err(super::not_written)?;
    set(SCREENSAVER, "picture-uri", &quoted(&original::file_uri(&path))).map_err(super::lock_refused)
}

// ── Putting it back ───────────────────────────────────────────────────────────

/// Hands every saved setting back; a picture that moved comes from Ciel's copy.
fn put_back(settings: &[Setting], dir: &Path, used: &mut Vec<PathBuf>) -> Result<(), String> {
    for s in settings {
        let value = match &s.picture {
            Some(picture) => match original::source_for(dir, picture) {
                Some(p) if p != Path::new(&picture.path) => {
                    used.push(p.clone());
                    quoted(&original::file_uri(&p))
                }
                _ => s.value.clone(),
            },
            None => s.value.clone(),
        };
        set(&s.schema, &s.key, &value)?;
    }
    Ok(())
}

pub fn restore() -> Result<(), String> {
    let dir = super::dir();
    let Some(mut record): Option<Original> = original::load(&dir) else { return Ok(()) };
    let mut used = Vec::new();
    let mut failure = None;
    if let Some(desktop) = record.desktop.clone() {
        let mut result = put_back(&desktop.settings, &dir, &mut used);
        if let (Ok(()), Some(picture)) = (&result, &desktop.plasma) {
            result = match original::source_for(&dir, picture) {
                Some(p) => {
                    if p != Path::new(&picture.path) {
                        used.push(p.clone());
                    }
                    apply_plasma(&p)
                }
                // A wallpaper package (a folder) Plasma ships: handed back as it was.
                None if !picture.path.is_empty() => apply_plasma(Path::new(&picture.path)),
                None => Ok(()),
            };
        }
        match result {
            Ok(()) => record.desktop = None,
            Err(e) => failure = Some(tf("Ciel couldn't put your wallpaper back: {error}", &[("error", &e)])),
        }
    }
    if let Some(lock) = record.lock_screen.clone() {
        match put_back(&lock.settings, &dir, &mut used) {
            Ok(()) => record.lock_screen = None,
            Err(e) => {
                failure.get_or_insert(tf("Ciel couldn't put your lock screen back: {error}", &[("error", &e)]));
            }
        }
    }
    original::save(&dir, &record).map_err(|e| tf("Ciel couldn't put your wallpaper back: {error}", &[("error", &e.to_string())]))?;
    if record.desktop.is_none() && record.lock_screen.is_none() {
        original::forget_copies(&dir, &used);
    }
    failure.map_or(Ok(()), Err)
}

// ── The live window ───────────────────────────────────────────────────────────

/// The few gtk-layer-shell calls a background surface needs.
mod layer {
    use gtk::ffi::GtkWindow;
    use std::os::raw::{c_char, c_int};

    pub const LAYER_BACKGROUND: c_int = 0;
    pub const EDGE_LEFT: c_int = 0;
    pub const EDGE_RIGHT: c_int = 1;
    pub const EDGE_TOP: c_int = 2;
    pub const EDGE_BOTTOM: c_int = 3;
    pub const KEYBOARD_NONE: c_int = 0;

    #[link(name = "gtk-layer-shell")]
    extern "C" {
        pub fn gtk_layer_is_supported() -> c_int;
        pub fn gtk_layer_init_for_window(window: *mut GtkWindow);
        pub fn gtk_layer_set_namespace(window: *mut GtkWindow, name_space: *const c_char);
        pub fn gtk_layer_set_layer(window: *mut GtkWindow, layer: c_int);
        pub fn gtk_layer_set_anchor(window: *mut GtkWindow, edge: c_int, anchor: c_int);
        pub fn gtk_layer_set_monitor(window: *mut GtkWindow, monitor: *mut gtk::gdk::ffi::GdkMonitor);
        pub fn gtk_layer_set_exclusive_zone(window: *mut GtkWindow, zone: c_int);
        pub fn gtk_layer_set_keyboard_mode(window: *mut GtkWindow, mode: c_int);
    }
}

fn no_input(w: &gtk::ApplicationWindow) {
    if let Some(gdk_window) = w.window() {
        let empty = gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(0, 0, 0, 0));
        gdk_window.input_shape_combine_region(&empty, 0, 0);
    }
}

/// Main thread, before the window is first shown: a background-layer surface
/// filling the monitor at `rect`, that never takes the keyboard or the mouse.
pub fn place_live(win: &WebviewWindow, rect: Rect) -> Result<(), String> {
    let gw = win.gtk_window().map_err(|e| e.to_string())?;
    if gw.is_realized() {
        return Err("window already shown".into());
    }
    gw.set_titlebar(None::<&gtk::Widget>);
    gw.set_accept_focus(false);
    let window: &gtk::Window = gw.upcast_ref();
    let ptr: *mut gtk::ffi::GtkWindow = window.to_glib_none().0;
    let display = gw.display();
    let monitor = (0..display.n_monitors()).filter_map(|i| display.monitor(i)).find(|m| {
        let g = m.geometry();
        let s = m.scale_factor();
        g.x() * s == rect.x && g.y() * s == rect.y
    });
    unsafe {
        layer::gtk_layer_init_for_window(ptr);
        layer::gtk_layer_set_namespace(ptr, c"ciel-wallpaper".as_ptr());
        layer::gtk_layer_set_layer(ptr, layer::LAYER_BACKGROUND);
        for edge in [layer::EDGE_LEFT, layer::EDGE_RIGHT, layer::EDGE_TOP, layer::EDGE_BOTTOM] {
            layer::gtk_layer_set_anchor(ptr, edge, 1);
        }
        layer::gtk_layer_set_exclusive_zone(ptr, -1);
        layer::gtk_layer_set_keyboard_mode(ptr, layer::KEYBOARD_NONE);
        if let Some(m) = &monitor {
            layer::gtk_layer_set_monitor(ptr, m.to_glib_none().0);
        }
    }
    // Same first-frame trouble as the island's layer surface (platform/linux.rs):
    // unmapped and mapped once, right after the first map, it draws for good.
    let remapped = std::cell::Cell::new(false);
    gw.connect_map_event(move |w, _| {
        no_input(w);
        if !remapped.replace(true) {
            let w = w.clone();
            gtk::glib::idle_add_local_once(move || {
                w.hide();
                w.show_all();
                no_input(&w);
            });
        }
        gtk::glib::Propagation::Proceed
    });
    win.show().map_err(|e| e.to_string())
}

pub fn describe_live(_win: &WebviewWindow) -> String {
    "background layer surface".into()
}

pub fn release_live(_win: &WebviewWindow) {}

pub fn after_live_removed() {}

/// A layer surface stays where it was put.
pub fn live_attached(_win: &WebviewWindow) -> bool {
    true
}

/// Linux only knows about the battery here.
pub fn signals(_monitors: &[Rect]) -> Signals {
    let on_battery = std::fs::read_dir("/sys/class/power_supply").ok().is_some_and(|entries| {
        entries.flatten().any(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).unwrap_or_default();
            read("type").trim() == "Battery" && read("status").trim() == "Discharging"
        })
    });
    Signals { on_battery, ..Default::default() }
}

/// No session watch on Linux yet.
pub fn follow_session(_app: &AppHandle, _on: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gvariant_strings_go_both_ways() {
        assert_eq!(unquoted("'file:///usr/share/backgrounds/a.jpg'"), "file:///usr/share/backgrounds/a.jpg");
        assert_eq!(quoted("file:///home/a/it's.png"), r"'file:///home/a/it\'s.png'");
        assert_eq!(unquoted(&quoted("file:///home/a/it's.png")), "file:///home/a/it's.png");
    }
}
