// Settings → General → Hide from screen sharing (Windows): every window of
// Ciel's stays on the screen but is left out of screen shares, recordings and
// screenshots, so the agents' sessions, approvals, mail and notes don't go out
// with a shared screen. Windows draws the window as before and gives whatever
// captures the screen what is behind it: Tauri's set_content_protected, which
// is SetWindowDisplayAffinity with WDA_EXCLUDEFROMCAPTURE. A courtesy, not a
// lock: Microsoft says it is no DRM, some capture paths through the GPU still
// see the window, and a camera sees everything.
//
// While an answer of the Notetaker's is on screen (notes/mod.rs), the setting
// doesn't apply: Ciel is in the share like the rest of the screen, so the
// others see what it suggests, unless Show answers in screen shares is off.
// The rest of the call, it is left out as set.
//
// Linux has nothing of the kind (on X11 any app reads the screen, Wayland's
// portal hands over all of it): the setting isn't offered there and this does
// nothing.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::webview::{PageLoadEvent, PageLoadPayload};
#[cfg(windows)]
use tauri::Manager;
use tauri::{AppHandle, Runtime, Webview};

/// What the user picked.
static SETTING: AtomicBool = AtomicBool::new(false);
/// Until then (Unix ms) an answer of the Notetaker's is on screen, and Ciel
/// is in screen shares with it.
static ANSWER_UNTIL: AtomicU64 = AtomicU64::new(0);
/// What the windows have now; they start out shown.
static HIDDEN: AtomicBool = AtomicBool::new(false);

/// The setting, given to every window open now. The ones opened later take it
/// in `page_load`.
pub fn sync(app: &AppHandle, hidden: bool) {
    SETTING.store(hidden, Ordering::Relaxed);
    apply(app);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// An answer comes on screen for `for_ms`: Ciel is in screen shares until it
/// goes, then left out again (unless a newer answer took its place).
pub fn answer_shown(app: &AppHandle, for_ms: u64) {
    let until = now_ms() + for_ms;
    ANSWER_UNTIL.store(until, Ordering::Relaxed);
    apply(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(for_ms)).await;
        if ANSWER_UNTIL.compare_exchange(until, 0, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            apply(&app);
        }
    });
}

/// The answer was dismissed, or the call is over.
pub fn answer_gone(app: &AppHandle) {
    ANSWER_UNTIL.store(0, Ordering::Relaxed);
    apply(app);
}

/// Left out of shares: what the user picked, except while an answer is on screen.
fn hidden_now(setting: bool, answer_until: u64, now: u64) -> bool {
    setting && now >= answer_until
}

fn apply(app: &AppHandle) {
    let hidden = hidden_now(SETTING.load(Ordering::Relaxed), ANSWER_UNTIL.load(Ordering::Relaxed), now_ms());
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

#[cfg(test)]
mod tests {
    use super::hidden_now;

    #[test]
    fn left_out_of_shares_as_set_except_while_an_answer_is_on_screen() {
        assert!(hidden_now(true, 0, 1_000));
        assert!(!hidden_now(true, 2_000, 1_000), "an answer on screen: in the share");
        assert!(hidden_now(true, 2_000, 2_000), "its time is up: left out again");
        assert!(!hidden_now(false, 0, 1_000));
    }
}
