// The Games page (src/hub/pages/games.ts): Ciel's brain gives the rival
// (src/play/cast.ts) a line now and then while you play chess or draughts
// with him, and names the constellations you draw. The moves are never the
// brain's (the page's own players pick them); it gets the game as text and
// answers with one sentence. Claude with the user's key or the chat's local
// model, as for the clean-up; with neither, the games go on without a word.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::ai::{self, Engine};

const MODEL: &str = "claude-haiku-4-5";
const TIMEOUT: Duration = Duration::from_secs(10);
const LOCAL_TIMEOUT: Duration = Duration::from_secs(45);

fn system() -> String {
    format!(
        "You are one of the clouds of Ciel's family (Ciel is a small cloud who lives at the top of the user's screen); \
the facts say which one and how you talk. You are playing a game with the user (the facts say which) so they can relax after long hours of work. \
Say one short line, at most 20 words, in your own character: never condescending. \
Talk to the user as \"you\". Use only the facts given: never invent a move, a threat or a variation that is not in them. \
No emoji, no quotation marks, no markdown. Write in the language whose code is {}.",
        crate::i18n::current()
    )
}

/// One line from Ciel about the game, or None (no brain, no answer in time).
#[tauri::command]
pub async fn game_line(app: tauri::AppHandle, facts: String) -> Option<String> {
    let engine = ai::current(&app)?;
    let system = system();
    let reply = match &engine {
        Engine::Claude => crate::claude::complete(MODEL, &system, &facts, 120, TIMEOUT).await,
        Engine::Local(local) => {
            let how = ai::Ask { max_tokens: 120, json: false, context: None, timeout: LOCAL_TIMEOUT };
            ai::complete(local, &system, &facts, how).await
        }
    };
    match reply {
        Ok(line) => Some(ai::visible_text(&line).trim().trim_matches('"').to_string()).filter(|l| !l.is_empty()),
        Err(err) => {
            crate::log::line(format!("games: no line from the brain: {err}"));
            None
        }
    }
}

// ── The games' own window ─────────────────────────────────────────────────────
//
// The Games page taken out of the Ciel window (games.html), to play beside
// something else, as big as you like. Made hidden at launch and only shown and
// hidden after, like the Ciel window (lib.rs create_settings_window): on
// Windows a WebView2 window made later may come up blank. The page builds its
// game when shown and drops it when hidden, so a hidden window costs nothing.

pub const LABEL: &str = "games";

fn page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/games.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("games.html".into())
}

pub fn setup(app: &AppHandle) {
    let built = WebviewWindowBuilder::new(app, LABEL, page_url(app))
        .additional_browser_args(crate::BROWSER_ARGS)
        .title("Ciel")
        .inner_size(1060.0, 780.0)
        .min_inner_size(560.0, 480.0)
        .resizable(true)
        .background_color(tauri::window::Color(14, 15, 17, 255))
        .visible(false)
        .center()
        .build();
    match built {
        Ok(win) => {
            // Closing it hides it, or it could never come back.
            let app = app.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    show(&app, false);
                }
            });
        }
        Err(err) => crate::log::line(format!("games: no window: {err}")),
    }
}

fn show(app: &AppHandle, open: bool) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    if open {
        let _ = win.emit_to(LABEL, "games-shown", true);
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let _ = win.hide();
        let _ = win.set_always_on_top(false);
        let _ = win.emit_to(LABEL, "games-hidden", true);
    }
    // The Ciel window's Games page shows the game again, or says where it is.
    let _ = app.emit("games-window", open);
}

/// Takes the games out to their own window, or back into the Ciel window.
#[tauri::command]
pub fn games_window(app: AppHandle, open: bool) {
    show(&app, open);
    if !open {
        crate::show_hub(&app, Some("games"));
    }
}

#[tauri::command]
pub fn games_window_open(app: AppHandle) -> bool {
    app.get_webview_window(LABEL).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}

/// Keeps the games' window over the others, to play beside them.
#[tauri::command]
pub fn games_on_top(app: AppHandle, on: bool) {
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.set_always_on_top(on);
    }
}
