// Wallpapers on macOS: not yet. `supported` says so, so Settings offers
// nothing and none of the setters below is ever reached. Setting the desktop
// picture (NSWorkspace) and the live sky window behind the icons come later.

use tauri::{AppHandle, WebviewWindow};

use super::live::{Rect, Signals};
use super::{MonitorInfo, Screen, Supported};

const NOT_YET: &str = "wallpapers are not available on macOS yet";

pub async fn supported(_app: &AppHandle) -> Supported {
    Supported { desktop: false, lock_screen: false, live: false, lock_needs_extension: false }
}

pub async fn live_supported(_app: &AppHandle) -> bool {
    false
}

pub fn desktop_ready() -> bool {
    false
}

pub fn monitors(_screens: &[Screen]) -> Result<Vec<MonitorInfo>, String> {
    Err(NOT_YET.into())
}

pub fn set_desktop(_id: &str, _png: &[u8], _screens: &[Screen]) -> Result<(), String> {
    Err(NOT_YET.into())
}

pub fn set_lock_screen(_png: &[u8]) -> Result<(), String> {
    Err(NOT_YET.into())
}

/// Nothing was ever changed, so there is nothing to put back.
pub fn restore(_desktop: bool, _lock_screen: bool) -> Result<(), String> {
    Ok(())
}

pub fn place_live(_win: &WebviewWindow, _rect: Rect) -> Result<(), String> {
    Err(NOT_YET.into())
}

pub fn describe_live(_win: &WebviewWindow) -> String {
    String::new()
}

pub fn release_live(_win: &WebviewWindow) {}

pub fn after_live_removed() {}

pub fn live_attached(_win: &WebviewWindow) -> bool {
    false
}

pub fn signals(_monitors: &[Rect]) -> Signals {
    Signals::default()
}

pub fn follow_session(_app: &AppHandle, _on: bool) {}
