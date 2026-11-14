// macOS: the files, folders, links, relay socket and account name are real;
// the window functions are placeholders until the island gets its macOS
// window (a non-activating panel above the menu bar).
//
// Until then the island is an ordinary always-on-top window: no cursor poll,
// no input region (the whole window takes the mouse), and the desktop Ciel
// stays in the island, as on GNOME's Wayland (`DesktopMode::Off`).

use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tauri::{AppHandle, WebviewWindow};

use super::{home_dir, LocalTime};

/// File name of the Claude Code relay.
pub const HOOK_EXE: &str = "ciel-hook";

/// Environment variable holding the home directory.
pub const HOME_VAR: &str = "HOME";

// ── Files ─────────────────────────────────────────────────────────────────────

/// ~/Library/Preferences/ciel — preferences.
pub fn config_dir() -> PathBuf {
    home_dir().join("Library/Preferences/ciel")
}

/// ~/Library/Application Support/ciel — where ciel-hook, the inbox and the log
/// live. A stable path outside the app bundle, which may move or be replaced
/// by an update.
pub fn local_dir() -> PathBuf {
    home_dir().join("Library/Application Support/ciel")
}

/// Where a saved image goes, best first. The caller takes the first one that
/// exists.
pub fn picture_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    vec![home.join("Pictures"), home.join("Downloads"), home]
}

/// Nothing to set before the webview starts.
pub fn prepare_environment() {}

pub fn local_time() -> LocalTime {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        libc::localtime_r(&now, &mut tm);
    }
    LocalTime {
        year: (tm.tm_year + 1900) as u32,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
    }
}

/// Creates `dir` and closes it to other users, as on Linux.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// True when `dir` is a real directory (not a symlink), owned by us, with no
/// access for group or others.
fn is_private_dir(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(dir)
        .map(|m| {
            m.file_type().is_dir() && m.uid() == unsafe { libc::getuid() } && m.mode() & 0o077 == 0
        })
        .unwrap_or(false)
}

/// The per-user temporary folder (`/var/folders/…/T`), from
/// `confstr(_CS_DARWIN_USER_TEMP_DIR)` rather than `$TMPDIR`, so the app and
/// a hook started from any environment agree on it.
fn darwin_user_temp_dir() -> Option<PathBuf> {
    let mut buf = vec![0u8; 1024];
    let len = unsafe {
        libc::confstr(libc::_CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr() as *mut libc::c_char, buf.len())
    };
    // 0 is an error; a length past the buffer means it was cut short.
    if len == 0 || len > buf.len() {
        return None;
    }
    buf.truncate(len - 1);
    let dir = PathBuf::from(std::ffi::OsStr::from_bytes(&buf));
    dir.is_absolute().then_some(dir)
}

/// Where ciel-hook finds us: `ciel.sock` in the per-user temporary folder. A
/// folder that is not ours and private means no relay at all. Must match
/// `socket_path()` in hook/src/unix.rs exactly.
pub fn relay_socket_path() -> Option<PathBuf> {
    let dir = darwin_user_temp_dir()?;
    is_private_dir(&dir).then(|| dir.join("ciel.sock"))
}

// ── Who we are ────────────────────────────────────────────────────────────────

/// The account's full name, from the passwd entry (macOS keeps it whole in the
/// GECOS field), or nothing when it is unset.
pub fn user_full_name() -> Option<String> {
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let mut buf = vec![0 as libc::c_char; 16 * 1024];
        let rc = libc::getpwuid_r(libc::getuid(), &mut pwd, buf.as_mut_ptr(), buf.len(), &mut result);
        if rc != 0 || result.is_null() || pwd.pw_gecos.is_null() {
            return None;
        }
        let gecos = std::ffi::CStr::from_ptr(pwd.pw_gecos).to_string_lossy().into_owned();
        let name = gecos.split(',').next().unwrap_or("").trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

// ── Processes ─────────────────────────────────────────────────────────────────

/// Nothing to hide: a spawned process only gets a terminal if it asks for one.
pub fn no_console(cmd: &mut Command) -> &mut Command {
    cmd
}

pub fn open_url(url: &str) {
    let _ = Command::new("/usr/bin/open").arg(url).spawn();
}

pub fn reveal_folder(path: &str) {
    let _ = Command::new("/usr/bin/open").arg(path).spawn();
}

/// Our own `which`: the first executable file named `stem` on $PATH.
pub fn find_on_path(stem: &str) -> Option<PathBuf> {
    let dirs = std::env::var_os("PATH")?;
    std::env::split_paths(&dirs).map(|dir| dir.join(stem)).find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

// ── Session windows ("Open terminal") ─────────────────────────────────────────

/// Not yet: opening the Claude desktop app comes with the macOS window work.
pub fn open_claude_desktop() -> bool {
    false
}

/// Where the Codex CLI may be, best first: $PATH, then the usual per-user and
/// Homebrew install folders, which an app launched from Finder doesn't have on
/// its $PATH, and nvm with its newest Node first.
pub fn codex_candidates() -> Vec<PathBuf> {
    let home = home_dir();
    let mut out: Vec<PathBuf> = find_on_path("codex").into_iter().collect();
    for dir in [".local/bin", ".npm-global/bin", ".volta/bin", ".bun/bin", "Library/pnpm"] {
        out.push(home.join(dir).join("codex"));
    }
    out.push(PathBuf::from("/opt/homebrew/bin/codex"));
    out.push(PathBuf::from("/usr/local/bin/codex"));
    let nvm = home.join(".nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(&nvm) {
        let mut versions: Vec<String> =
            entries.filter_map(|e| e.ok()?.file_name().into_string().ok()).collect();
        versions.sort_by(|a, b| crate::codex_plan::compare_versions(b, a));
        out.extend(versions.iter().map(|v| nvm.join(v).join("bin/codex")));
    }
    out.retain(|p| is_executable(p));
    out
}

/// No process tree walk yet: "Open terminal" finds nothing to raise.
pub fn process_ancestors(_pid: u32) -> Vec<u32> {
    Vec::new()
}

pub fn window_owners(ancestors: &[u32]) -> Vec<u32> {
    ancestors.to_vec()
}

pub fn focus_session_window(_owners: &[u32], _folder: &str) -> bool {
    false
}

// ── Cursor ────────────────────────────────────────────────────────────────────

/// Nothing polls the cursor yet: the page reports it over the island.
pub const CURSOR_POLL: bool = false;

pub fn cursor_physical() -> Option<(f64, f64)> {
    None
}

pub fn left_button_down() -> bool {
    false
}

pub fn any_button_down() -> bool {
    false
}

pub fn sees_clicks_outside() -> bool {
    false
}

// ── Island window ─────────────────────────────────────────────────────────────

pub fn unblock_webview_drops(_app: &AppHandle) {}

/// Placeholder: the island stays the ordinary always-on-top window Tauri makes.
pub fn make_non_activating(_win: &WebviewWindow) {}

pub fn set_pointer_watch(_active: bool) {}

/// A regular window, which `set_position` already places.
pub fn pin_to_monitor(_win: &WebviewWindow, _x: i32, _y: i32) {}

pub fn set_activating(_win: &WebviewWindow, _activating: bool) {}

pub fn give_focus_back() -> bool {
    false
}

/// Placeholder: the whole window takes the mouse.
pub fn set_input_region(_win: &WebviewWindow, _rect: Option<(f64, f64, f64, f64)>) {}

pub fn global_shortcuts_blocked() -> Option<&'static str> {
    None
}

pub fn ctrl_alt_types(_vk: u16, _shift: bool) -> Option<String> {
    None
}

// ── Desktop Ciel and the pill ─────────────────────────────────────────────────

/// Ciel stays in the island until the macOS window work lands.
pub fn desktop_mode() -> super::DesktopMode {
    super::DesktopMode::Off
}

pub fn prepare_desktop_window(_win: &WebviewWindow, _mode: super::DesktopMode) -> bool {
    false
}

pub fn set_desktop_shape(_win: &WebviewWindow, _shape: super::MouseShape) {}

pub fn set_layer_margins(_win: &WebviewWindow, _x: f64, _y: f64) {}

pub fn set_layer_overlay(_win: &WebviewWindow, _on: bool) {}

pub fn layer_display(_island: &WebviewWindow, _cloud: &WebviewWindow) -> Option<(f64, f64)> {
    None
}

pub fn prepare_pill_window(_win: &WebviewWindow, _mode: super::DesktopMode) -> bool {
    false
}

pub fn set_pill_edge(_win: &WebviewWindow, _edge: &str) {}

pub fn set_pill_input(_win: &WebviewWindow, _rects: Vec<(f64, f64, f64, f64)>) {}
