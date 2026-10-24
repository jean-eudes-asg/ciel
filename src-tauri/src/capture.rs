// Settings → General → Hide from screen sharing (Windows): every window of
// Ciel's stays on the screen but is left out of screen shares, recordings and
// screenshots, so the agents' sessions, approvals, mail and notes don't go out
// with a shared screen. Windows draws the window as before and gives whatever
// captures the screen what is behind it: Tauri's set_content_protected, which
// is SetWindowDisplayAffinity with WDA_EXCLUDEFROMCAPTURE. A courtesy, not a
// lock: Microsoft says it is no DRM, some capture paths through the GPU still
// see the window, and a camera sees everything.
//
// Linux has nothing of the kind (on X11 any app reads the screen, Wayland's
// portal hands over all of it): the setting isn't offered there and this does
// nothing.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::webview::{PageLoadEvent, PageLoadPayload};
#[cfg(windows)]
use tauri::Manager;
use tauri::{AppHandle, Runtime, Webview};

/// What the user picked; windows start out shown.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// The setting, given to every window open now. The ones opened later take it
/// in `page_load`.
pub fn sync(app: &AppHandle, hidden: bool) {
    if HIDDEN.swap(hidden, Ordering::Relaxed) == hidden {
        return;
    }
    #[cfg(windows)]
    for win in app.webview_windows().values() {
        if let Err(err) = win.set_content_protected(hidden) {
            crate::log::line(format!("hide from screen sharing: {}: {err}", win.label()));
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// Every page that starts loading, in any window (tauri::Builder::on_page_load):
/// a window made after `sync` (a wallpaper, Ciel on the desktop) is hidden
/// before it draws anything.
pub fn page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    if payload.event() != PageLoadEvent::Started || !HIDDEN.load(Ordering::Relaxed) {
        return;
    }
    #[cfg(windows)]
    if let Err(err) = webview.window().set_content_protected(true) {
        crate::log::line(format!("hide from screen sharing: {}: {err}", webview.label()));
    }
    #[cfg(not(windows))]
    let _ = webview;
}
