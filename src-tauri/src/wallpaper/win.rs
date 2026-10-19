// Windows: the desktop picture through IDesktopWallpaper (one per monitor),
// the lock screen through WinRT's LockScreen (which works from an unpackaged
// app), saving and putting back what the user had, and the probes the live
// window's pause poll reads.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, WebviewWindow};
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Storage::StorageFile;
use windows::Storage::Streams::DataReader;
use windows::System::UserProfile::LockScreen as WinLockScreen;
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, SHCreateShellItemArrayFromIDLists, SHParseDisplayName,
    SHQueryUserNotificationState, DESKTOP_SLIDESHOW_OPTIONS, DESKTOP_WALLPAPER_POSITION, DSS_SLIDESHOW, DWPOS_FILL,
    QUNS_NOT_PRESENT, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SIGDN_FILESYSPATH,
};
use windows::core::BOOL;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed, GA_PARENT,
};

use super::live::{Rect, Signals};
use super::original::{self, LockScreen, MonitorPicture, Picture, Slideshow};
use super::{layer, MonitorInfo, Screen, Supported};
use crate::i18n::{t, tf};

// ── COM ───────────────────────────────────────────────────────────────────────

/// Runs `f` on a thread of its own with COM set up: an apartment thread for
/// the shell's IDesktopWallpaper, the multithreaded one for WinRT (whose
/// blocking `get()` must not wait inside an apartment).
fn with_com<T: Send>(multithreaded: bool, f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        s.spawn(|| {
            let model = if multithreaded { COINIT_MULTITHREADED } else { COINIT_APARTMENTTHREADED };
            let ready = unsafe { CoInitializeEx(None, model) }.is_ok();
            let out = f();
            if ready {
                unsafe { CoUninitialize() };
            }
            out
        })
        .join()
        .expect("COM thread panicked")
    })
}

/// A string the shell allocated, freed once read.
fn take(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

fn wide(s: &str) -> HSTRING {
    HSTRING::from(s)
}

fn desktop_wallpaper() -> Result<IDesktopWallpaper, String> {
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }
        .map_err(|_| t("Ciel can't change the wallpaper on this system."))
}

/// Every monitor the wallpaper API knows that is connected: id and rectangle.
fn connected(wp: &IDesktopWallpaper) -> Vec<(String, (i32, i32, i32, i32))> {
    let count = unsafe { wp.GetMonitorDevicePathCount() }.unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            let id = take(unsafe { wp.GetMonitorDevicePathAt(i) }.ok()?);
            let r = unsafe { wp.GetMonitorRECT(&wide(&id)) }.ok()?;
            (r.right > r.left && r.bottom > r.top).then_some((id, (r.left, r.top, r.right, r.bottom)))
        })
        .collect()
}

fn wallpaper_of(wp: &IDesktopWallpaper, id: &str) -> String {
    unsafe { wp.GetWallpaper(&wide(id)) }.map(take).unwrap_or_default()
}

pub fn monitors(screens: &[Screen]) -> Result<Vec<MonitorInfo>, String> {
    with_com(false, || {
        let wp = desktop_wallpaper()?;
        Ok(super::match_monitors(&connected(&wp), screens))
    })
}

// ── Registry ──────────────────────────────────────────────────────────────────

const DESKTOP_KEY: &str = r"Control Panel\Desktop";
const DESKTOP_VALUES: [&str; 3] = ["WallPaper", "WallpaperStyle", "TileWallpaper"];

fn reg_string(root: HKEY, key: &str, name: &str) -> Option<String> {
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let status = unsafe {
        RegGetValueW(root, &wide(key), &wide(name), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if status.is_err() {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_string())
}

fn reg_dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(root, &wide(key), &wide(name), RRF_RT_REG_DWORD, None, Some((&mut value as *mut u32).cast()), Some(&mut size))
    };
    status.is_ok().then_some(value)
}

fn set_reg_string(key: &str, name: &str, value: &str) -> Result<(), String> {
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let status = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, &wide(key), &wide(name), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32)
    };
    status.ok().map_err(|e| e.to_string())
}

/// A policy that keeps the user from changing it (set by an administrator).
fn locked_by_policy(key: &str, name: &str) -> bool {
    [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].iter().any(|root| reg_dword(*root, key, name).is_some_and(|v| v != 0))
}

// ── What can be done here ─────────────────────────────────────────────────────

pub async fn supported(_app: &AppHandle) -> Supported {
    tauri::async_runtime::spawn_blocking(|| {
        let desktop = !locked_by_policy(r"Software\Microsoft\Windows\CurrentVersion\Policies\ActiveDesktop", "NoChangingWallPaper")
            && reg_string(HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Policies\System", "Wallpaper").is_none()
            && with_com(false, || desktop_wallpaper().is_ok());
        let lock_screen = !locked_by_policy(r"Software\Policies\Microsoft\Windows\Personalization", "NoChangingLockScreen")
            && with_com(true, || WinLockScreen::OriginalImageFile().is_ok());
        Supported { desktop, lock_screen, live: desktop_ready() }
    })
    .await
    .unwrap_or(Supported { desktop: false, lock_screen: false, live: false })
}

/// Explorer's desktop is there to put a window behind (looked up, nothing sent).
pub fn desktop_ready() -> bool {
    unsafe { FindWindowW(windows::core::w!("Progman"), PCWSTR::null()) }.is_ok()
}

pub async fn live_supported(_app: &AppHandle) -> bool {
    desktop_ready()
}

// ── Saving what the user had ─────────────────────────────────────────────────

fn save_desktop(wp: &IDesktopWallpaper, dir: &Path) -> Result<original::Desktop, String> {
    let mut kept = Vec::new();
    let count = unsafe { wp.GetMonitorDevicePathCount() }.map_err(super::not_saved)?;
    let mut monitors = Vec::new();
    for i in 0..count {
        let Ok(id) = unsafe { wp.GetMonitorDevicePathAt(i) }.map(take) else { continue };
        let path = wallpaper_of(wp, &id);
        let copy = if !path.is_empty() && Path::new(&path).is_file() {
            Some(original::keep_copy(dir, Path::new(&path), "desktop", &mut kept).map_err(super::not_saved)?)
        } else {
            None
        };
        monitors.push(MonitorPicture { id, picture: Picture { path, copy } });
    }
    let slideshow = unsafe { wp.GetStatus() }
        .ok()
        .filter(|s| s.0 & DSS_SLIDESHOW.0 != 0)
        .and_then(|_| slideshow_of(wp));
    let registry = DESKTOP_VALUES
        .iter()
        .filter_map(|name| reg_string(HKEY_CURRENT_USER, DESKTOP_KEY, name).map(|v| (name.to_string(), v)))
        .collect();
    Ok(original::Desktop {
        monitors,
        position: unsafe { wp.GetPosition() }.ok().map(|p| p.0),
        background_color: unsafe { wp.GetBackgroundColor() }.ok().map(|c| c.0),
        slideshow,
        registry,
        ..Default::default()
    })
}

fn slideshow_of(wp: &IDesktopWallpaper) -> Option<Slideshow> {
    let items = unsafe { wp.GetSlideshow() }.ok()?;
    let count = unsafe { items.GetCount() }.ok()?;
    let paths = (0..count)
        .filter_map(|i| {
            let item = unsafe { items.GetItemAt(i) }.ok()?;
            Some(take(unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?))
        })
        .filter(|p| !p.is_empty())
        .collect();
    let mut options = DESKTOP_SLIDESHOW_OPTIONS::default();
    let mut tick = 0u32;
    unsafe {
        let _ = wp.GetSlideshowOptions(&mut options, &mut tick);
    }
    Some(Slideshow { items: paths, options: options.0 as u32, tick })
}

fn save_lock_screen(dir: &Path) -> Result<LockScreen, String> {
    let uri = WinLockScreen::OriginalImageFile()
        .and_then(|u| u.AbsoluteUri())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if let Some(path) = original::path_from_file_uri(&uri).filter(|p| p.is_file()) {
        let copy = original::keep_copy(dir, &path, "lockscreen", &mut Vec::new()).map_err(super::not_saved)?;
        let picture = Picture { path: path.to_string_lossy().into_owned(), copy: Some(copy) };
        return Ok(LockScreen { uri, picture: Some(picture), ..Default::default() });
    }
    // Not a file of the user's (a picture Windows ships, say): the image the
    // lock screen shows, as Windows hands it over.
    let bytes = lock_screen_bytes().map_err(super::not_saved)?;
    let ext = match super::photos::image_size(&bytes) {
        Some((_, _, ext)) => ext,
        None if bytes.starts_with(b"BM") => "bmp",
        None => "img",
    };
    let copy = original::keep_bytes(dir, &bytes, "lockscreen", ext).map_err(super::not_saved)?;
    Ok(LockScreen { uri, picture: Some(Picture { path: String::new(), copy: Some(copy) }), ..Default::default() })
}

fn lock_screen_bytes() -> windows::core::Result<Vec<u8>> {
    let stream = WinLockScreen::GetImageStream()?;
    let size = stream.Size()? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
    let loaded = reader.LoadAsync(size)?.get()?;
    let mut bytes = vec![0u8; loaded as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}

// ── Applying ──────────────────────────────────────────────────────────────────

pub fn set_desktop(id: &str, png: &[u8], _screens: &[Screen]) -> Result<(), String> {
    with_com(false, || {
        let wp = desktop_wallpaper()?;
        if !connected(&wp).iter().any(|(known, _)| known == id) {
            return Err(t("That screen isn't connected any more."));
        }
        let dir = super::dir();
        let mut record = original::load(&dir).unwrap_or_default();
        if record.desktop.is_none() {
            record.desktop = Some(save_desktop(&wp, &dir)?);
            original::save(&dir, &record).map_err(super::not_saved)?;
            crate::log::line("wallpaper: the user's desktop pictures are saved for restore");
        }
        let now = wallpaper_of(&wp, id);
        let path = super::next_name(&dir, &format!("desktop-{}", super::safe_id(id)), Some(Path::new(&now)));
        super::write_atomic(&path, png).map_err(super::not_written)?;
        unsafe {
            wp.SetWallpaper(&wide(id), &wide(&path.to_string_lossy())).map_err(super::desktop_refused)?;
            wp.SetPosition(DWPOS_FILL).map_err(super::desktop_refused)?;
        }
        Ok(())
    })
}

fn apply_lock_screen(path: &Path) -> windows::core::Result<()> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.get()?;
    WinLockScreen::SetImageFileAsync(&file)?.get()
}

pub fn set_lock_screen(png: &[u8]) -> Result<(), String> {
    with_com(true, || {
        let dir = super::dir();
        let mut record = original::load(&dir).unwrap_or_default();
        if record.lock_screen.is_none() {
            record.lock_screen = Some(save_lock_screen(&dir)?);
            original::save(&dir, &record).map_err(super::not_saved)?;
            crate::log::line("wallpaper: the user's lock screen picture is saved for restore");
        }
        let now = WinLockScreen::OriginalImageFile()
            .and_then(|u| u.AbsoluteUri())
            .ok()
            .and_then(|u| original::path_from_file_uri(&u.to_string()));
        let path = super::next_name(&dir, "lockscreen", now.as_deref());
        super::write_atomic(&path, png).map_err(super::not_written)?;
        apply_lock_screen(&path).map_err(super::lock_refused)
    })
}

// ── Putting it back ───────────────────────────────────────────────────────────

fn restore_desktop(saved: &original::Desktop, dir: &Path) -> Result<Vec<PathBuf>, String> {
    let fail = |e: &dyn std::fmt::Display| tf("Ciel couldn't put your wallpaper back: {error}", &[("error", &e.to_string())]);
    let wp = desktop_wallpaper()?;
    let mut used = Vec::new();
    let mut all_where_they_were = true;
    // A monitor connected since gets the picture the first one had.
    let fallback = saved.monitors.iter().find(|m| !m.picture.path.is_empty() || m.picture.copy.is_some());
    for (id, _) in connected(&wp) {
        let Some(m) = saved.monitors.iter().find(|m| m.id == id).or(fallback) else { continue };
        let source = if m.picture.path.is_empty() && m.picture.copy.is_none() {
            // A plain colour: no picture at all.
            String::new()
        } else {
            match original::source_for(dir, &m.picture) {
                Some(p) => {
                    if p != Path::new(&m.picture.path) {
                        all_where_they_were = false;
                        used.push(p.clone());
                    }
                    p.to_string_lossy().into_owned()
                }
                None => {
                    all_where_they_were = false;
                    continue;
                }
            }
        };
        unsafe { wp.SetWallpaper(&wide(&id), &wide(&source)) }.map_err(|e| fail(&e))?;
    }
    unsafe {
        if let Some(p) = saved.position {
            wp.SetPosition(DESKTOP_WALLPAPER_POSITION(p)).map_err(|e| fail(&e))?;
        }
        if let Some(c) = saved.background_color {
            let _ = wp.SetBackgroundColor(COLORREF(c));
        }
    }
    if let Some(show) = &saved.slideshow {
        if let Err(err) = restore_slideshow(&wp, show) {
            crate::log::line(format!("wallpaper: the slideshow could not be put back: {err}"));
        }
    }
    // The values Windows keeps for the picture, exactly as they were, when the
    // pictures are back where they were.
    if all_where_they_were {
        for (name, value) in &saved.registry {
            if let Err(err) = set_reg_string(DESKTOP_KEY, name, value) {
                crate::log::line(format!("wallpaper: {name} not written back: {err}"));
            }
        }
    }
    Ok(used)
}

fn restore_slideshow(wp: &IDesktopWallpaper, show: &Slideshow) -> windows::core::Result<()> {
    let mut lists: Vec<*mut ITEMIDLIST> = Vec::new();
    for item in show.items.iter().filter(|p| Path::new(p).exists()) {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if unsafe { SHParseDisplayName(&wide(item), None, &mut pidl, 0, None) }.is_ok() && !pidl.is_null() {
            lists.push(pidl);
        }
    }
    let result = (|| {
        if lists.is_empty() {
            return Ok(());
        }
        let refs: Vec<*const ITEMIDLIST> = lists.iter().map(|p| *p as *const _).collect();
        let array = unsafe { SHCreateShellItemArrayFromIDLists(&refs) }?;
        unsafe {
            wp.SetSlideshow(&array)?;
            wp.SetSlideshowOptions(DESKTOP_SLIDESHOW_OPTIONS(show.options as i32), show.tick)
        }
    })();
    for pidl in lists {
        unsafe { CoTaskMemFree(Some(pidl as *const _)) };
    }
    result
}

fn restore_lock_screen(saved: &LockScreen, dir: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(picture) = &saved.picture else { return Ok(Vec::new()) };
    let fail = |e: &dyn std::fmt::Display| tf("Ciel couldn't put your lock screen back: {error}", &[("error", &e.to_string())]);
    let source = original::source_for(dir, picture).ok_or_else(|| fail(&"the saved picture is gone"))?;
    apply_lock_screen(&source).map_err(|e| fail(&e))?;
    Ok(if source != Path::new(&picture.path) { vec![source] } else { Vec::new() })
}

/// Ciel's own pictures in the wallpaper folder, once nothing shows them.
fn forget_own_pictures(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let own = (name.starts_with("desktop-") || name.starts_with("lockscreen")) && name.ends_with(".png");
        if own && entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub fn restore() -> Result<(), String> {
    let dir = super::dir();
    let Some(mut record) = original::load(&dir) else { return Ok(()) };
    let mut keep = Vec::new();
    let mut failure = None;
    if let Some(desktop) = record.desktop.clone() {
        match with_com(false, || restore_desktop(&desktop, &dir)) {
            Ok(used) => {
                keep.extend(used);
                record.desktop = None;
            }
            Err(err) => failure = Some(err),
        }
    }
    if let Some(lock) = record.lock_screen.clone() {
        match with_com(true, || restore_lock_screen(&lock, &dir)) {
            Ok(used) => {
                keep.extend(used);
                record.lock_screen = None;
            }
            Err(err) => {
                failure.get_or_insert(err);
            }
        }
    }
    // Only what was put back leaves the record.
    original::save(&dir, &record).map_err(|e| tf("Ciel couldn't put your wallpaper back: {error}", &[("error", &e.to_string())]))?;
    if record.desktop.is_none() && record.lock_screen.is_none() {
        original::forget_copies(&dir, &keep);
        forget_own_pictures(&dir);
        crate::log::line("wallpaper: the user's pictures are back");
    }
    failure.map_or(Ok(()), Err)
}

// ── The live window ───────────────────────────────────────────────────────────

fn hwnd_of(win: &WebviewWindow) -> Option<HWND> {
    let raw = win.hwnd().ok()?.0;
    (!raw.is_null()).then_some(HWND(raw))
}

fn rect_of(r: Rect) -> RECT {
    RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

/// Main thread: behind the icons, over `rect`.
pub fn place_live(win: &WebviewWindow, rect: Rect) -> Result<(), String> {
    let hwnd = hwnd_of(win).ok_or("no window handle")?;
    let layer = layer::attach(hwnd, rect_of(rect))?;
    crate::log::line(format!("wallpaper: desktop layout {}", layer.layout.as_str()));
    Ok(())
}

pub fn describe_live(win: &WebviewWindow) -> String {
    hwnd_of(win).map(layer::describe).unwrap_or_default()
}

/// Main thread: out of the desktop layer, before it is destroyed.
pub fn release_live(win: &WebviewWindow) {
    if let Some(hwnd) = hwnd_of(win) {
        layer::detach(hwnd);
    }
}

pub fn after_live_removed() {
    layer::refresh();
}

/// Still a child of the desktop (Progman or a WorkerW).
pub fn live_attached(win: &WebviewWindow) -> bool {
    let Some(hwnd) = hwnd_of(win) else { return false };
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        let parent = GetAncestor(hwnd, GA_PARENT);
        matches!(class_of(parent).as_str(), "Progman" | "WorkerW")
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

// ── What pauses it ────────────────────────────────────────────────────────────

/// The window in front, if it is an app's own (not the desktop, the taskbar
/// or one of Ciel's): its rectangle, and the monitor it is on.
fn foreground() -> Option<(RECT, RECT)> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        if pid == std::process::id() {
            return None;
        }
        let class = class_of(fg);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return None;
        }
        let mut rect = RECT::default();
        GetWindowRect(fg, &mut rect).ok()?;
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let screen = if GetMonitorInfoW(monitor, &mut info).as_bool() { info.rcMonitor } else { rect };
        Some((rect, screen))
    }
}

/// The monitors that a visible, maximized window fills. Cloaked windows (on
/// another virtual desktop, or a suspended store app) and Ciel's own don't count.
fn covered_monitors() -> Vec<Rect> {
    unsafe extern "system" fn each(hwnd: HWND, out: LPARAM) -> BOOL {
        let found = &mut *(out.0 as *mut Vec<Rect>);
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || !IsZoomed(hwnd).as_bool() {
            return true.into();
        }
        let mut cloaked = 0u32;
        let attr = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, std::mem::size_of::<u32>() as u32);
        if attr.is_ok() && cloaked != 0 {
            return true.into();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return true.into();
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            let screen = to_rect(info.rcMonitor);
            if !found.contains(&screen) {
                found.push(screen);
            }
        }
        true.into()
    }
    let mut found: Vec<Rect> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut Vec<Rect> as isize));
    }
    found
}

fn to_rect(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

pub fn signals(_monitors: &[Rect]) -> Signals {
    let state = unsafe { SHQueryUserNotificationState() }.ok();
    let mut out = Signals {
        locked: super::session::locked() || state == Some(QUNS_NOT_PRESENT),
        display_off: super::session::display_off(),
        presentation: state == Some(QUNS_PRESENTATION_MODE),
        ..Default::default()
    };
    let mut power = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut power) }.is_ok() {
        // 0 = on battery; 1 = plugged in; 255 = unknown (a desktop PC).
        out.on_battery = power.ACLineStatus == 0;
    }
    out.covered = covered_monitors();
    if let Some((window, screen)) = foreground() {
        out.fullscreen.push(to_rect(window));
        // A Direct3D game in exclusive fullscreen owns its monitor whatever
        // its window says.
        if state == Some(QUNS_RUNNING_D3D_FULL_SCREEN) {
            out.fullscreen.push(to_rect(screen));
        }
    }
    out
}

pub fn follow_session(app: &AppHandle, on: bool) {
    super::session::follow(app, on);
}
