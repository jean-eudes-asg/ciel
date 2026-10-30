// Ciel's wallpapers, Rust side: putting the pictures the island draws on the
// desktop and the lock screen, the live desktop window behind the icons, the
// free photos, and putting the user's own wallpaper back.
//
// The island draws (src/sky) and decides; Rust only applies, and only when a
// command asks: nothing here runs, polls or touches the desktop while the
// wallpaper settings are off, but for putting the user's own pictures back
// once, when one is turned off.
//
//   * original.rs — what was there before Ciel's first change, for restore;
//   * photos.rs   — NASA and Wikimedia Commons photos, cached with credits;
//   * live.rs     — the live window(s) and what pauses them;
//   * win.rs, layer.rs, session.rs — Windows (IDesktopWallpaper, the WinRT
//     lock screen, the desktop layer, session and power notifications);
//   * linux.rs    — GNOME and KDE, best effort.

mod live;
mod original;
mod photos;

#[cfg(windows)]
mod layer;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as os;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager};

use crate::i18n::{t, tf};
use crate::settings::WallpaperPrefs;

/// Rust → live windows: `{paused, reason}`.
pub const PAUSE_EVENT: &str = "wallpaper-live-pause";
/// Rust → island: `{locked}`.
#[cfg_attr(not(windows), allow(dead_code))]
pub const SESSION_EVENT: &str = "wallpaper-session";

/// Largest picture the island may hand over (a 3440×1440 PNG is a few MB).
const MAX_PNG: usize = 64 * 1024 * 1024;

/// One set or restore at a time: the first change saves the user's own
/// wallpaper, and two monitors set at once must not both think they are first.
static APPLY: Mutex<()> = Mutex::new(());

fn apply_lock() -> std::sync::MutexGuard<'static, ()> {
    APPLY.lock().unwrap_or_else(|e| e.into_inner())
}

/// `<data dir>/wallpaper`.
pub fn dir() -> PathBuf {
    crate::settings::local_dir().join("wallpaper")
}

/// Writes beside `path` and renames over it: a crash never leaves half a picture.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        crate::platform::ensure_private_dir(parent)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".ciel-{}", std::process::id()));
    let temp = path.with_file_name(name);
    let written = std::fs::File::create(&temp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

pub(crate) fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

/// A monitor id as a file name: what is safe kept, a hash so two ids that
/// read the same after that never share a file.
pub(crate) fn safe_id(id: &str) -> String {
    let mut out: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    out = out.trim_matches('_').to_string();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.truncate(60);
    let hash = id.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{out}-{:08x}", hash as u32)
}

/// `<stem>.png`, or `<stem>-1.png` when `<stem>.png` is the picture shown now:
/// Windows and GNOME may keep showing what they read from a path they already
/// know, so a new picture always comes under the other name.
pub(crate) fn next_name(dir: &Path, stem: &str, current: Option<&Path>) -> PathBuf {
    let first = dir.join(format!("{stem}.png"));
    let same = |a: &Path, b: &Path| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy());
    match current {
        Some(now) if same(now, &first) => dir.join(format!("{stem}-1.png")),
        _ => first,
    }
}

/// `%XX` sequences decoded (a header value may come through encodeURIComponent).
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(b) = hex {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── Arguments ─────────────────────────────────────────────────────────────────
//
// The pictures come as raw bytes: `invoke(cmd, pngBytes, { headers: { monitorId } })`
// puts the PNG in the request body and the other arguments in headers. A JSON
// call (`{ monitorId, png: [..] }`) is taken too.

fn text_arg(request: &Request<'_>, name: &str) -> Option<String> {
    if let Some(value) = request.headers().get(name).and_then(|v| v.to_str().ok()) {
        return Some(percent_decode(value));
    }
    match request.body() {
        InvokeBody::Json(v) => v.get(name).and_then(|v| v.as_str()).map(str::to_string),
        InvokeBody::Raw(_) => None,
    }
}

fn png_arg(request: &Request<'_>) -> Result<Vec<u8>, String> {
    let bytes = match request.body() {
        InvokeBody::Raw(bytes) => bytes.clone(),
        InvokeBody::Json(v) => v
            .get("png")
            .and_then(|p| p.as_array())
            .map(|a| a.iter().map(|n| n.as_u64().unwrap_or(0).min(255) as u8).collect())
            .unwrap_or_default(),
    };
    if bytes.len() > MAX_PNG || !is_png(&bytes) {
        return Err(t("That picture isn't a PNG image."));
    }
    Ok(bytes)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

// ── Monitors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    /// Windows: the device path IDesktopWallpaper uses. Linux: the output name.
    pub id: String,
    /// Physical pixels.
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub primary: bool,
}

/// A display as Tauri sees it: physical position and size, scale, primary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Screen {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub primary: bool,
}

pub(crate) fn screens(app: &AppHandle) -> Vec<Screen> {
    let primary = app.primary_monitor().ok().flatten().map(|m| (*m.position(), *m.size()));
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| Screen {
            name: m.name().cloned().unwrap_or_default(),
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
            scale: m.scale_factor(),
            primary: primary.is_some_and(|(p, s)| p == *m.position() && s == *m.size()),
        })
        .collect()
}

/// The wallpaper API's monitors (id and rectangle, physical pixels), with the
/// scale and the primary flag of the Tauri display at the same rectangle.
/// Without one, scale 1 and primary where the origin is.
pub(crate) fn match_monitors(ids: &[(String, (i32, i32, i32, i32))], screens: &[Screen]) -> Vec<MonitorInfo> {
    ids.iter()
        .map(|(id, (l, t, r, b))| {
            let screen = screens
                .iter()
                .find(|s| s.x == *l && s.y == *t && s.width as i32 == r - l && s.height as i32 == b - t);
            MonitorInfo {
                id: id.clone(),
                x: *l,
                y: *t,
                width: (r - l).max(0) as u32,
                height: (b - t).max(0) as u32,
                scale: screen.map(|s| s.scale).unwrap_or(1.0),
                primary: screen.map(|s| s.primary).unwrap_or(*l == 0 && *t == 0),
            }
        })
        .collect()
}

/// The monitors, in the order the live windows are numbered (`wallpaper-<n>`).
pub(crate) async fn monitors(app: &AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let screens = screens(app);
    blocking(move || os::monitors(&screens)).await
}

#[tauri::command]
pub async fn wallpaper_monitors(app: AppHandle) -> Result<Vec<MonitorInfo>, String> {
    monitors(&app).await
}

// ── Desktop and lock screen ───────────────────────────────────────────────────

/// Whether the user still wants Ciel's pictures there. Asked once the picture
/// may go on: one drawn before they turned it off doesn't go on after.
fn wanted(app: &AppHandle, part: fn(&WallpaperPrefs) -> bool) -> bool {
    app.try_state::<crate::Shared>().is_none_or(|s| part(&s.settings.lock().unwrap().wallpaper))
}

/// Blocking: `png` on the monitor `id`, saving the user's own pictures first
/// if this is Ciel's first change.
pub(crate) fn apply_desktop(app: &AppHandle, id: &str, png: &[u8], screens: &[Screen]) -> Result<(), String> {
    let _apply = apply_lock();
    if !wanted(app, |w| w.desktop) {
        return Ok(());
    }
    os::set_desktop(id, png, screens)
}

/// Blocking: `png` on the lock screen, saving the user's own picture first.
/// False when the lock screen was turned off meanwhile and nothing changed.
pub(crate) fn apply_lock_screen(app: &AppHandle, png: &[u8]) -> Result<bool, String> {
    let _apply = apply_lock();
    if !wanted(app, |w| w.lock_screen) {
        return Ok(false);
    }
    os::set_lock_screen(png).map(|()| true)
}

/// Blocking: what was there before Ciel's first change, back.
pub(crate) fn put_back() -> Result<(), String> {
    let _apply = apply_lock();
    os::restore(true, true)
}

/// The desktop and the lock screen Ciel changed and the user has turned off.
fn turned_off(prefs: &WallpaperPrefs, record: &original::Original) -> (bool, bool) {
    (!prefs.desktop && record.desktop.is_some(), !prefs.lock_screen && record.lock_screen.is_some())
}

/// What the user turned off gets their own picture back, so Ciel's last one
/// doesn't stay on the desktop or the lock screen. At launch too, for a
/// picture an earlier version left behind.
fn put_back_turned_off(prefs: &WallpaperPrefs) {
    let Some(record) = original::load(&dir()) else { return };
    let (desktop, lock_screen) = turned_off(prefs, &record);
    if !desktop && !lock_screen {
        return;
    }
    std::thread::spawn(move || {
        let _apply = apply_lock();
        if let Err(err) = os::restore(desktop, lock_screen) {
            crate::log::line(format!("wallpaper: turned off, but {err}"));
        }
    });
}

#[tauri::command]
pub async fn wallpaper_set_desktop(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let id = text_arg(&request, "monitorId").unwrap_or_default();
    let png = png_arg(&request)?;
    let screens = screens(&app);
    blocking(move || apply_desktop(&app, &id, &png, &screens)).await
}

#[tauri::command]
pub async fn wallpaper_set_lock_screen(app: AppHandle, request: Request<'_>) -> Result<(), String> {
    let png = png_arg(&request)?;
    let handle = app.clone();
    if blocking(move || apply_lock_screen(&handle, &png)).await? {
        // The island may want to redraw it around a lock from now on.
        live::follow_session(&app, true);
    }
    Ok(())
}

/// Puts back what was there before Ciel's first change, and stops live mode.
#[tauri::command]
pub async fn wallpaper_restore(app: AppHandle) -> Result<(), String> {
    live::stop(&app).await;
    blocking(put_back).await
}

// ── Photos ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn wallpaper_photos(theme: String) -> Result<Vec<photos::Photo>, String> {
    photos::cached(&theme)
}

/// Downloads up to `count` new photos for `theme`; how many came.
#[tauri::command]
pub async fn wallpaper_photos_fetch(theme: String, count: u32) -> Result<u32, String> {
    photos::fetch(&theme, count).await
}

/// A cached photo's bytes, for the webview to decode (its CSP forbids file URLs).
#[tauri::command]
pub async fn wallpaper_photo_bytes(path: String) -> Result<Response, String> {
    blocking(move || photos::bytes(&path)).await.map(Response::new)
}

// ── Live ──────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn wallpaper_live(app: AppHandle, enabled: bool) -> Result<(), String> {
    if enabled {
        live::start(&app).await
    } else {
        live::stop(&app).await;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Supported {
    pub desktop: bool,
    pub lock_screen: bool,
    pub live: bool,
    /// GNOME: the lock screen waits for Ciel's extension (gnome_ext.rs).
    pub lock_needs_extension: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub supported: Supported,
    /// Live windows are up.
    pub live: bool,
    /// Every live window is paused right now.
    pub paused: bool,
    /// Ciel has saved the user's own wallpaper or lock screen, so there is
    /// something to restore.
    pub saved: bool,
}

#[tauri::command]
pub async fn wallpaper_status(app: AppHandle) -> Status {
    let supported = os::supported(&app).await;
    let saved = original::load(&dir()).is_some_and(|o| o.desktop.is_some() || o.lock_screen.is_some());
    Status { supported, live: live::is_live(), paused: live::all_paused(), saved }
}

// ── Following the settings ───────────────────────────────────────────────────

/// At launch and after every settings save: with the lock screen on, the
/// session is followed so the island hears about locks; otherwise nothing of
/// the wallpapers runs (unless live mode is up). What was turned off gets the
/// user's own picture back.
pub fn sync(app: &AppHandle, prefs: &WallpaperPrefs) {
    live::set_pause_on_battery(prefs.pause_on_battery);
    live::follow_session(app, prefs.lock_screen);
    put_back_turned_off(prefs);
}

/// The app is quitting: live windows leave the desktop layer and the picture
/// underneath is repainted, so nothing of them stays on screen.
pub fn shutdown(app: &AppHandle) {
    live::shutdown(app);
}

/// The user-facing message for an OS error while setting the desktop picture.
pub(crate) fn desktop_refused(error: impl std::fmt::Display) -> String {
    tf("The system refused the new wallpaper: {error}", &[("error", &error.to_string())])
}

/// The user-facing message for an OS error while setting the lock screen.
pub(crate) fn lock_refused(error: impl std::fmt::Display) -> String {
    tf("The system refused the new lock screen picture: {error}", &[("error", &error.to_string())])
}

/// Saving the user's own wallpaper failed: nothing is changed.
pub(crate) fn not_saved(error: impl std::fmt::Display) -> String {
    tf(
        "Ciel couldn't keep a copy of your current picture, so nothing was changed: {error}",
        &[("error", &error.to_string())],
    )
}

/// Writing Ciel's picture to disk failed.
pub(crate) fn not_written(error: impl std::fmt::Display) -> String {
    tf("Ciel couldn't save the picture: {error}", &[("error", &error.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_monitor_id_becomes_a_file_name_that_is_its_own() {
        let a = safe_id(r"\\?\DISPLAY#XYP3429#5&2c0a76fb&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}");
        assert!(a.starts_with("DISPLAY_XYP3429_5_2c0a76fb_0_UID4352_e6f07b5f"), "{a}");
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(a.len() <= 69);
        // Same safe characters, different ids: different names.
        assert_ne!(safe_id("DP-1"), safe_id("DP_1"));
        assert_eq!(safe_id("DP-1"), safe_id("DP-1"));
        assert!(!safe_id("../../etc").contains(['.', '/', '\\']));
    }

    #[test]
    fn a_new_picture_never_reuses_the_path_shown_now() {
        let dir = Path::new("/w");
        assert_eq!(next_name(dir, "lockscreen", None), dir.join("lockscreen.png"));
        assert_eq!(next_name(dir, "lockscreen", Some(&dir.join("lockscreen.png"))), dir.join("lockscreen-1.png"));
        assert_eq!(next_name(dir, "lockscreen", Some(&dir.join("lockscreen-1.png"))), dir.join("lockscreen.png"));
        assert_eq!(next_name(dir, "lockscreen", Some(Path::new("/elsewhere/x.jpg"))), dir.join("lockscreen.png"));
    }

    #[test]
    fn header_values_are_percent_decoded() {
        assert_eq!(percent_decode("%5C%5C%3F%5CDISPLAY%23X"), r"\\?\DISPLAY#X");
        assert_eq!(percent_decode(r"\\?\DISPLAY#X&0"), r"\\?\DISPLAY#X&0");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn what_is_turned_off_gets_the_users_own_picture_back() {
        let prefs = |desktop, lock_screen| WallpaperPrefs { desktop, lock_screen, ..WallpaperPrefs::default() };
        let both = original::Original {
            desktop: Some(Default::default()),
            lock_screen: Some(Default::default()),
            ..Default::default()
        };
        let lock_only = original::Original { lock_screen: Some(Default::default()), ..Default::default() };
        assert_eq!(turned_off(&prefs(true, true), &both), (false, false));
        assert_eq!(turned_off(&prefs(true, false), &both), (false, true));
        assert_eq!(turned_off(&prefs(false, true), &both), (true, false));
        assert_eq!(turned_off(&prefs(false, false), &both), (true, true));
        // Only what Ciel changed: the desktop it never touched stays as it is.
        assert_eq!(turned_off(&prefs(false, false), &lock_only), (false, true));
        assert_eq!(turned_off(&prefs(false, false), &original::Original::default()), (false, false));
    }

    #[test]
    fn only_png_bytes_are_pictures() {
        assert!(is_png(b"\x89PNG\r\n\x1a\nrest"));
        assert!(!is_png(b"\xff\xd8\xff\xe0 jpeg"));
        assert!(!is_png(b""));
    }

    #[test]
    fn monitors_are_matched_to_displays_by_rectangle() {
        let screens = vec![
            Screen { name: "a".into(), x: 0, y: 0, width: 3440, height: 1440, scale: 1.0, primary: true },
            Screen { name: "b".into(), x: -1920, y: 865, width: 1920, height: 1200, scale: 1.25, primary: false },
        ];
        let ids = vec![
            ("left".to_string(), (-1920, 865, 0, 2065)),
            ("main".to_string(), (0, 0, 3440, 1440)),
            ("gone".to_string(), (5000, 0, 6920, 1080)),
        ];
        let m = match_monitors(&ids, &screens);
        assert_eq!((m[0].x, m[0].y, m[0].width, m[0].height, m[0].scale, m[0].primary), (-1920, 865, 1920, 1200, 1.25, false));
        assert_eq!((m[1].width, m[1].height, m[1].scale, m[1].primary), (3440, 1440, 1.0, true));
        assert_eq!((m[2].scale, m[2].primary), (1.0, false));
    }

    #[test]
    fn a_picture_is_written_whole_or_not_at_all() {
        let dir = std::env::temp_dir().join(format!("ciel-wallpaper-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("desktop-x.png");
        write_atomic(&file, b"one").unwrap();
        write_atomic(&file, b"two").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
