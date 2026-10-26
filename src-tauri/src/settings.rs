// Preferences, stored as plain JSON in settings.json under platform::config_dir().
// No secret ever lands here — API keys live in the OS keychain (see secrets.rs).

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// A field the file lacks takes its value from `Default`, so a settings.json
/// written by an older build still loads, whatever has been added since.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub sound_enabled: bool,
    pub sound_volume: f64,
    pub auto_close_interval: f64,
    /// Hovering the island opens it all the way, and it folds again shortly
    /// after the pointer leaves (the Mac's "Open on hover"). Off by default.
    pub open_on_hover: bool,
    /// A press anywhere outside the open island folds it, as a popover does; a
    /// card waiting for an answer stays. On by default. Wayland shows no app
    /// the clicks in other windows: there the island folds once the pointer
    /// has left it for a moment.
    pub close_on_click_outside: bool,
    /// Once a day, Ciel asks its latest release whether a newer version is out
    /// (updates.rs). Off by default: the network only reaches what the user
    /// turned on.
    pub check_updates: bool,
    pub absence_interval: f64,
    pub active_integrations: Vec<String>,
    /// The always-on workspace pill (src/core/pills.ts checks it is one).
    pub main_pill: String,
    /// "primary" = the main display, "cursor" = whichever display the mouse is on.
    pub screen: String,
    pub autostart: bool,
    pub hooks_installed: bool,
    /// Claude model used by the chat. Changeable in the settings window.
    pub model: String,
    /// Show the Claude plan pill (5 h and weekly limits) in the island's header.
    /// Off until the user turns it on, so the header stays as it shipped.
    pub show_plan_in_notch: bool,
    /// Ciel's status line relay is the one in Claude Code's settings.json.
    /// Like `hooks_installed`, the real state wins at launch over what was stored.
    pub plan_relay_installed: bool,
    /// Show the Codex plan pill (5 h / weekly limits from `codex app-server`).
    /// Off by default; nothing is installed for it.
    pub show_codex_plan_in_notch: bool,
    /// How long a notification shows in the island, in seconds; 0 keeps it
    /// until it is opened or dismissed (Settings → Integrations → Notifications).
    pub notification_seconds: u32,
    /// How many minutes before a meeting of the Calendar pill its card shows
    /// in the island (calendar/mod.rs); 0 never.
    pub meeting_reminder_minutes: u32,
    /// Linux: the other apps' notifications show in the island too (notify.rs).
    pub desktop_notifications: bool,
    /// GNOME: Ciel's extension hides the top bar, the island has the top (gnome_ext.rs).
    pub hide_gnome_panel: bool,
    /// Windows: Ciel's windows are left out of screen shares, recordings and
    /// screenshots (capture.rs). Off by default.
    pub hide_from_screen_sharing: bool,
    /// The flip clock after this many minutes without input; 0 is off (screensaver.rs).
    pub screensaver_minutes: u32,
    /// The System pill warns when CPU, memory, disk or temperature runs high (system.rs).
    pub system_warnings: bool,
    /// 12 or 24 hours on the flip clocks: "auto" (the language's), "12h" or "24h".
    pub clock_format: String,
    /// The flip clock's size: "small", "medium" or "large".
    pub screensaver_size: String,
    /// The date under the flip clock.
    pub screensaver_date: bool,
    /// At night, the screens off once the clock has shown `night_off_after` minutes.
    pub night_off: bool,
    /// The night, "HH:MM" to "HH:MM": the screens come back on with the clock at its end.
    pub night_from: String,
    pub night_until: String,
    pub night_off_after: u32,
    /// Who the chat talks to: "anthropic", a cloud provider of
    /// openai_compat.rs ("openai", "google", "openrouter"), or a model server
    /// of local_chat.rs ("ollama", "lmstudio", "custom"). Picked in the chat view.
    pub chat_provider: String,
    /// The model picked for each provider other than Anthropic (whose model is
    /// `model`), by provider id.
    pub chat_models: BTreeMap<String, String>,
    /// Addresses of the model servers once connected; empty means not connected.
    pub ollama_url: String,
    pub lmstudio_url: String,
    /// Any other OpenAI-compatible server; its key, if any, is in the keychain.
    pub custom_url: String,
    /// Who thinks for « OK Ciel », the dictation clean-up and the call notes
    /// (ai.rs): "auto" (Claude with a key, else the chat's local model),
    /// "claude" or "local". Kept as it comes: an unknown value reads as "auto".
    pub ai_engine: String,
    /// Global shortcuts the user changed, by action id; the others keep their
    /// default (see shortcuts.rs).
    pub shortcuts: crate::shortcuts::Bindings,
    /// A colour of the user's own for a pill's Ciel, by pill ID ("#RRGGBB"),
    /// picked in Settings → Active pills. Empty means the catalog's colours.
    /// Kept as it comes: src/core/pill-colors.ts reads whatever is not a
    /// colour as "no choice".
    pub pill_colors: BTreeMap<String, String>,
    /// Interface language: "" follows the system, else one of i18n::LANGUAGES
    /// ("fr", "pt-BR", "zh-Hans"…). Kept as it comes: a code this build
    /// doesn't know reads as "".
    pub language: String,
    /// Ciel on the desktop: whether he lives there, and his spot. Owned by
    /// the Rust side (desktop.rs) — what a webview sends back is ignored.
    pub desktop_cloud: DesktopCloudPref,
    /// The Finance pill's folder (expenses.csv and settings.json). Empty means
    /// `<config dir>/finance`; see finance.rs.
    pub finance_folder: String,
    /// The Alerts pill's port (alerts.rs).
    pub alerts_port: u16,
    /// The Alerts pill takes alerts from other computers, not only this one.
    /// Off by default: Ciel listens on 127.0.0.1 until the user allows it.
    pub alerts_lan: bool,
    /// Ciel's wallpapers (wallpaper/): what they go on and how they look.
    /// Written by the webviews, keys this build doesn't know included.
    pub wallpaper: WallpaperPrefs,
    /// What Ciel's brain may read when it is Claude (Settings → Ciel's brain).
    pub brain_reads: BrainReads,
}

/// What « OK Ciel » may read when its brain is Claude, with a key: what it
/// reads then leaves the computer, so the user picks. A local model reads
/// everything Ciel has, as before (voice/mod.rs `may_read`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BrainReads {
    /// The Calendar pill's meetings: titles, times, who is invited, agendas.
    pub calendar: bool,
    /// The Notetaker's calls: summaries, key points, to-dos (never the words).
    pub notes: bool,
    /// The Finance pill's spending. Off until the user turns it on.
    pub finance: bool,
}

impl Default for BrainReads {
    fn default() -> Self {
        Self { calendar: true, notes: true, finance: false }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopCloudPref {
    /// He was on the desktop when the app quit: he flies back out at launch.
    pub on_desktop: bool,
    /// Top-left corner of his window where the user last left him.
    pub spot: Option<DesktopSpot>,
}

/// Settings → Wallpapers. Off by default: nothing on the user's desktop or lock
/// screen changes until they turn it on. Rust reads `pauseOnBattery` and
/// `lockScreen`; the island owns the rest. A key this build doesn't know is
/// kept as it comes, and a value of the wrong kind falls back to its default
/// without costing the others.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperPrefs {
    /// Apply to the desktop.
    pub desktop: bool,
    /// Apply to the lock screen.
    pub lock_screen: bool,
    /// Desktop animated; false = still pictures.
    pub live: bool,
    /// Live pauses on battery.
    pub pause_on_battery: bool,
    /// Where the sun is reckoned from; `None` = guessed from the time zone.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Ciel's width as a fraction of the screen height.
    pub ciel_size: f64,
    /// Ciel's centre, as fractions of the width and height.
    pub ciel_x: f64,
    pub ciel_y: f64,
    /// Still pictures are redrawn at least this often.
    pub refresh_minutes: u32,
    /// Mood → theme or "auto"; a missing mood is "auto".
    pub moods: Map<String, Value>,
    /// Theme → "drawn" or "photos"; a missing theme is "drawn".
    pub sources: Map<String, Value>,
    /// Whatever else the webviews keep here.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for WallpaperPrefs {
    fn default() -> Self {
        Self {
            desktop: false,
            lock_screen: false,
            live: true,
            pause_on_battery: true,
            lat: None,
            lon: None,
            ciel_size: 0.2,
            ciel_x: 0.5,
            ciel_y: 0.66,
            refresh_minutes: 15,
            moods: Map::new(),
            sources: Map::new(),
            extra: Map::new(),
        }
    }
}

impl<'de> Deserialize<'de> for WallpaperPrefs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        fn take<T: serde::de::DeserializeOwned>(fields: &mut Map<String, Value>, key: &str, into: &mut T) {
            if let Some(value) = fields.remove(key) {
                if let Ok(value) = serde_json::from_value(value) {
                    *into = value;
                }
            }
        }
        let mut fields = Map::<String, Value>::deserialize(deserializer)?;
        let mut prefs = WallpaperPrefs::default();
        take(&mut fields, "desktop", &mut prefs.desktop);
        take(&mut fields, "lockScreen", &mut prefs.lock_screen);
        take(&mut fields, "live", &mut prefs.live);
        take(&mut fields, "pauseOnBattery", &mut prefs.pause_on_battery);
        take(&mut fields, "lat", &mut prefs.lat);
        take(&mut fields, "lon", &mut prefs.lon);
        take(&mut fields, "cielSize", &mut prefs.ciel_size);
        take(&mut fields, "cielX", &mut prefs.ciel_x);
        take(&mut fields, "cielY", &mut prefs.ciel_y);
        take(&mut fields, "refreshMinutes", &mut prefs.refresh_minutes);
        take(&mut fields, "moods", &mut prefs.moods);
        take(&mut fields, "sources", &mut prefs.sources);
        prefs.extra = fields;
        Ok(prefs)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopSpot {
    pub x: f64,
    pub y: f64,
    /// What x and y are measured in (`DesktopMode::space`).
    pub space: String,
}

fn default_model() -> String {
    crate::claude::DEFAULT_MODEL.to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sound_enabled: true,
            sound_volume: 0.12,
            auto_close_interval: 15.0,
            open_on_hover: false,
            close_on_click_outside: true,
            check_updates: false,
            absence_interval: 180.0,
            active_integrations: vec![
                "integration_resend".into(),
                "integration_n8n".into(),
                "integration_vercel".into(),
                "integration_github".into(),
            ],
            main_pill: "integration_claude".into(),
            screen: "primary".into(),
            autostart: false,
            hooks_installed: false,
            model: default_model(),
            show_plan_in_notch: false,
            plan_relay_installed: false,
            show_codex_plan_in_notch: false,
            notification_seconds: 7,
            meeting_reminder_minutes: 5,
            desktop_notifications: false,
            hide_gnome_panel: false,
            hide_from_screen_sharing: false,
            screensaver_minutes: 0,
            system_warnings: false,
            clock_format: "auto".into(),
            screensaver_size: "large".into(),
            screensaver_date: true,
            night_off: false,
            night_from: "23:00".into(),
            night_until: "06:00".into(),
            night_off_after: 120,
            chat_provider: crate::chat::ANTHROPIC.into(),
            chat_models: BTreeMap::new(),
            ollama_url: String::new(),
            lmstudio_url: String::new(),
            custom_url: String::new(),
            ai_engine: crate::ai::AUTO.into(),
            shortcuts: Default::default(),
            pill_colors: BTreeMap::new(),
            language: String::new(),
            desktop_cloud: DesktopCloudPref::default(),
            finance_folder: String::new(),
            alerts_port: crate::alerts::DEFAULT_PORT,
            alerts_lan: false,
            wallpaper: WallpaperPrefs::default(),
            brain_reads: BrainReads::default(),
        }
    }
}

pub use crate::platform::{config_dir, local_dir};

pub fn hook_exe_path() -> PathBuf {
    local_dir().join("bin").join(crate::platform::HOOK_EXE)
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load() -> Settings {
    load_from(&settings_path())
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let dir = config_dir();
    crate::platform::ensure_private_dir(&dir)?;
    save_to(&settings_path(), settings)
}

/// How many copies `set_aside` can make within one second before it gives up.
const MAX_COPIES_PER_SECOND: u32 = 100;

/// settings.json files that are there but whose contents never made it into
/// memory: they could not be read, or could not be used and could not be copied
/// aside either. What is in memory for them is the defaults, so the next save
/// must not quietly replace them — see `keep_what_was_not_loaded`.
static NOT_LOADED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn not_loaded() -> MutexGuard<'static, Vec<PathBuf>> {
    NOT_LOADED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One line in ciel.log. Tests must never write to the real one.
fn note(message: String) {
    #[cfg(not(test))]
    crate::log::line(message);
    #[cfg(test)]
    eprintln!("{message}");
}

/// The settings held in the bytes of a settings.json, or `None` when there is
/// nothing usable in them. Split from `load_from` so it can be tested without a
/// file.
fn parse(bytes: &[u8]) -> Option<Settings> {
    // PowerShell 5 and Notepad write a UTF-8 BOM, and serde_json refuses it.
    let text = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if text.iter().all(u8::is_ascii_whitespace) {
        return Some(Settings::default());
    }
    let Value::Object(fields) = serde_json::from_slice(text).ok()? else {
        return None;
    };
    let whole = serde_json::from_value(Value::Object(fields.clone()));
    Some(whole.unwrap_or_else(|_| salvage(fields)))
}

/// Settings from the fields of a settings.json that does not load as a whole.
/// Each field is taken on its own: one that is not the kind of value expected
/// falls back to its default instead of costing every other setting.
fn salvage(fields: Map<String, Value>) -> Settings {
    let Ok(Value::Object(mut kept)) = serde_json::to_value(Settings::default()) else {
        return Settings::default();
    };
    for (key, value) in fields {
        let mut candidate = kept.clone();
        candidate.insert(key.clone(), value);
        if serde_json::from_value::<Settings>(Value::Object(candidate.clone())).is_ok() {
            kept = candidate;
        } else {
            note(unusable_field(&key));
        }
    }
    serde_json::from_value(Value::Object(kept)).unwrap_or_default()
}

/// The log line for a field `salvage` had to drop. The name comes straight from
/// the file, so it is written escaped: a line break in it must not be able to
/// start what looks like another line of ciel.log.
fn unusable_field(key: &str) -> String {
    format!("settings.json: {key:?} is not usable — its default is used instead")
}

/// `load`, for the file at `path`: tests point it at a temporary directory.
///
/// Only a file that is not there means "start from the defaults" with nothing
/// more to do. One that cannot be read, or cannot be used, also gives the
/// defaults, but it still holds what the user had: it is kept, not written over.
fn load_from(path: &Path) -> Settings {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == ErrorKind::NotFound => return Settings::default(),
        Err(err) => {
            note(format!("can't read {}: {err} — using the defaults", path.display()));
            protect(path);
            return Settings::default();
        }
    };
    parse(&bytes).unwrap_or_else(|| {
        match set_aside(path, "corrupt", &bytes) {
            Ok(copy) => note(format!(
                "{} is not usable — kept as {}, using the defaults",
                path.display(),
                copy.display()
            )),
            Err(err) => {
                note(format!("{} is not usable and could not be copied: {err}", path.display()));
                protect(path);
            }
        }
        Settings::default()
    })
}

/// Marks `path` as holding something the next save must not write over.
fn protect(path: &Path) {
    let mut pending = not_loaded();
    if !pending.iter().any(|p| p == path) {
        pending.push(path.to_path_buf());
    }
}

/// Before the first save over a file `load_from` could not load: copies it
/// aside if it can be read by now, and refuses the save if it still cannot.
fn keep_what_was_not_loaded(path: &Path) -> std::io::Result<()> {
    let mut pending = not_loaded();
    let Some(index) = pending.iter().position(|p| p == path) else {
        return Ok(());
    };
    match std::fs::read(path) {
        Ok(bytes) => {
            let copy = set_aside(path, "unread", &bytes)?;
            note(format!("{} was never loaded — kept as {}", path.display(), copy.display()));
        }
        // Gone since: nothing is left to protect.
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => {
            return Err(std::io::Error::new(
                err.kind(),
                format!("{} still can't be read, so it is left as it is: {err}", path.display()),
            ));
        }
    }
    pending.swap_remove(index);
    Ok(())
}

/// Keeps `bytes`, the contents of `path`, in a file beside it named after it
/// with `.<kind>-YYYYMMDD-HHMMSS` appended. Contents an earlier copy of that
/// kind already holds are not copied again: a file nobody repairs would
/// otherwise leave one more copy behind at every launch.
fn set_aside(path: &Path, kind: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    if let Some(copy) = same_copy(path, kind, bytes) {
        return Ok(copy);
    }
    let t = crate::platform::local_time();
    let stamp = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    );
    set_aside_as(path, kind, &stamp, bytes, write_whole)
}

/// The copy of `path` of this `kind` that already holds exactly `bytes`.
fn same_copy(path: &Path, kind: &str, bytes: &[u8]) -> Option<PathBuf> {
    let prefix = format!("{}.{kind}-", path.file_name()?.to_string_lossy());
    std::fs::read_dir(path.parent()?)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .filter(|entry| entry.metadata().is_ok_and(|m| m.len() == bytes.len() as u64))
        .map(|entry| entry.path())
        .find(|copy| std::fs::read(copy).is_ok_and(|held| held == bytes))
}

/// `set_aside` for a given time stamp, writing through `write`. `-1`, `-2`… are
/// appended while the name is taken, so a copy that is already there is never
/// written over; and a copy that could not be written whole is removed, never
/// left looking like a faithful one.
fn set_aside_as(
    path: &Path,
    kind: &str,
    stamp: &str,
    bytes: &[u8],
    write: fn(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
) -> std::io::Result<PathBuf> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let base = format!("{name}.{kind}-{stamp}");
    for attempt in 0..MAX_COPIES_PER_SECOND {
        let copy = match attempt {
            0 => path.with_file_name(&base),
            n => path.with_file_name(format!("{base}-{n}")),
        };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&copy) {
            Ok(mut file) => {
                let written = write(&mut file, bytes);
                drop(file);
                if written.is_err() {
                    let _ = std::fs::remove_file(&copy);
                }
                return written.map(|()| copy);
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(ErrorKind::AlreadyExists, format!("too many copies named {base}")))
}

/// Writes `bytes` to `file` and waits until they are on the disk.
fn write_whole(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    file.write_all(bytes)?;
    file.sync_all()
}

/// `save`, for the file at `path`, whose directory already exists.
fn save_to(path: &Path, settings: &Settings) -> std::io::Result<()> {
    keep_what_was_not_loaded(path)?;
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;

    // A dotfiles setup can make settings.json a symlink: write to the file it
    // points at, so the link survives the rename below.
    #[cfg(unix)]
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(unix)]
    let path = resolved.as_path();

    // Write beside the target and rename over it: a crash, a full disk or a
    // power cut leaves the previous settings.json intact rather than half a file.
    let temp = path.with_extension(format!("json.ciel-{}", std::process::id()));
    let written = std::fs::File::create(&temp)
        .and_then(|mut file| write_whole(&mut file, &json))
        .and_then(|()| std::fs::rename(&temp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A settings.json in which no value is the default one.
    // Two #: the colours in it are written "#RRGGBB".
    const CUSTOM: &str = r##"{
  "soundEnabled": false,
  "soundVolume": 0.5,
  "autoCloseInterval": 30.0,
  "openOnHover": true,
  "closeOnClickOutside": false,
  "checkUpdates": true,
  "absenceInterval": 60.0,
  "activeIntegrations": ["integration_notion"],
  "mainPill": "agent_cursor",
  "screen": "cursor",
  "autostart": true,
  "hooksInstalled": true,
  "model": "some-model",
  "showPlanInNotch": true,
  "planRelayInstalled": true,
  "showCodexPlanInNotch": true,
  "notificationSeconds": 20,
  "meetingReminderMinutes": 10,
  "desktopNotifications": true,
  "hideGnomePanel": true,
  "hideFromScreenSharing": true,
  "screensaverMinutes": 10,
  "systemWarnings": true,
  "clockFormat": "24h",
  "screensaverSize": "small",
  "screensaverDate": false,
  "nightOff": true,
  "nightFrom": "22:30",
  "nightUntil": "07:00",
  "nightOffAfter": 60,
  "chatProvider": "ollama",
  "chatModels": { "ollama": "llama3.2", "openai": "gpt-x" },
  "ollamaUrl": "http://127.0.0.1:11434",
  "lmstudioUrl": "http://127.0.0.1:1234",
  "customUrl": "https://llm.example.com",
  "aiEngine": "local",
  "shortcuts": { "openChat": { "keys": "Ctrl+Shift+K", "enabled": false } },
  "pillColors": { "integration_claude": "#2DD4BF" },
  "language": "pt-BR",
  "desktopCloud": { "onDesktop": true, "spot": { "x": 1500.5, "y": -300.0, "space": "screen" } },
  "financeFolder": "/srv/dashboard/data",
  "alertsPort": 8080,
  "alertsLan": true,
  "wallpaper": {
    "desktop": true, "lockScreen": true, "live": false, "pauseOnBattery": false,
    "lat": 48.85, "lon": 2.35, "cielSize": 0.3, "cielX": 0.25, "cielY": 0.5,
    "refreshMinutes": 30, "moods": { "error": "storm", "idle": "auto" },
    "sources": { "space": "photos" }, "aKeyFromALaterVersion": [1, 2]
  },
  "brainReads": { "calendar": false, "notes": true, "finance": true }
}"##;

    fn custom() -> Value {
        serde_json::from_str(CUSTOM).unwrap()
    }

    /// `CUSTOM` with one key replaced, or removed when `value` is `None`.
    fn custom_with(key: &str, value: Option<Value>) -> Vec<u8> {
        let mut object = custom().as_object().unwrap().clone();
        match value {
            Some(value) => object.insert(key.to_string(), value),
            None => object.remove(key),
        };
        serde_json::to_vec(&object).unwrap()
    }

    /// Settings compare through their JSON: the struct has no `PartialEq`.
    fn shown(settings: &Settings) -> Value {
        serde_json::to_value(settings).unwrap()
    }

    fn defaults() -> Value {
        shown(&Settings::default())
    }

    /// A fresh directory of our own, and the settings.json it will hold.
    fn scratch(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir()
            .join(format!("ciel-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        (dir, file)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    /// What was set aside under `settings.json.<kind>-…`.
    fn kept(dir: &Path, kind: &str) -> Vec<Vec<u8>> {
        let prefix = format!("settings.json.{kind}-");
        names(dir)
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .map(|name| std::fs::read(dir.join(name)).unwrap())
            .collect()
    }

    // ── Reading ───────────────────────────────────────────────────────────────

    #[test]
    fn a_missing_file_is_the_defaults_and_is_not_created() {
        let (dir, file) = scratch("missing");
        assert_eq!(shown(&load_from(&file)), defaults());
        assert!(names(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_file_is_the_defaults_and_nothing_is_set_aside() {
        let (dir, file) = scratch("empty");
        for nothing in [&b""[..], &b" \r\n\t "[..]] {
            std::fs::write(&file, nothing).unwrap();
            assert_eq!(shown(&load_from(&file)), defaults());
        }
        assert_eq!(names(&dir), ["settings.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_fields_are_ignored_and_the_known_ones_kept() {
        let bytes = custom_with("somethingFromALaterVersion", Some(json!({ "a": [1, 2] })));
        assert_eq!(shown(&parse(&bytes).unwrap()), custom());
    }

    #[test]
    fn a_file_from_before_the_model_setting_keeps_everything_else() {
        let loaded = shown(&parse(&custom_with("model", None)).unwrap());
        let mut expected = custom();
        expected["model"] = json!(crate::claude::DEFAULT_MODEL);
        assert_eq!(loaded, expected);
    }

    #[test]
    fn a_file_from_before_the_desktop_cloud_keeps_him_in_the_island() {
        let loaded = parse(&custom_with("desktopCloud", None)).unwrap();
        assert_eq!(loaded.desktop_cloud, DesktopCloudPref::default());
        assert!(!loaded.desktop_cloud.on_desktop);
        assert_eq!(loaded.language, "pt-BR");
    }

    #[test]
    fn a_half_written_desktop_spot_costs_only_the_spot() {
        let loaded = parse(&custom_with(
            "desktopCloud",
            Some(json!({ "onDesktop": true, "spot": { "x": "left" } })),
        ))
        .unwrap();
        // The whole field falls back, and nothing else does.
        assert_eq!(loaded.desktop_cloud, DesktopCloudPref::default());
        assert_eq!(loaded.model, "some-model");

        let loaded = parse(&custom_with("desktopCloud", Some(json!({ "onDesktop": true })))).unwrap();
        assert!(loaded.desktop_cloud.on_desktop);
        assert_eq!(loaded.desktop_cloud.spot, None);
    }

    #[test]
    fn a_file_from_before_the_colours_paints_every_pill_as_the_catalog_says() {
        let loaded = parse(&custom_with("pillColors", None)).unwrap();
        assert!(loaded.pill_colors.is_empty());
        assert_eq!(loaded.language, "pt-BR");
    }

    #[test]
    fn pill_colours_are_kept_as_written_and_cost_nothing_else_when_unusable() {
        // A pill a newer build added keeps its colour through a save by this one.
        let loaded = parse(&custom_with("pillColors", Some(json!({ "agent_new": "#abcdef" })))).unwrap();
        assert_eq!(loaded.pill_colors.get("agent_new").map(String::as_str), Some("#abcdef"));
        // Not a map of strings: the colours fall back, and nothing else does.
        let loaded = parse(&custom_with("pillColors", Some(json!(["#2DD4BF"])))).unwrap();
        assert!(loaded.pill_colors.is_empty());
        assert_eq!(loaded.model, "some-model");
    }

    #[test]
    fn a_language_this_build_does_not_know_is_kept_as_written() {
        // "" (follow the system) when absent, as it comes otherwise.
        assert_eq!(parse(&custom_with("language", None)).unwrap().language, "");
        assert_eq!(parse(&custom_with("language", Some(json!("xx")))).unwrap().language, "xx");
    }

    #[test]
    fn a_missing_field_takes_its_default_and_the_rest_are_kept() {
        for key in custom().as_object().unwrap().keys() {
            let loaded = parse(&custom_with(key, None))
                .unwrap_or_else(|| panic!("a file without {key} was thrown away"));
            let mut expected = custom();
            expected[key] = defaults()[key].clone();
            assert_eq!(shown(&loaded), expected, "without {key}");
        }
    }

    #[test]
    fn a_field_of_the_wrong_type_takes_its_default_and_the_rest_are_kept() {
        for (key, wrong) in [
            ("soundVolume", json!("loud")),
            ("soundEnabled", json!(1)),
            ("activeIntegrations", json!("integration_n8n")),
            ("mainPill", json!(["agent_cursor"])),
            ("screen", Value::Null),
            ("shortcuts", json!("Ctrl+Alt+A")),
        ] {
            let loaded = parse(&custom_with(key, Some(wrong)))
                .unwrap_or_else(|| panic!("a file with a bad {key} was thrown away"));
            let mut expected = custom();
            expected[key] = defaults()[key].clone();
            assert_eq!(shown(&loaded), expected, "bad {key}");
        }
    }

    #[test]
    fn a_utf8_bom_is_not_corruption() {
        // What PowerShell 5's `Set-Content -Encoding utf8` and Notepad write.
        let (dir, file) = scratch("bom");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(CUSTOM.as_bytes());
        std::fs::write(&file, &bytes).unwrap();
        assert_eq!(shown(&load_from(&file)), custom());
        assert_eq!(names(&dir), ["settings.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unusable_file_is_the_defaults_and_is_set_aside_byte_for_byte() {
        for (name, bad) in [
            ("invalid", &b"{ \"soundVolume\": 0.5, \xff oops"[..]),
            ("not-an-object", &b"[1, 2, 3]"[..]),
        ] {
            let (dir, file) = scratch(name);
            std::fs::write(&file, bad).unwrap();
            assert_eq!(shown(&load_from(&file)), defaults());
            assert_eq!(kept(&dir, "corrupt"), [bad.to_vec()], "{name}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_second_unusable_file_never_replaces_the_copy_of_the_first() {
        // Both within the same second: the names must still differ.
        let (dir, file) = scratch("twice");
        let (first, second) = (&b"{ first"[..], &b"{ second"[..]);
        std::fs::write(&file, first).unwrap();
        load_from(&file);
        std::fs::write(&file, second).unwrap();
        load_from(&file);
        let mut copies = kept(&dir, "corrupt");
        copies.sort();
        assert_eq!(copies, [first.to_vec(), second.to_vec()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn something_that_is_not_a_readable_file_is_the_defaults_and_is_left_alone() {
        // A directory in the file's place: a read error that is not "not found".
        let (dir, file) = scratch("unreadable");
        std::fs::create_dir(&file).unwrap();
        assert_eq!(shown(&load_from(&file)), defaults());
        assert_eq!(names(&dir), ["settings.json"]);
        assert!(file.is_dir());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Opens `path` the way a process that shares nothing does: nobody else can
    /// read or write it until the handle is dropped.
    #[cfg(windows)]
    fn locked(path: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).share_mode(0).open(path).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn a_file_that_could_not_be_read_is_set_aside_before_it_is_saved_over() {
        let (dir, file) = scratch("locked-then-free");
        std::fs::write(&file, CUSTOM).unwrap();
        let lock = locked(&file);
        let loaded = load_from(&file);
        assert_eq!(shown(&loaded), defaults());
        drop(lock);

        save_to(&file, &loaded).unwrap();
        assert_eq!(kept(&dir, "unread"), [CUSTOM.as_bytes().to_vec()]);
        assert_eq!(shown(&load_from(&file)), defaults());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_that_still_cannot_be_read_is_not_saved_over() {
        let (dir, file) = scratch("locked");
        std::fs::write(&file, CUSTOM).unwrap();
        let lock = locked(&file);
        let loaded = load_from(&file);
        assert!(save_to(&file, &loaded).is_err());
        drop(lock);
        assert_eq!(std::fs::read(&file).unwrap(), CUSTOM.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_could_not_be_read_is_never_saved_over_unseen() {
        // The same as the two locked-file tests above, on every platform: a
        // directory in the file's place cannot be read either.
        let (dir, file) = scratch("unread");
        std::fs::create_dir(&file).unwrap();
        let loaded = load_from(&file);

        // Still unreadable: the save is refused and nothing is touched.
        assert!(save_to(&file, &loaded).is_err());
        assert!(file.is_dir());
        assert_eq!(names(&dir), ["settings.json"]);

        // Readable by now: kept aside first, and only then saved over.
        std::fs::remove_dir(&file).unwrap();
        std::fs::write(&file, CUSTOM).unwrap();
        save_to(&file, &loaded).unwrap();
        assert_eq!(kept(&dir, "unread"), [CUSTOM.as_bytes().to_vec()]);
        assert_eq!(shown(&load_from(&file)), defaults());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wallpapers_are_off_until_the_user_turns_them_on() {
        assert_eq!(
            serde_json::to_value(WallpaperPrefs::default()).unwrap(),
            json!({
                "desktop": false, "lockScreen": false, "live": true, "pauseOnBattery": true,
                "lat": null, "lon": null, "cielSize": 0.2, "cielX": 0.5, "cielY": 0.66,
                "refreshMinutes": 15, "moods": {}, "sources": {}
            })
        );
        assert_eq!(parse(&custom_with("wallpaper", None)).unwrap().wallpaper, WallpaperPrefs::default());
        assert_eq!(parse(b"{}").unwrap().wallpaper, WallpaperPrefs::default());
    }

    #[test]
    fn ciel_shows_in_screen_shares_until_the_user_hides_him() {
        assert!(!Settings::default().hide_from_screen_sharing);
        assert!(!parse(&custom_with("hideFromScreenSharing", None)).unwrap().hide_from_screen_sharing);
        assert!(parse(&custom_with("hideFromScreenSharing", Some(json!(true)))).unwrap().hide_from_screen_sharing);
    }

    #[test]
    fn wallpaper_keys_from_a_later_version_are_kept() {
        let loaded = parse(CUSTOM.as_bytes()).unwrap();
        assert_eq!(loaded.wallpaper.extra.get("aKeyFromALaterVersion"), Some(&json!([1, 2])));
        assert_eq!(loaded.wallpaper.lat, Some(48.85));
        assert_eq!(loaded.wallpaper.moods.get("error"), Some(&json!("storm")));
        let written = serde_json::to_value(&loaded).unwrap();
        assert_eq!(written["wallpaper"]["aKeyFromALaterVersion"], json!([1, 2]));
        assert_eq!(written["wallpaper"]["refreshMinutes"], json!(30));
    }

    #[test]
    fn a_bad_wallpaper_value_costs_only_itself() {
        let loaded = parse(&custom_with(
            "wallpaper",
            Some(json!({ "desktop": true, "cielSize": "big", "lat": "north", "moods": [1], "refreshMinutes": -5 })),
        ))
        .unwrap();
        let w = loaded.wallpaper;
        assert!(w.desktop);
        assert_eq!((w.ciel_size, w.lat, w.refresh_minutes), (0.2, None, 15));
        assert!(w.moods.is_empty() && w.extra.is_empty());
        // Not an object at all: the defaults, and nothing else is lost.
        let loaded = parse(&custom_with("wallpaper", Some(json!("on")))).unwrap();
        assert_eq!(loaded.wallpaper, WallpaperPrefs::default());
        assert_eq!(loaded.model, "some-model");
    }

    // ── Copies set aside ──────────────────────────────────────────────────────

    #[test]
    fn an_unusable_file_is_set_aside_once_however_often_it_is_loaded() {
        let (dir, file) = scratch("once");
        std::fs::write(&file, b"{ broken").unwrap();
        for _ in 0..3 {
            load_from(&file);
        }
        assert_eq!(kept(&dir, "corrupt"), [b"{ broken".to_vec()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_made_within_the_same_second_are_numbered_and_none_is_written_over() {
        let (dir, file) = scratch("numbered");
        let stamp = "20260102-030405";
        let contents = [&b"one"[..], &b"two"[..], &b"three"[..]];
        let copies: Vec<PathBuf> = contents
            .iter()
            .map(|bytes| set_aside_as(&file, "corrupt", stamp, bytes, write_whole).unwrap())
            .collect();

        let named = |suffix: &str| dir.join(format!("settings.json.corrupt-{stamp}{suffix}"));
        assert_eq!(copies, [named(""), named("-1"), named("-2")]);
        for (copy, bytes) in copies.iter().zip(contents) {
            assert_eq!(std::fs::read(copy).unwrap(), bytes);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_copy_that_could_not_be_written_whole_is_removed() {
        fn half_then_fail(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
            use std::io::Write;
            file.write_all(&bytes[..bytes.len() / 2])?;
            Err(std::io::Error::other("the disk is full"))
        }
        let (dir, file) = scratch("half");
        let copy = set_aside_as(&file, "corrupt", "20260102-030405", b"all of it", half_then_fail);
        assert!(copy.is_err());
        assert!(names(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_field_name_cannot_start_a_line_of_its_own_in_the_log() {
        let forged = "volume\r\n2026-01-02 03:04:05 hook id=1 answered allow";
        let line = unusable_field(forged);
        assert!(!line.contains(['\r', '\n']));
        assert!(line.contains(r"volume\r\n2026-01-02"));
    }

    // ── Writing ───────────────────────────────────────────────────────────────

    /// A dotfiles setup keeps the real file elsewhere and links to it.
    #[cfg(unix)]
    #[test]
    fn a_save_through_a_symlink_keeps_the_link_and_writes_its_target() {
        let (dir, file) = scratch("symlink");
        let real = dir.join("dotfiles-settings.json");
        std::fs::write(&real, CUSTOM).unwrap();
        std::os::unix::fs::symlink(&real, &file).unwrap();

        save_to(&file, &Settings::default()).unwrap();
        assert!(std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
        assert_eq!(shown(&parse(&std::fs::read(&real).unwrap()).unwrap()), defaults());
        assert_eq!(names(&dir), ["dotfiles-settings.json", "settings.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_field_survives_a_save_and_a_load() {
        let (dir, file) = scratch("round-trip");
        let settings: Settings = serde_json::from_str(CUSTOM).unwrap();
        save_to(&file, &settings).unwrap();
        assert_eq!(shown(&load_from(&file)), custom());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_holds_the_camel_case_keys_the_front_end_uses() {
        let (dir, file) = scratch("keys");
        save_to(&file, &Settings::default()).unwrap();
        let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let keys: Vec<&str> = written.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "soundEnabled",
                "soundVolume",
                "autoCloseInterval",
                "openOnHover",
                "closeOnClickOutside",
                "checkUpdates",
                "absenceInterval",
                "activeIntegrations",
                "mainPill",
                "screen",
                "autostart",
                "hooksInstalled",
                "model",
                "showPlanInNotch",
                "planRelayInstalled",
                "showCodexPlanInNotch",
                "notificationSeconds",
                "meetingReminderMinutes",
                "desktopNotifications",
                "hideGnomePanel",
                "hideFromScreenSharing",
                "screensaverMinutes",
                "systemWarnings",
                "clockFormat",
                "screensaverSize",
                "screensaverDate",
                "nightOff",
                "nightFrom",
                "nightUntil",
                "nightOffAfter",
                "chatProvider",
                "chatModels",
                "ollamaUrl",
                "lmstudioUrl",
                "customUrl",
                "aiEngine",
                "shortcuts",
                "pillColors",
                "language",
                "desktopCloud",
                "financeFolder",
                "alertsPort",
                "alertsLan",
                "wallpaper",
                "brainReads",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_replaces_the_file_that_was_there() {
        let (dir, file) = scratch("replace");
        std::fs::write(&file, CUSTOM).unwrap();
        save_to(&file, &Settings::default()).unwrap();
        assert_eq!(shown(&load_from(&file)), defaults());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_leaves_nothing_but_settings_json_behind() {
        let (dir, file) = scratch("tidy");
        save_to(&file, &Settings::default()).unwrap();
        save_to(&file, &Settings::default()).unwrap();
        assert_eq!(names(&dir), ["settings.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_temporary_file_that_cannot_be_written_leaves_the_original_alone() {
        // The new contents go to a file beside the target first. With a
        // directory squatting on that name, the write cannot even start.
        let (dir, file) = scratch("blocked");
        std::fs::write(&file, CUSTOM).unwrap();
        let temp = dir.join(format!("settings.json.ciel-{}", std::process::id()));
        std::fs::create_dir(&temp).unwrap();

        assert!(save_to(&file, &Settings::default()).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), CUSTOM.as_bytes());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
