// Ciel for Chrome's bridge, made known to the browsers (native messaging): a
// small manifest naming the relay (ciel-hook, in Ciel's own bin folder, put
// there at every launch) and the one extension allowed to start it, then on
// Windows a registry value per browser pointing at that manifest, and on Linux
// the manifest itself in each browser's folder. Written only when the user
// asks (Settings → Integrations → Notifications), after seeing every place
// written; taken out the same way.

use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;

/// The bridge's name, as the extension asks for it (extension/sw.js).
pub const HOST: &str = "io.github.jeaneudesasg.ciel";
/// Ciel for Chrome, as loaded from extension/ (its manifest's key fixes the id).
pub const EXTENSION_ID: &str = "jfpabeoddliadjnngnhbelmmcogkgaje";

/// The manifest for the relay at `relay`.
pub fn manifest(relay: &str) -> Value {
    json!({
        "name": HOST,
        "description": "Ciel: what websites show, in Ciel's island",
        "path": relay,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    })
}

/// Whether the bridge is known to the browsers, and every place that says so
/// (or would, once connected): files, and registry values on Windows.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub installed: bool,
    pub places: Vec<String>,
}

fn manifest_text() -> String {
    let relay = crate::settings::hook_exe_path().to_string_lossy().to_string();
    serde_json::to_string_pretty(&manifest(&relay)).unwrap_or_default()
}

// ── Windows: one manifest, a registry value per browser ───────────────────────

/// Browsers' native messaging keys, under HKEY_CURRENT_USER.
#[cfg(windows)]
const KEYS: &[&str] = &[
    r"Software\Google\Chrome\NativeMessagingHosts",
    r"Software\Microsoft\Edge\NativeMessagingHosts",
    r"Software\BraveSoftware\Brave-Browser\NativeMessagingHosts",
    r"Software\Chromium\NativeMessagingHosts",
];

#[cfg(windows)]
fn manifest_path() -> PathBuf {
    crate::settings::local_dir().join("NativeMessaging").join(format!("{HOST}.json"))
}

#[cfg(windows)]
mod reg {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegDeleteKeyW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// The key's default value, if it has one.
    pub fn read(key: &str) -> Option<String> {
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        let ok = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, PCWSTR(wide(key).as_ptr()), PCWSTR::null(), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
        };
        if ok.is_err() {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..len.min(buf.len())]))
    }

    /// Sets the key's default value, making the key if needed.
    pub fn write(key: &str, value: &str) -> Result<(), String> {
        let data = wide(value);
        unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, PCWSTR(wide(key).as_ptr()), PCWSTR::null(), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32)
        }
        .ok()
        .map_err(|e| e.to_string())
    }

    pub fn delete(key: &str) {
        unsafe {
            let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(wide(key).as_ptr()));
        }
    }
}

#[cfg(windows)]
fn status() -> BridgeStatus {
    let manifest = manifest_path().to_string_lossy().to_string();
    let installed = manifest_path().is_file()
        && KEYS.iter().any(|k| reg::read(&format!(r"{k}\{HOST}")).is_some_and(|v| v.eq_ignore_ascii_case(&manifest)));
    let mut places = vec![manifest.clone()];
    places.extend(KEYS.iter().map(|k| format!(r"HKEY_CURRENT_USER\{k}\{HOST}")));
    BridgeStatus { installed, places }
}

#[cfg(windows)]
fn set(on: bool) -> Result<(), String> {
    let path = manifest_path();
    if on {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, manifest_text()).map_err(|e| e.to_string())?;
        let value = path.to_string_lossy().to_string();
        for key in KEYS {
            reg::write(&format!(r"{key}\{HOST}"), &value)?;
        }
    } else {
        for key in KEYS {
            reg::delete(&format!(r"{key}\{HOST}"));
        }
        let _ = std::fs::remove_file(&path);
    }
    Ok(())
}

// ── Linux: the manifest in each browser's folder ──────────────────────────────

/// Browsers' config folders under ~/.config, and their hosts' folder in each.
#[cfg(target_os = "linux")]
const FOLDERS: &[&str] = &["google-chrome", "chromium", "BraveSoftware/Brave-Browser", "microsoft-edge"];

/// The manifest's place for each browser that is there (its folder exists).
#[cfg(target_os = "linux")]
fn linux_places() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    FOLDERS
        .iter()
        .map(|f| config.join(f))
        .filter(|dir| dir.is_dir())
        .map(|dir| dir.join("NativeMessagingHosts").join(format!("{HOST}.json")))
        .collect()
}

#[cfg(target_os = "linux")]
fn status() -> BridgeStatus {
    let places = linux_places();
    let installed = places.iter().any(|p| p.is_file());
    BridgeStatus { installed, places: places.iter().map(|p| p.to_string_lossy().to_string()).collect() }
}

#[cfg(target_os = "linux")]
fn set(on: bool) -> Result<(), String> {
    for path in linux_places() {
        if on {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&path, manifest_text()).map_err(|e| e.to_string())?;
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
fn status() -> BridgeStatus {
    BridgeStatus { installed: false, places: Vec::new() }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn set(_on: bool) -> Result<(), String> {
    Err("not on this system".into())
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Whether the browsers know the bridge, and the places involved.
#[tauri::command]
pub fn browser_bridge_status() -> BridgeStatus {
    status()
}

/// Connects the bridge (`on`) or takes it out, once the user has seen the places.
#[tauri::command]
pub fn browser_bridge_set(on: bool) -> Result<BridgeStatus, String> {
    set(on)?;
    crate::log::line(format!("notifications: browser bridge {}", if on { "connected" } else { "removed" }));
    Ok(status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_names_the_relay_and_only_ciel_for_chrome() {
        let m = manifest(r"C:\Users\me\AppData\Local\Ciel\bin\ciel-hook.exe");
        assert_eq!(m["name"], HOST);
        assert_eq!(m["type"], "stdio");
        assert_eq!(m["path"], r"C:\Users\me\AppData\Local\Ciel\bin\ciel-hook.exe");
        assert_eq!(m["allowed_origins"], json!([format!("chrome-extension://{EXTENSION_ID}/")]));
    }

    #[test]
    fn the_names_match_the_extension() {
        let manifest: Value = serde_json::from_str(include_str!("../../extension/manifest.json")).unwrap();
        assert!(manifest["permissions"].as_array().unwrap().contains(&json!("nativeMessaging")));
        assert!(include_str!("../../extension/sw.js").contains(&format!("\"{HOST}\"")));
        // A host name is lower-case letters, digits, dots and underscores.
        assert!(HOST.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_'));
        // The id Chrome derives from the manifest's key.
        use sha2::Digest;
        let der = base64_decode(manifest["key"].as_str().unwrap());
        let hash = sha2::Sha256::digest(&der);
        let id: String = hash[..16].iter().flat_map(|b| [b >> 4, b & 15]).map(|n| (b'a' + n) as char).collect();
        assert_eq!(id, EXTENSION_ID);
    }

    fn base64_decode(s: &str) -> Vec<u8> {
        const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let (mut acc, mut bits) = (0u32, 0);
        for c in s.bytes().filter(|c| *c != b'=') {
            acc = (acc << 6) | A.iter().position(|a| *a == c).unwrap() as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        out
    }
}
