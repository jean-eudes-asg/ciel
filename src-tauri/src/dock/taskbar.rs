// The Windows taskbar, hidden while the dock takes its place (Settings →
// Island → Dock → Hide the Windows taskbar), and always put back as it was:
// when that is turned off or the dock goes, when Ciel quits, if Ciel stops on
// an error (a panic hook), and at the next launch should it have been stopped
// any other way: a file says what to put back, written before anything is
// changed. Windows' own files are never touched.
//
// Hidden as other docks hide it: set to hide itself, so it keeps no room at the
// bottom of the screen (the other screens' taskbars do the same, and come up
// when pointed at), and the main screen's window hidden. Its auto-hide is put
// back to what it was.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, ShowWindow, SW_HIDE, SW_SHOWNA,
};

/// What to put back, in Ciel's settings folder.
const FILE: &str = "taskbar.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    /// The taskbar hid itself before Ciel hid it.
    auto_hide: bool,
}

static HIDDEN: AtomicBool = AtomicBool::new(false);
/// Shown for a moment from the dock: it is left alone until it goes again.
static REVEALED: AtomicBool = AtomicBool::new(false);
/// Ciel is quitting: once put back, nothing hides it again. (Putting it back
/// changes the work area, and the dock's watcher, told so, would otherwise
/// hide it once more on its way out.)
static QUITTING: AtomicBool = AtomicBool::new(false);

fn file() -> PathBuf {
    crate::settings::config_dir().join(FILE)
}

fn tray() -> Option<HWND> {
    unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.ok().filter(|h| !h.is_invalid())
}

fn data(tray: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: tray, ..Default::default() }
}

fn state(tray: HWND) -> u32 {
    let mut abd = data(tray);
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut abd) as u32 }
}

fn set_state(tray: HWND, value: u32) {
    let mut abd = data(tray);
    abd.lParam = LPARAM(value as isize);
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut abd);
    }
}

fn read() -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(file()).ok()?).ok()
}

pub fn hidden() -> bool {
    HIDDEN.load(Ordering::Relaxed)
}

/// Hides the taskbar; false (and nothing changed) when there is none, or what
/// to put back could not be written first.
pub fn hide() -> bool {
    if QUITTING.load(Ordering::SeqCst) {
        return false;
    }
    let Some(tray) = tray() else { return false };
    if read().is_none() {
        let saved = Saved { auto_hide: state(tray) & ABS_AUTOHIDE != 0 };
        let written = crate::platform::ensure_private_dir(&crate::settings::config_dir())
            .and_then(|_| std::fs::write(file(), serde_json::to_vec(&saved).unwrap_or_default()));
        if let Err(err) = written {
            crate::log::line(format!("dock: the taskbar stays, nothing to put it back from: {err}"));
            return false;
        }
    }
    set_state(tray, state(tray) | ABS_AUTOHIDE);
    unsafe {
        let _ = ShowWindow(tray, SW_HIDE);
    }
    REVEALED.store(false, Ordering::Relaxed);
    if !HIDDEN.swap(true, Ordering::Relaxed) {
        crate::log::line("dock: the Windows taskbar is hidden");
    }
    true
}

/// Puts the taskbar back as it was, if Ciel hid it (now or before a crash).
pub fn restore() {
    let saved = read();
    let was_hidden = HIDDEN.swap(false, Ordering::Relaxed);
    REVEALED.store(false, Ordering::Relaxed);
    if saved.is_none() && !was_hidden {
        return;
    }
    // Explorer not running: the file stays, for the next launch.
    let Some(tray) = tray() else { return };
    unsafe {
        let _ = ShowWindow(tray, SW_SHOWNA);
    }
    if saved.as_ref().is_some_and(|s| !s.auto_hide) {
        set_state(tray, state(tray) & !ABS_AUTOHIDE);
    }
    let _ = std::fs::remove_file(file());
    crate::log::line("dock: the Windows taskbar is back");
}

/// Ciel quits: the taskbar back as it was, for good.
pub fn quit() {
    QUITTING.store(true, Ordering::SeqCst);
    restore();
}

/// Explorer showed it again by itself (it does, now and then): hidden again.
pub fn keep_hidden() {
    if !hidden() || REVEALED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(tray) = tray() {
        if unsafe { IsWindowVisible(tray) }.as_bool() {
            unsafe {
                let _ = ShowWindow(tray, SW_HIDE);
            }
        }
    }
}

/// Shows the hidden taskbar for a moment (it then hides itself, as set).
pub fn reveal() -> bool {
    if !hidden() {
        return false;
    }
    let Some(tray) = tray() else { return false };
    REVEALED.store(true, Ordering::Relaxed);
    unsafe {
        let _ = ShowWindow(tray, SW_SHOWNA);
    }
    true
}

/// Shown for a moment from the dock, now.
pub fn is_revealed() -> bool {
    REVEALED.load(Ordering::Relaxed)
}

/// Its window's place, and the process that owns it (Explorer), while revealed.
pub fn revealed() -> Option<(RECT, u32)> {
    if !REVEALED.load(Ordering::Relaxed) {
        return None;
    }
    let tray = tray()?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(tray, &mut rect) }.ok()?;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(tray, Some(&mut pid)) };
    Some((rect, pid))
}

/// The moment is over: hidden again.
pub fn end_reveal() {
    if REVEALED.swap(false, Ordering::Relaxed) {
        keep_hidden();
    }
}
