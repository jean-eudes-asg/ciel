// Ciel's own updates (Settings → General → Updates), through Tauri's updater:
// it reads latest.json from the latest GitHub release and installs the package
// it names once its signature checks out against the key in tauri.conf.json —
// the installer on Windows (it restarts Ciel when done), the AppImage itself on
// Linux. A .deb or .rpm belongs to the system's package manager: those are told
// a new version is out and get the release page.
//
// Nothing reaches out unless asked: "Check now", or once a day when the user
// turned that on (off by default — the network only reaches what the user
// configured). Nothing installs without a press on "Install and restart".

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The AppImage's entry in latest.json: what a Linux install, of any kind,
/// reads the new version from (only an AppImage installs it).
#[cfg(target_os = "linux")]
const LINUX_TARGET: &str = "linux-x86_64-appimage";
/// After launch, the first daily check waits this long: Ciel starts first.
const FIRST_CHECK: Duration = Duration::from_secs(90);
const DAY: Duration = Duration::from_secs(24 * 3600);
const EVENT: &str = "update-status";

/// What Settings shows, sent whole on every change.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// This build knows the key updates are signed with (a dev build doesn't).
    pub ready: bool,
    pub current: String,
    /// A newer version found by the last check.
    pub available: Option<String>,
    /// This install replaces itself (Windows, an AppImage); else the release page.
    pub can_install: bool,
    /// Checking or installing.
    pub busy: bool,
    /// Downloaded so far while installing, 0…1, when the size is known.
    pub progress: Option<f64>,
    /// The last check's or install's error, as the updater says it.
    pub error: Option<String>,
    /// When the last check ended (Unix ms), 0 when none did.
    pub checked_at: f64,
}

static STATUS: Mutex<Option<UpdateStatus>> = Mutex::new(None);
static FOUND: Mutex<Option<Update>> = Mutex::new(None);
/// The daily check is on; its generation, so a restarted loop ends the old one.
static AUTO: AtomicBool = AtomicBool::new(false);
static AUTO_GEN: AtomicU64 = AtomicU64::new(0);
/// The version the island was last told about, so a day's check doesn't repeat it.
static TOLD: Mutex<Option<String>> = Mutex::new(None);

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// The key the packages are checked against, from tauri.conf.json.
fn ready(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

/// Whether this install can replace itself: Windows' installer can, and an
/// AppImage (its runtime says where it is); a .deb or .rpm can't.
pub fn installs_itself(windows: bool, appimage: Option<&std::ffi::OsStr>) -> bool {
    windows || appimage.is_some_and(|p| !p.is_empty())
}

fn can_install() -> bool {
    installs_itself(cfg!(windows), std::env::var_os("APPIMAGE").as_deref())
}

fn status(app: &AppHandle) -> UpdateStatus {
    STATUS.lock().unwrap().clone().unwrap_or_else(|| UpdateStatus {
        ready: ready(app),
        current: app.package_info().version.to_string(),
        can_install: can_install(),
        ..UpdateStatus::default()
    })
}

/// Changes the status and tells every window (Settings listens).
fn update(app: &AppHandle, change: impl FnOnce(&mut UpdateStatus)) -> UpdateStatus {
    let mut s = status(app);
    change(&mut s);
    *STATUS.lock().unwrap() = Some(s.clone());
    let _ = app.emit(EVENT, &s);
    s
}

/// Asks latest.json whether a newer version is out.
async fn check(app: &AppHandle) -> UpdateStatus {
    if !ready(app) {
        return status(app);
    }
    if status(app).busy {
        return status(app);
    }
    update(app, |s| {
        s.busy = true;
        s.error = None;
    });
    // Windows' installer closes Ciel at once: the same as Quit does first.
    let handle = app.clone();
    #[allow(unused_mut)]
    let mut builder = app.updater_builder().on_before_exit(move || crate::before_exit(&handle));
    #[cfg(target_os = "linux")]
    {
        builder = builder.target(LINUX_TARGET);
    }
    let found = match builder.build() {
        Ok(updater) => updater.check().await,
        Err(err) => Err(err),
    };
    let (available, error) = match found {
        Ok(Some(found)) => {
            let version = found.version.clone();
            *FOUND.lock().unwrap() = Some(found);
            (Some(version), None)
        }
        Ok(None) => {
            *FOUND.lock().unwrap() = None;
            (None, None)
        }
        Err(err) => {
            crate::log::line(format!("updates: check failed: {err}"));
            (status(app).available, Some(err.to_string()))
        }
    };
    update(app, |s| {
        s.busy = false;
        s.available = available;
        s.error = error;
        s.checked_at = now_ms();
    })
}

/// The island hears of a new version once: a notification whose Open is Settings.
fn tell_island(app: &AppHandle, version: &str) {
    {
        let mut told = TOLD.lock().unwrap();
        if told.as_deref() == Some(version) {
            return;
        }
        *told = Some(version.to_string());
    }
    let n = crate::notify::Notification {
        id: crate::notify::next_id("ciel"),
        source: "ciel".into(),
        app: "Ciel".into(),
        title: crate::i18n::tf("Ciel {version} is out", &[("version", version)]),
        body: crate::i18n::t("Install it from Settings → General."),
        icon: None,
        at: now_ms(),
        tag: Some("ciel-update".into()),
        buttons: Vec::new(),
        until: None,
        time: None,
    };
    crate::notify::show(app, n, crate::notify::Target::Settings("general".into()));
}

/// The daily check: on at launch and after each settings save when the user
/// wants it. Asleep between checks: nothing runs meanwhile.
pub fn sync(app: &AppHandle, on: bool) {
    if AUTO.swap(on, Ordering::SeqCst) == on || !on {
        if !on {
            AUTO_GEN.fetch_add(1, Ordering::SeqCst);
        }
        return;
    }
    let generation = AUTO_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        while AUTO.load(Ordering::SeqCst) && AUTO_GEN.load(Ordering::SeqCst) == generation {
            let s = check(&app).await;
            if let Some(version) = s.available.as_deref() {
                tell_island(&app, version);
            }
            tokio::time::sleep(DAY).await;
        }
    });
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    status(&app)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateStatus {
    check(&app).await
}

/// "Install and restart": downloads the package, checks its signature, and
/// installs it. Windows' installer closes Ciel and opens it again when done;
/// on Linux the AppImage is replaced, and Ciel restarts itself.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let Some(found) = FOUND.lock().unwrap().clone() else {
        return Err("no update to install".into());
    };
    if !can_install() || status(&app).busy {
        return Err("this install can't update itself".into());
    }
    crate::log::line(format!("updates: installing {}", found.version));
    update(&app, |s| {
        s.busy = true;
        s.error = None;
        s.progress = Some(0.0);
    });
    let progress_app = app.clone();
    let mut got = 0usize;
    let mut shown = 0.0f64;
    let installed = found
        .download_and_install(
            move |chunk, total| {
                got += chunk;
                let Some(total) = total.filter(|t| *t > 0) else { return };
                let f = (got as f64 / total as f64).min(1.0);
                // A step every 2 %: Settings redraws, nothing more.
                if f - shown >= 0.02 || f >= 1.0 {
                    shown = f;
                    update(&progress_app, |s| s.progress = Some(f));
                }
            },
            || {},
        )
        .await;
    match installed {
        // Windows doesn't get here: its installer took over and Ciel exited.
        Ok(()) => {
            crate::log::line("updates: installed, restarting");
            crate::before_exit(&app);
            app.restart();
        }
        Err(err) => {
            crate::log::line(format!("updates: install failed: {err}"));
            update(&app, |s| {
                s.busy = false;
                s.progress = None;
                s.error = Some(err.to_string());
            });
            Err(err.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn windows_and_an_appimage_replace_themselves_a_package_doesnt() {
        assert!(installs_itself(true, None));
        assert!(installs_itself(false, Some(OsStr::new("/home/me/Apps/Ciel.AppImage"))));
        assert!(!installs_itself(false, None), "a .deb or .rpm: the release page");
        assert!(!installs_itself(false, Some(OsStr::new(""))));
    }

    #[test]
    fn settings_get_the_status_in_camel_case() {
        let s = UpdateStatus { ready: true, current: "0.2.7".into(), available: Some("0.2.8".into()), can_install: true, ..Default::default() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["available"], "0.2.8");
        assert_eq!(v["canInstall"], true);
        assert_eq!(v["checkedAt"], 0.0);
        assert!(v["progress"].is_null());
    }
}
