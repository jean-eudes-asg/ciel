// The clipboard history (Tools → Clipboard): what the user copies, kept in
// memory while Ciel runs, and the items they pin on disk.
//
// It is off until the user turns it on, and nothing is read while it is off.
// What is marked private is never kept: Windows' "Clipboard Viewer Ignore",
// ExcludeClipboardContentFromMonitorProcessing and CanIncludeInClipboardHistory
// set to 0 (what password managers set, and Flow dictation's own paste), and
// KDE's x-kde-passwordManagerHint on Linux. A one-time code (4 to 8 digits)
// goes after two minutes.
//
// Nothing runs on a timer. Windows tells a hidden window of ours about every
// change (AddClipboardFormatListener); X11 sends XFixes selection events to a
// window of ours; a Wayland session without XWayland has `wl-paste --watch`,
// when it is installed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

const EVENT: &str = "clipboard";
/// Items kept, pins aside.
const MAX_ITEMS: usize = 60;
/// Longer text is kept cut to this.
const MAX_TEXT: usize = 200 * 1024;
/// Pictures kept in full (to copy back), the newest; older ones keep their thumbnail only.
const MAX_IMAGES: usize = 12;
/// A picture bigger than this many pixels is not kept (a 8K screenshot).
const MAX_PIXELS: u64 = 40_000_000;
const THUMB: u32 = 240;
/// A one-time code is forgotten after this long.
const CODE_TTL_MS: f64 = 120_000.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Text,
    Image,
    Files,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: u64,
    pub kind: Kind,
    /// The text; the paths, one a line, for files; "" for a picture.
    pub text: String,
    /// A picture's thumbnail, as a data URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// Unix ms.
    pub at: f64,
    #[serde(default)]
    pub pinned: bool,
    /// The app it was copied from, when known ("Chrome", "Word"…).
    #[serde(default)]
    pub app: String,
    /// Unix ms: a one-time code goes then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<f64>,
    /// What tells two copies apart (text, paths, or a picture's pixels).
    #[serde(skip)]
    hash: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClipState {
    pub on: bool,
    /// Whether this session can see the copies at all (a Wayland session without wl-paste can't).
    pub watching: bool,
    pub items: Vec<Clip>,
}

struct History {
    on: bool,
    watching: bool,
    items: Vec<Clip>,
    /// The full pictures, by item id, as PNG.
    images: HashMap<u64, Vec<u8>>,
    pins_loaded: bool,
}

static HISTORY: LazyLock<Mutex<History>> = LazyLock::new(|| {
    Mutex::new(History { on: false, watching: false, items: Vec::new(), images: HashMap::new(), pins_loaded: false })
});
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static APP: OnceLock<AppHandle> = OnceLock::new();
/// One clipboard for copying back: on X11 what Ciel copies is served only while it lives.
static BOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    // FNV-1a: telling copies apart, nothing more.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn pins_dir() -> PathBuf {
    crate::settings::local_dir().join("clipboard")
}

/// A one-time code: 4 to 8 digits, maybe split by one space or dash ("123 456").
pub fn is_one_time_code(text: &str) -> bool {
    let t = text.trim();
    let digits: String = t.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    let seps = t.chars().filter(|c| matches!(c, ' ' | '-')).count();
    (4..=8).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()) && seps <= 1 && !t.starts_with(['-', ' '])
}

fn clip_text(s: &str) -> String {
    if s.len() <= MAX_TEXT {
        return s.to_string();
    }
    let mut end = MAX_TEXT;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// What was just copied, read from the clipboard.
enum Copied {
    Text(String),
    Files(Vec<String>),
    Image { width: u32, height: u32, rgba: Vec<u8> },
}

fn read_clipboard() -> Option<Copied> {
    let mut board = arboard::Clipboard::new().ok()?;
    if let Ok(files) = board.get().file_list() {
        let files: Vec<String> = files.iter().map(|p| p.to_string_lossy().to_string()).filter(|p| !p.is_empty()).collect();
        if !files.is_empty() {
            return Some(Copied::Files(files));
        }
    }
    if let Ok(text) = board.get_text() {
        if !text.trim().is_empty() {
            return Some(Copied::Text(text));
        }
    }
    if let Ok(img) = board.get_image() {
        if img.width > 0 && img.height > 0 && (img.width as u64) * (img.height as u64) <= MAX_PIXELS {
            return Some(Copied::Image { width: img.width as u32, height: img.height as u32, rgba: img.bytes.into_owned() });
        }
    }
    None
}

fn png(width: u32, height: u32, rgba: Vec<u8>) -> Option<(Vec<u8>, String)> {
    use base64::Engine;
    let img = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut full = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut full), image::ImageFormat::Png).ok()?;
    let scale = (THUMB as f64 / width.max(height) as f64).min(1.0);
    let (tw, th) = (((width as f64 * scale).round() as u32).max(1), ((height as f64 * scale).round() as u32).max(1));
    let small = image::imageops::thumbnail(&img, tw, th);
    let mut thumb = Vec::new();
    small.write_to(&mut std::io::Cursor::new(&mut thumb), image::ImageFormat::Png).ok()?;
    Some((full, format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(thumb))))
}

/// The clipboard changed: what is on it now joins the history.
fn changed() {
    if !HISTORY.lock().unwrap().on {
        return;
    }
    if platform::private_content() {
        return;
    }
    let app_name = platform::owner_app();
    let Some(copied) = read_clipboard() else { return };
    let at = now_ms();
    let mut item = Clip {
        id: 0,
        kind: Kind::Text,
        text: String::new(),
        thumb: None,
        width: 0,
        height: 0,
        at,
        pinned: false,
        app: app_name,
        expires: None,
        hash: 0,
    };
    let mut full_png = None;
    match copied {
        Copied::Text(text) => {
            item.hash = hash_bytes(text.as_bytes());
            if is_one_time_code(&text) {
                item.expires = Some(at + CODE_TTL_MS);
            }
            item.text = clip_text(&text);
        }
        Copied::Files(files) => {
            item.kind = Kind::Files;
            item.text = files.join("\n");
            item.hash = hash_bytes(item.text.as_bytes());
        }
        Copied::Image { width, height, rgba } => {
            item.kind = Kind::Image;
            item.hash = hash_bytes(&rgba) ^ ((width as u64) << 32 | height as u64);
            item.width = width;
            item.height = height;
            // Encoding a big screenshot takes a moment: outside the lock.
            let Some((full, thumb)) = png(width, height, rgba) else { return };
            item.thumb = Some(thumb);
            full_png = Some(full);
        }
    }
    let expires = item.expires;
    {
        let mut h = HISTORY.lock().unwrap();
        // The same again (copied twice, or copied back from here): it moves to the top.
        if let Some(i) = h.items.iter().position(|c| c.hash == item.hash && c.kind == item.kind) {
            let mut old = h.items.remove(i);
            old.at = at;
            if !item.app.is_empty() {
                old.app = item.app.clone();
            }
            h.items.insert(0, old);
        } else {
            item.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            if let Some(full) = full_png {
                h.images.insert(item.id, full);
            }
            let id = item.id;
            h.items.insert(0, item);
            trim(&mut h);
            if expires.is_some() {
                forget_later(id);
            }
        }
    }
    emit();
}

/// Keeps MAX_ITEMS (pins aside) and MAX_IMAGES full pictures.
fn trim(h: &mut History) {
    let mut kept = 0;
    let mut dropped = Vec::new();
    h.items.retain(|c| {
        if c.pinned {
            return true;
        }
        kept += 1;
        if kept > MAX_ITEMS {
            dropped.push(c.id);
            false
        } else {
            true
        }
    });
    for id in dropped {
        h.images.remove(&id);
    }
    let pictures: Vec<u64> = h.items.iter().filter(|c| c.kind == Kind::Image && !c.pinned).map(|c| c.id).collect();
    for id in pictures.into_iter().skip(MAX_IMAGES) {
        h.images.remove(&id);
    }
}

fn forget_later(id: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(CODE_TTL_MS as u64 + 200)).await;
        let gone = {
            let mut h = HISTORY.lock().unwrap();
            let before = h.items.len();
            h.items.retain(|c| c.id != id || c.pinned);
            before != h.items.len()
        };
        if gone {
            emit();
        }
    });
}

fn state() -> ClipState {
    let h = HISTORY.lock().unwrap();
    ClipState { on: h.on, watching: h.watching, items: h.items.clone() }
}

fn emit() {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, state());
    }
}

// ── Pins, on disk ─────────────────────────────────────────────────────────────
//
// pins.json holds the pinned items; a pinned picture is a PNG beside it, named
// by its hash. Only what the user pinned is ever written.

/// A pin as written: the item, and its hash (which names its picture).
#[derive(Serialize, Deserialize)]
struct SavedPin {
    #[serde(flatten)]
    clip: Clip,
    #[serde(default)]
    hash: u64,
}

fn load_pins(h: &mut History) {
    if h.pins_loaded {
        return;
    }
    h.pins_loaded = true;
    let Ok(bytes) = std::fs::read(pins_dir().join("pins.json")) else { return };
    let Ok(pins) = serde_json::from_slice::<Vec<SavedPin>>(&bytes) else { return };
    for SavedPin { clip: mut pin, hash } in pins {
        pin.id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        pin.pinned = true;
        pin.expires = None;
        pin.hash = hash;
        if pin.kind == Kind::Image {
            match std::fs::read(pins_dir().join(format!("{hash}.png"))) {
                Ok(full) => {
                    h.images.insert(pin.id, full);
                }
                // Its picture is gone: nothing to copy back.
                Err(_) => continue,
            }
        } else {
            pin.hash = hash_bytes(pin.text.as_bytes());
        }
        h.items.push(pin);
    }
}

fn save_pins(h: &History) {
    let dir = pins_dir();
    if crate::platform::ensure_private_dir(&dir).is_err() {
        return;
    }
    let pins: Vec<SavedPin> = h.items.iter().filter(|c| c.pinned).map(|c| SavedPin { clip: c.clone(), hash: c.hash }).collect();
    let mut keep: Vec<String> = vec!["pins.json".into()];
    for pin in &pins {
        if pin.clip.kind == Kind::Image {
            let name = format!("{}.png", pin.hash);
            if let Some(full) = h.images.get(&pin.clip.id) {
                let _ = std::fs::write(dir.join(&name), full);
            }
            keep.push(name);
        }
    }
    if let Ok(json) = serde_json::to_vec_pretty(&pins) {
        let _ = std::fs::write(dir.join("pins.json"), json);
    }
    // A picture no longer pinned goes from the disk too.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !keep.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

// ── On and off ────────────────────────────────────────────────────────────────

/// Settings were saved, or Ciel started: the watch follows the setting.
pub fn sync(app: &AppHandle, on: bool) {
    let _ = APP.set(app.clone());
    let changed_state = {
        let mut h = HISTORY.lock().unwrap();
        let was = h.on;
        h.on = on;
        if on {
            load_pins(&mut h);
        } else {
            // Turned off: what was copied goes; the pins stay on disk.
            h.items.retain(|c| c.pinned);
            let ids: Vec<u64> = h.items.iter().map(|c| c.id).collect();
            h.images.retain(|id, _| ids.contains(id));
        }
        was != on
    };
    if on {
        let watching = platform::start(changed);
        HISTORY.lock().unwrap().watching = watching;
    } else {
        platform::stop();
        HISTORY.lock().unwrap().watching = false;
    }
    if changed_state {
        emit();
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn clipboard_state() -> ClipState {
    state()
}

fn with_board<T>(f: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
    let mut slot = BOARD.lock().unwrap();
    if slot.is_none() {
        *slot = arboard::Clipboard::new().ok();
    }
    slot.as_mut().map(f)
}

/// Puts `item` back on the clipboard.
fn copy_back(item: &Clip, image: Option<&Vec<u8>>) -> bool {
    match item.kind {
        Kind::Text => with_board(|b| b.set_text(item.text.clone()).is_ok()).unwrap_or(false),
        Kind::Files => {
            let paths: Vec<PathBuf> = item.text.lines().map(PathBuf::from).collect();
            with_board(|b| b.set().file_list(&paths).is_ok()).unwrap_or(false)
        }
        Kind::Image => {
            let Some(bytes) = image else { return false };
            let Ok(img) = image::load_from_memory_with_format(bytes, image::ImageFormat::Png) else { return false };
            let rgba = img.to_rgba8();
            let data = arboard::ImageData {
                width: rgba.width() as usize,
                height: rgba.height() as usize,
                bytes: std::borrow::Cow::Owned(rgba.into_raw()),
            };
            with_board(|b| b.set_image(data).is_ok()).unwrap_or(false)
        }
    }
}

/// Ctrl+V in the app in front, once the clipboard has been announced.
fn paste_now() -> bool {
    std::thread::sleep(std::time::Duration::from_millis(60));
    match crate::flow::os::send_paste(false) {
        Ok(()) => true,
        Err(err) => {
            crate::log::line(format!("clipboard: paste keystroke failed: {err}"));
            false
        }
    }
}

/// Before a paste: the keyboard goes back to the app it was in, if the island had it.
fn hand_back(app: &AppHandle) {
    if let Some(win) = crate::island::window(app) {
        crate::platform::set_activating(&win, false);
    }
    if crate::platform::give_focus_back() {
        std::thread::sleep(std::time::Duration::from_millis(90));
    }
}

/// Copies an item back, and pastes it in the app in front with `paste`.
#[tauri::command]
pub async fn clipboard_use(app: AppHandle, id: u64, paste: bool) -> bool {
    let found = {
        let h = HISTORY.lock().unwrap();
        h.items.iter().find(|c| c.id == id).cloned().map(|c| {
            let image = h.images.get(&c.id).cloned();
            (c, image)
        })
    };
    let Some((item, image)) = found else { return false };
    tauri::async_runtime::spawn_blocking(move || {
        if paste {
            hand_back(&app);
        }
        copy_back(&item, image.as_ref()) && (!paste || paste_now())
    })
    .await
    .unwrap_or(false)
}

/// `text` on the clipboard: an answer of the answer key's (notify.rs), or
/// what `clipboard_put` is given.
pub fn copy_text(text: &str) -> bool {
    !text.is_empty() && with_board(|b| b.set_text(clip_text(text)).is_ok()).unwrap_or(false)
}

/// Copies `text` (an item turned into something else: plain, cleaned, upper case…).
#[tauri::command]
pub async fn clipboard_put(app: AppHandle, text: String, paste: bool) -> bool {
    if text.is_empty() {
        return false;
    }
    tauri::async_runtime::spawn_blocking(move || {
        if paste {
            hand_back(&app);
        }
        copy_text(&text) && (!paste || paste_now())
    })
    .await
    .unwrap_or(false)
}

#[tauri::command]
pub fn clipboard_pin(id: u64, pinned: bool) -> bool {
    let found = {
        let mut h = HISTORY.lock().unwrap();
        let Some(item) = h.items.iter_mut().find(|c| c.id == id) else { return false };
        item.pinned = pinned;
        item.expires = None;
        if !pinned {
            trim(&mut h);
        }
        save_pins(&h);
        true
    };
    emit();
    found
}

#[tauri::command]
pub fn clipboard_remove(id: u64) -> bool {
    let removed = {
        let mut h = HISTORY.lock().unwrap();
        let before = h.items.len();
        let was_pinned = h.items.iter().any(|c| c.id == id && c.pinned);
        h.items.retain(|c| c.id != id);
        h.images.remove(&id);
        if was_pinned {
            save_pins(&h);
        }
        before != h.items.len()
    };
    emit();
    removed
}

/// Forgets everything but the pins.
#[tauri::command]
pub fn clipboard_clear() {
    {
        let mut h = HISTORY.lock().unwrap();
        h.items.retain(|c| c.pinned);
        let ids: Vec<u64> = h.items.iter().map(|c| c.id).collect();
        h.images.retain(|id, _| ids.contains(id));
    }
    emit();
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod platform {
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::{Mutex, OnceLock};

    use windows::core::{w, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::DataExchange::{
        AddClipboardFormatListener, CloseClipboard, GetClipboardData, GetClipboardOwner, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowThreadProcessId,
        PostMessageW, PostQuitMessage, RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY, WNDCLASSW,
    };

    static WINDOW: AtomicIsize = AtomicIsize::new(0);
    static STARTING: Mutex<()> = Mutex::new(());
    static ON_CHANGE: OnceLock<fn()> = OnceLock::new();

    /// Starts the watch (once); true when it runs.
    pub fn start(on_change: fn()) -> bool {
        let _ = ON_CHANGE.set(on_change);
        let _guard = STARTING.lock().unwrap_or_else(|e| e.into_inner());
        if WINDOW.load(Ordering::SeqCst) != 0 {
            return true;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        if std::thread::Builder::new().name("ciel-clipboard".into()).spawn(move || run(tx)).is_err() {
            return false;
        }
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or(false)
    }

    pub fn stop() {
        let running = WINDOW.swap(0, Ordering::SeqCst);
        if running != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(running as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }

    fn run(ready: std::sync::mpsc::Sender<bool>) {
        unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = w!("CielClipboardWatch");
            let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
            // A second registration (turned on again) fails harmlessly.
            RegisterClassW(&wc);
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!("Ciel clipboard"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                None,
            ) {
                Ok(h) => h,
                Err(err) => {
                    crate::log::line(format!("clipboard: no window: {err}"));
                    let _ = ready.send(false);
                    return;
                }
            };
            if let Err(err) = AddClipboardFormatListener(hwnd) {
                crate::log::line(format!("clipboard: no listener: {err}"));
                let _ = DestroyWindow(hwnd);
                let _ = ready.send(false);
                return;
            }
            WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
            let _ = ready.send(true);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_CLIPBOARDUPDATE => {
                if let Some(f) = ON_CHANGE.get() {
                    f();
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                let _ = unsafe { DestroyWindow(hwnd) };
                return LRESULT(0);
            }
            WM_DESTROY => {
                unsafe {
                    let _ = RemoveClipboardFormatListener(hwnd);
                    PostQuitMessage(0);
                }
                return LRESULT(0);
            }
            _ => {}
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// What password managers (and Ciel's own dictation) mark as not for a history.
    pub fn private_content() -> bool {
        unsafe {
            for name in [w!("ExcludeClipboardContentFromMonitorProcessing"), w!("Clipboard Viewer Ignore")] {
                let format = RegisterClipboardFormatW(name);
                if format != 0 && IsClipboardFormatAvailable(format).is_ok() {
                    return true;
                }
            }
            let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
            if history == 0 || IsClipboardFormatAvailable(history).is_err() {
                return false;
            }
            // Its value: a DWORD, 0 for "keep it out". Held open the shortest time.
            if OpenClipboard(None).is_err() {
                // Can't tell: better not to keep it.
                return true;
            }
            let mut out = false;
            if let Ok(handle) = GetClipboardData(history) {
                let global = HGLOBAL(handle.0);
                let ptr = GlobalLock(global);
                if !ptr.is_null() {
                    out = *(ptr as *const u32) == 0;
                    let _ = GlobalUnlock(global);
                }
            }
            let _ = CloseClipboard();
            out
        }
    }

    /// The app that owns what was copied, by its program's name.
    pub fn owner_app() -> String {
        unsafe {
            let Ok(owner) = GetClipboardOwner() else { return String::new() };
            if owner.0.is_null() {
                return String::new();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(owner, Some(&mut pid));
            if pid == 0 || pid == std::process::id() {
                return String::new();
            }
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(process);
            if !ok {
                return String::new();
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]);
            super::app_name(&path)
        }
    }
}

// ── Linux ─────────────────────────────────────────────────────────────────────

/// macOS: no clipboard watch yet, so the history stays empty.
#[cfg(target_os = "macos")]
mod platform {
    /// False: nothing will tell us of a copy.
    pub fn start(_on_change: fn()) -> bool {
        false
    }
    pub fn stop() {}
    pub fn private_content() -> bool {
        false
    }
    pub fn owner_app() -> String {
        String::new()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};

    use x11rb::connection::Connection;
    use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
    use x11rb::protocol::xproto::{
        AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, WindowClass,
    };
    use x11rb::protocol::Event;
    use x11rb::CURRENT_TIME;

    static ON_CHANGE: OnceLock<fn()> = OnceLock::new();
    /// Bumped on every start and stop: an old watch that wakes up finds it changed and ends.
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    /// The X11 watch's window while it runs.
    static WINDOW: AtomicU32 = AtomicU32::new(0);
    static WATCHER: Mutex<Option<Child>> = Mutex::new(None);
    static RUNNING: Mutex<bool> = Mutex::new(false);

    fn x11() -> bool {
        std::env::var_os("DISPLAY").is_some() && !crate::platform::is_layer_surface()
    }

    pub fn start(on_change: fn()) -> bool {
        let _ = ON_CHANGE.set(on_change);
        let mut running = RUNNING.lock().unwrap();
        if *running {
            return true;
        }
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let ok = if x11() { start_x11(generation) } else { start_wl_paste() };
        *running = ok;
        ok
    }

    pub fn stop() {
        let mut running = RUNNING.lock().unwrap();
        GENERATION.fetch_add(1, Ordering::SeqCst);
        *running = false;
        let window = WINDOW.swap(0, Ordering::SeqCst);
        if window != 0 {
            wake(window);
        }
        if let Some(mut child) = WATCHER.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Unblocks the watch's wait_for_event, from a connection of its own.
    fn wake(window: u32) {
        if let Ok((conn, _)) = x11rb::connect(None) {
            let event = ClientMessageEvent::new(32, window, AtomEnum::NONE, [0u32; 5]);
            let _ = conn.send_event(false, window, EventMask::NO_EVENT, event);
            let _ = conn.flush();
        }
    }

    fn start_x11(generation: u64) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("ciel-clipboard".into()).spawn(move || {
            let result = watch_x11(generation, &tx);
            if let Err(err) = result {
                crate::log::line(format!("clipboard: X11 watch ended: {err}"));
                let _ = tx.send(false);
            }
        });
        spawned.is_ok() && rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or(false)
    }

    fn watch_x11(generation: u64, ready: &std::sync::mpsc::Sender<bool>) -> Result<(), Box<dyn std::error::Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        conn.xfixes_query_version(5, 0)?.reply()?;
        let window = conn.generate_id()?;
        conn.create_window(0, window, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new())?;
        let clipboard = conn.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
        conn.xfixes_select_selection_input(window, clipboard, SelectionEventMask::SET_SELECTION_OWNER)?;
        conn.flush()?;
        WINDOW.store(window, Ordering::SeqCst);
        let _ = ready.send(true);
        loop {
            let event = conn.wait_for_event()?;
            if GENERATION.load(Ordering::SeqCst) != generation {
                return Ok(());
            }
            if let Event::XfixesSelectionNotify(e) = event {
                if e.selection == clipboard && e.owner != window {
                    // The owner answers TARGETS; a password manager's hint keeps it out.
                    if !password_hint(&conn, window, clipboard) {
                        if let Some(f) = ON_CHANGE.get() {
                            f();
                        }
                    }
                }
            }
        }
    }

    /// KDE's x-kde-passwordManagerHint among the selection's targets.
    fn password_hint(conn: &impl Connection, window: u32, clipboard: u32) -> bool {
        let atom = |name: &[u8]| conn.intern_atom(false, name).ok().and_then(|c| c.reply().ok()).map(|r| r.atom);
        let (Some(targets), Some(prop), Some(hint)) =
            (atom(b"TARGETS"), atom(b"CIEL_CLIP_TARGETS"), atom(b"x-kde-passwordManagerHint"))
        else {
            return false;
        };
        if conn.convert_selection(window, clipboard, targets, prop, CURRENT_TIME).is_err() || conn.flush().is_err() {
            return false;
        }
        // The owner answers with a SelectionNotify; give it a moment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while std::time::Instant::now() < deadline {
            match conn.poll_for_event() {
                Ok(Some(Event::SelectionNotify(n))) if n.requestor == window => {
                    if n.property == x11rb::NONE {
                        return false;
                    }
                    let Ok(reply) = conn.get_property(true, window, prop, AtomEnum::ATOM, 0, 1024).map(|c| c.reply()) else {
                        return false;
                    };
                    return reply.ok().and_then(|r| r.value32().map(|v| v.collect::<Vec<u32>>())).is_some_and(|v| v.contains(&hint));
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(5)),
                Err(_) => return false,
            }
        }
        false
    }

    /// Wayland without XWayland: wl-paste tells us of each copy, when it is installed.
    fn start_wl_paste() -> bool {
        let child = Command::new("wl-paste")
            .args(["--watch", "echo", "."])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return false };
        let Some(out) = child.stdout.take() else { return false };
        let generation = GENERATION.load(Ordering::SeqCst);
        *WATCHER.lock().unwrap() = Some(child);
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                if line.is_err() || GENERATION.load(Ordering::SeqCst) != generation {
                    break;
                }
                if wl_password_hint() {
                    continue;
                }
                if let Some(f) = ON_CHANGE.get() {
                    f();
                }
            }
        });
        true
    }

    fn wl_password_hint() -> bool {
        Command::new("wl-paste")
            .arg("--list-types")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "x-kde-passwordManagerHint"))
            .unwrap_or(false)
    }

    /// The X11 path checks the hint before calling back; nothing more to see here.
    pub fn private_content() -> bool {
        false
    }

    pub fn owner_app() -> String {
        String::new()
    }
}

/// "C:\Program Files\Google\Chrome\Application\chrome.exe" → "Chrome".
#[cfg_attr(not(windows), allow(dead_code))]
pub fn app_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    let known = match stem.to_ascii_lowercase().as_str() {
        "chrome" => "Chrome",
        "msedge" => "Edge",
        "firefox" => "Firefox",
        "brave" => "Brave",
        "opera" => "Opera",
        "code" => "VS Code",
        "cursor" => "Cursor",
        "explorer" => "Explorer",
        "windowsterminal" => "Terminal",
        "notepad" => "Notepad",
        "winword" => "Word",
        "excel" => "Excel",
        "powerpnt" => "PowerPoint",
        "outlook" | "olk" => "Outlook",
        "ms-teams" | "teams" => "Teams",
        "slack" => "Slack",
        "discord" => "Discord",
        "spotify" => "Spotify",
        "obsidian" => "Obsidian",
        "notion" => "Notion",
        "snippingtool" | "screenclippinghost" => "Snipping Tool",
        "mspaint" => "Paint",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_time_codes_are_four_to_eight_digits() {
        for code in ["1234", "123456", "123 456", "123-456", "12345678", " 482910 "] {
            assert!(is_one_time_code(code), "{code}");
        }
        for not in ["123", "123456789", "12a456", "1 2 3 4", "-1234", "2026-11-07", "hello", "12.50"] {
            assert!(!is_one_time_code(not), "{not}");
        }
    }

    #[test]
    fn program_paths_become_app_names() {
        assert_eq!(app_name(r"C:\Program Files\Google\Chrome\Application\chrome.exe"), "Chrome");
        assert_eq!(app_name(r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"), "Word");
        assert_eq!(app_name("/usr/bin/gimp"), "Gimp");
        assert_eq!(app_name(r"C:\Tools\foo.exe"), "Foo");
        assert_eq!(app_name(""), "");
    }

    #[test]
    fn long_text_is_cut_on_a_character() {
        let long = "é".repeat(MAX_TEXT);
        let cut = clip_text(&long);
        assert!(cut.len() <= MAX_TEXT);
        assert!(cut.chars().all(|c| c == 'é'));
    }

    #[test]
    fn the_history_keeps_its_size_and_its_pins() {
        let mut h = History { on: true, watching: true, items: Vec::new(), images: HashMap::new(), pins_loaded: true };
        for i in 0..(MAX_ITEMS as u64 + 10) {
            h.items.push(Clip {
                id: i,
                kind: Kind::Text,
                text: i.to_string(),
                thumb: None,
                width: 0,
                height: 0,
                at: 0.0,
                pinned: i == MAX_ITEMS as u64 + 5,
                app: String::new(),
                expires: None,
                hash: i,
            });
        }
        trim(&mut h);
        assert_eq!(h.items.iter().filter(|c| !c.pinned).count(), MAX_ITEMS);
        assert!(h.items.iter().any(|c| c.pinned));
    }
}
