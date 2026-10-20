// Ciel's GNOME Shell extension (gnome-extension/): what lets Open on a desktop
// app's notification go to the very chat or page, as GNOME's own list does.
// Only the notification server can tell an app its notification was clicked,
// and on GNOME that is the Shell; the extension finds GNOME's notification by
// its id and activates or dismisses it (notify.rs keeps the id).
//
// Installed only from Settings, after the user has seen every place written:
// the extension's two files and its name added to GNOME's enabled extensions.
// GNOME Shell reads new extensions when it starts: on X11 Alt+F2 then r, on
// Wayland the next login.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtStatus {
    /// A GNOME session: the extension can be of use.
    pub available: bool,
    /// This version's files are in place and enabled.
    pub installed: bool,
    /// GNOME Shell has it running: Open goes through it.
    pub active: bool,
    /// What installing writes, or removing takes back.
    pub places: Vec<String>,
}

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Duration;

    use dbus::blocking::Connection;

    use super::ExtStatus;

    pub const UUID: &str = "ciel@jeaneudesasg.github.io";
    const PATH: &str = "/io/github/jeaneudesasg/Ciel/Shell";
    const IFACE: &str = "io.github.jeaneudesasg.Ciel.Shell";
    const METADATA: &str = include_str!("../gnome-extension/metadata.json");
    const EXTENSION: &str = include_str!("../gnome-extension/extension.js");
    const ENABLED: [&str; 2] = ["org.gnome.shell", "enabled-extensions"];

    fn dir() -> PathBuf {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
        data.join("gnome-shell/extensions").join(UUID)
    }

    fn gnome() -> bool {
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().split(':').any(|d| d.eq_ignore_ascii_case("gnome"))
    }

    fn gsettings(args: &[&str]) -> Option<String> {
        let out = Command::new("gsettings").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// GNOME's list as gsettings prints it: "['a', 'b']" or "@as []".
    pub fn parse_list(printed: &str) -> Vec<String> {
        printed.split('\'').skip(1).step_by(2).map(String::from).collect()
    }

    pub fn print_list(items: &[String]) -> String {
        let quoted: Vec<String> = items.iter().map(|i| format!("'{i}'")).collect();
        format!("[{}]", quoted.join(", "))
    }

    fn enabled() -> Vec<String> {
        gsettings(&["get", ENABLED[0], ENABLED[1]]).map(|l| parse_list(&l)).unwrap_or_default()
    }

    fn call(method: &str, args: impl dbus::arg::AppendAll) -> Option<bool> {
        let conn = Connection::new_session().ok()?;
        let proxy = conn.with_proxy("org.gnome.Shell", PATH, Duration::from_secs(2));
        let (done,): (bool,) = proxy.method_call(IFACE, method, args).ok()?;
        Some(done)
    }

    fn active() -> bool {
        use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
        let Ok(conn) = Connection::new_session() else { return false };
        let proxy = conn.with_proxy("org.gnome.Shell", PATH, Duration::from_millis(800));
        proxy.get::<u32>(IFACE, "Version").is_ok()
    }

    pub fn status() -> ExtStatus {
        let d = dir();
        let installed = std::fs::read_to_string(d.join("extension.js")).is_ok_and(|e| e == EXTENSION)
            && enabled().iter().any(|u| u == UUID);
        ExtStatus {
            available: gnome(),
            installed,
            active: gnome() && active(),
            places: vec![
                d.join("metadata.json").display().to_string(),
                d.join("extension.js").display().to_string(),
                format!("gsettings {} {}: + '{UUID}'", ENABLED[0], ENABLED[1]),
            ],
        }
    }

    pub fn set(on: bool) -> Result<ExtStatus, String> {
        let d = dir();
        let mut list = enabled();
        list.retain(|u| u != UUID);
        if on {
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            std::fs::write(d.join("metadata.json"), METADATA).map_err(|e| e.to_string())?;
            std::fs::write(d.join("extension.js"), EXTENSION).map_err(|e| e.to_string())?;
            list.push(UUID.to_string());
        } else {
            let _ = std::fs::remove_dir_all(&d);
        }
        gsettings(&["set", ENABLED[0], ENABLED[1], &print_list(&list)]).ok_or("gsettings refused the change")?;
        crate::log::line(format!("gnome extension: {}", if on { "installed" } else { "removed" }));
        Ok(status())
    }

    /// The top bar hidden or back, at once; false when the extension isn't running.
    pub fn set_panel(hidden: bool) -> bool {
        call("SetPanelHidden", (hidden,)).unwrap_or(false)
    }

    /// Open or dismiss GNOME's own notification; false when the extension
    /// isn't running or GNOME no longer has it.
    pub fn act(shell: &crate::notify::ShellRef, open: bool) -> bool {
        use crate::notify::ShellRef;
        let done = match shell {
            ShellRef::Fdo(id) => call(if open { "Activate" } else { "Dismiss" }, (*id,)),
            ShellRef::Gtk { app, id } => call(if open { "ActivateGtk" } else { "DismissGtk" }, (app.as_str(), id.as_str())),
        };
        done.unwrap_or(false)
    }
}

#[cfg(target_os = "linux")]
pub use imp::{act, set_panel};

#[tauri::command]
pub fn gnome_extension_status() -> ExtStatus {
    #[cfg(target_os = "linux")]
    return imp::status();
    #[cfg(not(target_os = "linux"))]
    ExtStatus { available: false, installed: false, active: false, places: Vec::new() }
}

#[tauri::command]
pub fn gnome_extension_set(on: bool) -> Result<ExtStatus, String> {
    #[cfg(target_os = "linux")]
    return imp::set(on);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = on;
        Err("GNOME only".into())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::imp::*;

    #[test]
    fn gnomes_extension_list_reads_and_writes_back() {
        assert_eq!(parse_list("@as []"), Vec::<String>::new());
        let list = parse_list("['a@x', 'ciel@jeaneudesasg.github.io']");
        assert_eq!(list, ["a@x", UUID]);
        assert_eq!(print_list(&list), "['a@x', 'ciel@jeaneudesasg.github.io']");
        assert_eq!(print_list(&[]), "[]");
    }
}
