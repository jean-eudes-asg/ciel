// Ciel's canvas: what « OK Ciel »'s brain draws while it talks — a diagram,
// a chart, a table, a plan, a small live page. Small in the island, big on
// the Canvas page of the Ciel window (src/island/canvas.ts, src/hub/pages/canvas.ts).
//
// The brain's `canvas` tool gives a title and either Markdown, which the
// island renders itself with its own safe renderer, or a whole HTML page,
// which may hold scripts. Such a page is never put in Ciel's own document:
// it is served here, under its own scheme, and shown in a frame sealed off
// from everything else:
//   * the frame is sandboxed (scripts only: no same origin, no popups, no
//     forms, no navigating Ciel, no dialogs, no camera or microphone);
//   * this response's security policy lets the page reach nothing — no
//     network at all, so nothing can be fetched or sent, and no request to
//     Ciel's own commands, which go through a fetch too;
//   * a first script of Ciel's own takes away what the webview injects to
//     reach Ciel, before the page's scripts run.
// The last ten drawings are kept in canvas.json in Ciel's own folder, a plain
// file the user can delete; nothing leaves the computer.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Emitter};

/// The scheme the pages are served under (http://ciel-canvas.localhost on
/// Windows, ciel-canvas://localhost elsewhere).
pub const SCHEME: &str = "ciel-canvas";
/// Drawings kept.
const KEEP: usize = 10;
/// A drawing, at most.
const MAX_BODY: usize = 256 * 1024;
const MAX_TITLE: usize = 120;

/// What a page may do: run its own scripts and styles, show its own images,
/// and nothing else. No network, no frames, no forms, no workers.
const POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; \
font-src data:; media-src data: blob:; connect-src 'none'; frame-src 'none'; child-src 'none'; worker-src 'none'; \
object-src 'none'; form-action 'none'; base-uri 'none'; sandbox allow-scripts";

/// Run before the page's own scripts: what the webview puts in a frame to
/// reach Ciel is taken away, and can't be put back.
const SEAL: &str = r#"<script>(function(){var gone=["__TAURI_INTERNALS__","__TAURI__","__TAURI_IPC__","__TAURI_METADATA__","__TAURI_POST_MESSAGE__","__TAURI_INVOKE_KEY__","ipc","chrome"];for(var i=0;i<gone.length;i++){var k=gone[i];try{delete window[k];}catch(e){}try{Object.defineProperty(window,k,{value:undefined,writable:false,configurable:false});}catch(e){}}})();</script>"#;

/// Dark by default, as the island is; the page may style itself over it.
const BASE_STYLE: &str = "<style>html,body{margin:0;background:#000;color:#ECECEC;font:14px/1.45 system-ui,-apple-system,'Segoe UI',sans-serif}</style>";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drawing {
    pub id: String,
    pub title: String,
    /// "html" or "markdown".
    pub kind: String,
    pub body: String,
    /// Seconds since 1970.
    pub at: i64,
}

/// A drawing without its body: what the lists show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub at: i64,
}

impl Drawing {
    fn meta(&self) -> Meta {
        Meta { id: self.id.clone(), title: self.title.clone(), kind: self.kind.clone(), at: self.at }
    }
}

static STORE: Mutex<Option<Vec<Drawing>>> = Mutex::new(None);
static COUNTER: AtomicU64 = AtomicU64::new(0);

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn file() -> std::path::PathBuf {
    crate::platform::local_dir().join("canvas.json")
}

/// The drawings, oldest first, read once.
fn with<T>(f: impl FnOnce(&mut Vec<Drawing>) -> T) -> T {
    let mut store = STORE.lock().unwrap();
    let list = store.get_or_insert_with(|| {
        std::fs::read(file()).ok().and_then(|b| serde_json::from_slice::<Vec<Drawing>>(&b).ok()).unwrap_or_default()
    });
    f(list)
}

fn save(list: &[Drawing]) {
    let path = file();
    if let Some(dir) = path.parent() {
        let _ = crate::platform::ensure_private_dir(dir);
    }
    if let Ok(bytes) = serde_json::to_vec(list) {
        if let Err(err) = crate::wallpaper::write_atomic(&path, &bytes) {
            crate::log::line(format!("canvas: couldn't be kept: {err}"));
        }
    }
}

fn clip(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// An id is letters and digits only: it goes in a path.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// A drawing from the brain's input, or what to tell it instead.
fn drawing(input: &Value, at: i64, n: u64) -> Result<Drawing, String> {
    let title = clip(input["title"].as_str().unwrap_or(""), MAX_TITLE);
    let kind = match input["kind"].as_str().unwrap_or("html") {
        "html" => "html",
        "markdown" => "markdown",
        _ => return Err("The kinds are html and markdown.".into()),
    };
    let body = input["content"].as_str().unwrap_or("");
    if body.trim().is_empty() {
        return Err("Nothing to draw: give the content.".into());
    }
    if body.len() > MAX_BODY {
        return Err(format!("Too big to draw: keep it under {} KB.", MAX_BODY / 1024));
    }
    let title = if title.is_empty() { crate::i18n::t("Drawing") } else { title };
    Ok(Drawing { id: format!("{at:x}{n:x}"), title, kind: kind.into(), body: body.to_string(), at })
}

/// The brain's `canvas` tool: what goes back to it, and that it drew.
pub fn tool(app: &AppHandle, input: &Value) -> (String, bool) {
    let d = match drawing(input, now(), COUNTER.fetch_add(1, Ordering::Relaxed)) {
        Ok(d) => d,
        Err(why) => return (why, false),
    };
    let meta = d.meta();
    with(|list| {
        list.push(d);
        let extra = list.len().saturating_sub(KEEP);
        list.drain(..extra);
        save(list);
    });
    // That something was drawn, never what.
    crate::log::line("canvas: one drawn");
    let _ = app.emit("canvas", json!(meta));
    (
        format!("Drawn: « {} ». It shows small in the island now, and big on the Canvas page of the Ciel window. Say in a few words what it shows; don't read it out.", meta.title),
        true,
    )
}

#[tauri::command]
pub fn canvas_list() -> Vec<Meta> {
    with(|list| list.iter().rev().map(Drawing::meta).collect())
}

#[tauri::command]
pub fn canvas_get(id: String) -> Option<Drawing> {
    with(|list| list.iter().find(|d| d.id == id).cloned())
}

#[tauri::command]
pub fn canvas_remove(id: String) {
    with(|list| {
        let before = list.len();
        list.retain(|d| d.id != id);
        if list.len() != before {
            save(list);
        }
    });
}

#[tauri::command]
pub fn canvas_clear() {
    with(|list| {
        list.clear();
        save(list);
    });
}

/// The page shown for an HTML drawing: Ciel's seal and base style first, then
/// the page as the brain wrote it.
fn page(d: &Drawing) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">{SEAL}{BASE_STYLE}</head><body>{}</body></html>",
        d.body
    )
}

fn respond(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_SECURITY_POLICY, POLICY)
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

/// What the scheme serves: `/<id>` of an HTML drawing, sealed; nothing else.
pub fn protocol(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let id = request.uri().path().trim_start_matches('/');
    if request.method() != tauri::http::Method::GET || !valid_id(id) {
        return respond(StatusCode::NOT_FOUND, Vec::new());
    }
    match canvas_get(id.to_string()).filter(|d| d.kind == "html") {
        Some(d) => respond(StatusCode::OK, page(&d).into_bytes()),
        None => respond(StatusCode::NOT_FOUND, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_needs_content_a_known_kind_and_a_reasonable_size() {
        let ok = drawing(&json!({ "title": "  My week  ", "kind": "markdown", "content": "# Hi" }), 0x10, 2).unwrap();
        assert_eq!((ok.title.as_str(), ok.kind.as_str(), ok.id.as_str()), ("My week", "markdown", "102"));
        assert_eq!(drawing(&json!({ "content": "<svg/>" }), 1, 0).unwrap().kind, "html", "html when not said");
        assert!(drawing(&json!({ "kind": "pdf", "content": "x" }), 1, 0).is_err());
        assert!(drawing(&json!({ "kind": "html", "content": "  " }), 1, 0).is_err());
        assert!(drawing(&json!({ "content": "x".repeat(MAX_BODY + 1) }), 1, 0).is_err());
        let long = drawing(&json!({ "title": "t".repeat(500), "content": "x" }), 1, 0).unwrap();
        assert_eq!(long.title.chars().count(), MAX_TITLE);
    }

    #[test]
    fn only_a_plain_id_is_ever_served() {
        for bad in ["", "../canvas.json", "a/b", "a%2Fb", "abc.html", &"a".repeat(40)] {
            assert!(!valid_id(bad), "{bad}");
        }
        assert!(valid_id("19a2b3c40"));
    }

    #[test]
    fn a_page_comes_sealed_and_reaches_nothing() {
        let d = Drawing { id: "1".into(), title: "t".into(), kind: "html".into(), body: "<script>fetch('http://x')</script>".into(), at: 0 };
        let html = page(&d);
        // Ciel's seal runs before anything of the page's.
        assert!(html.find("__TAURI_INTERNALS__").unwrap() < html.find("fetch(").unwrap());
        let r = respond(StatusCode::OK, html.into_bytes());
        let policy = r.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        for must in ["default-src 'none'", "connect-src 'none'", "frame-src 'none'", "form-action 'none'", "sandbox allow-scripts"] {
            assert!(policy.contains(must), "{must}");
        }
        assert!(!policy.contains("allow-same-origin") && !policy.contains("http"));
    }
}
