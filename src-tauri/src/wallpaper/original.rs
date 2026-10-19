// What was on the desktop and the lock screen before Ciel's first change, so
// wallpaper_restore can put it back exactly. It lives in
// `<data dir>/wallpaper/original/`: the record (original.json) and copies of
// the pictures themselves, because the folder a picture sits in may move or be
// emptied (OneDrive, Downloads) while Ciel's pictures are showing.
//
// The record is the "saved" marker: a section is written before the first
// change of its kind and removed only once that kind was put back.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A picture the user had: where it was, and the name of Ciel's copy of it in
/// `original/` (none when there was no file to copy, a plain colour say).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Picture {
    pub path: String,
    pub copy: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MonitorPicture {
    /// The monitor's device path (Windows).
    pub id: String,
    #[serde(flatten)]
    pub picture: Picture,
}

/// A desktop setting as the system printed it, to be handed back as it was
/// (Linux: `gsettings get` output, GVariant text).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Setting {
    pub schema: String,
    pub key: String,
    pub value: String,
    /// The picture the value names, if it names one.
    pub picture: Option<Picture>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Slideshow {
    /// The folders or pictures it went through.
    pub items: Vec<String>,
    pub options: u32,
    pub tick: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Desktop {
    /// Windows: each monitor's picture, by device path.
    pub monitors: Vec<MonitorPicture>,
    /// Windows: DESKTOP_WALLPAPER_POSITION (Fill, Fit, Span…).
    pub position: Option<i32>,
    /// Windows: the colour around or instead of the picture (COLORREF).
    pub background_color: Option<u32>,
    pub slideshow: Option<Slideshow>,
    /// Windows: HKCU\Control Panel\Desktop values (WallPaper, WallpaperStyle,
    /// TileWallpaper), as they were.
    pub registry: BTreeMap<String, String>,
    /// Linux (GNOME): the background settings.
    pub settings: Vec<Setting>,
    /// Linux (KDE): the picture Plasma showed.
    pub plasma: Option<Picture>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LockScreen {
    /// Windows: LockScreen.OriginalImageFile, as a URI.
    pub uri: String,
    pub picture: Option<Picture>,
    /// Linux (GNOME): the screensaver settings.
    pub settings: Vec<Setting>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Original {
    pub desktop: Option<Desktop>,
    pub lock_screen: Option<LockScreen>,
}

pub fn folder(dir: &Path) -> PathBuf {
    dir.join("original")
}

fn record_path(dir: &Path) -> PathBuf {
    folder(dir).join("original.json")
}

pub fn load(dir: &Path) -> Option<Original> {
    let bytes = std::fs::read(record_path(dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes the record, or removes it once nothing is left in it.
pub fn save(dir: &Path, original: &Original) -> std::io::Result<()> {
    let path = record_path(dir);
    if original.desktop.is_none() && original.lock_screen.is_none() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let json = serde_json::to_vec_pretty(original).map_err(std::io::Error::other)?;
    super::write_atomic(&path, &json)
}

/// Copies the picture at `source` into `original/` as `<stem>-<n>.<ext>` and
/// returns that name. The same file kept twice (two monitors showing one
/// picture) is copied once.
pub fn keep_copy(dir: &Path, source: &Path, stem: &str, kept: &mut Vec<(PathBuf, String)>) -> std::io::Result<String> {
    if let Some((_, name)) = kept.iter().find(|(p, _)| p == source) {
        return Ok(name.clone());
    }
    let bytes = std::fs::read(source)?;
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| "img".into());
    let name = keep_bytes(dir, &bytes, stem, &ext)?;
    kept.push((source.to_path_buf(), name.clone()));
    Ok(name)
}

/// Writes `bytes` into `original/` under a name no other copy has.
pub fn keep_bytes(dir: &Path, bytes: &[u8], stem: &str, ext: &str) -> std::io::Result<String> {
    let folder = folder(dir);
    crate::platform::ensure_private_dir(&folder)?;
    let mut n = 0;
    let name = loop {
        let name = format!("{stem}-{n}.{ext}");
        if !folder.join(&name).exists() {
            break name;
        }
        n += 1;
    };
    super::write_atomic(&folder.join(&name), bytes)?;
    Ok(name)
}

/// The file to put back for `picture`: where it was, if it is still there as
/// it was; else Ciel's copy; else where it was, changed or not.
pub fn source_for(dir: &Path, picture: &Picture) -> Option<PathBuf> {
    let at = (!picture.path.is_empty()).then(|| PathBuf::from(&picture.path)).filter(|p| p.is_file());
    let copy = picture.copy.as_ref().map(|c| folder(dir).join(c)).filter(|p| p.is_file());
    match (at, copy) {
        (Some(at), Some(copy)) => {
            let same = std::fs::read(&at).ok().zip(std::fs::read(&copy).ok()).is_some_and(|(a, b)| a == b);
            Some(if same { at } else { copy })
        }
        (at, copy) => copy.or(at),
    }
}

/// Removes the copies in `original/` that nothing needs any more (the
/// record is gone, and these were not the ones put back).
pub fn forget_copies(dir: &Path, keep: &[PathBuf]) {
    let Ok(entries) = std::fs::read_dir(folder(dir)) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_record = path.file_name().is_some_and(|n| n == "original.json");
        if !is_record && path.is_file() && !keep.iter().any(|k| k == &path) {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// `file:///C:/Users/a%20b/x.png` → `C:\Users\a b\x.png`; `file://host/share/x`
/// → `\\host\share\x` (Windows); `file:///home/a/x.png` → `/home/a/x.png`.
pub fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://").or_else(|| uri.strip_prefix("FILE://"))?;
    let decoded = super::percent_decode(rest);
    if cfg!(windows) {
        let decoded = decoded.strip_prefix("localhost").unwrap_or(&decoded);
        // `/C:/…` is a local path; anything else names a host (a share).
        let path = match decoded.strip_prefix('/') {
            Some(local) => local.to_string(),
            None => format!("//{decoded}"),
        };
        Some(PathBuf::from(path.replace('/', "\\")))
    } else {
        decoded.starts_with('/').then(|| PathBuf::from(decoded))
    }
}

/// `path` as a `file://` URI, as GNOME stores pictures.
#[cfg_attr(windows, allow(dead_code))]
pub fn file_uri(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ciel-wallpaper-original-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_record_survives_a_save_and_goes_once_empty() {
        let dir = scratch("record");
        assert!(load(&dir).is_none());
        let mut original = Original {
            desktop: Some(Desktop {
                monitors: vec![MonitorPicture {
                    id: r"\\?\DISPLAY#A".into(),
                    picture: Picture { path: r"C:\Pictures\space.jpg".into(), copy: Some("desktop-0.jpg".into()) },
                }],
                position: Some(4),
                background_color: Some(0),
                registry: [("WallPaper".to_string(), r"C:\Pictures\space.jpg".to_string())].into(),
                ..Default::default()
            }),
            lock_screen: None,
        };
        save(&dir, &original).unwrap();
        assert_eq!(load(&dir), Some(original.clone()));
        original.desktop = None;
        save(&dir, &original).unwrap();
        assert!(load(&dir).is_none());
        assert!(!record_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_comes_back_from_where_it_was_unless_it_changed_or_went() {
        let dir = scratch("source");
        let mut kept = Vec::new();
        let own = dir.join("mine.jpg");
        std::fs::write(&own, b"the user's picture").unwrap();
        let copy = keep_copy(&dir, &own, "desktop", &mut kept).unwrap();
        assert_eq!(copy, "desktop-0.jpg");
        // The same file kept again is not copied twice.
        assert_eq!(keep_copy(&dir, &own, "desktop", &mut kept).unwrap(), copy);
        let picture = Picture { path: own.to_string_lossy().into(), copy: Some(copy.clone()) };

        assert_eq!(source_for(&dir, &picture), Some(own.clone()));
        std::fs::write(&own, b"something else since").unwrap();
        assert_eq!(source_for(&dir, &picture), Some(folder(&dir).join(&copy)));
        std::fs::remove_file(&own).unwrap();
        assert_eq!(source_for(&dir, &picture), Some(folder(&dir).join(&copy)));
        // Nothing at all: nothing to put back.
        assert_eq!(source_for(&dir, &Picture { path: String::new(), copy: None }), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unused_copies_are_removed_and_the_kept_ones_stay() {
        let dir = scratch("forget");
        let a = keep_bytes(&dir, b"a", "desktop", "jpg").unwrap();
        let b = keep_bytes(&dir, b"b", "desktop", "jpg").unwrap();
        assert_ne!(a, b);
        forget_copies(&dir, &[folder(&dir).join(&b)]);
        assert!(!folder(&dir).join(&a).exists());
        assert!(folder(&dir).join(&b).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_uris_become_paths() {
        if cfg!(windows) {
            assert_eq!(
                path_from_file_uri("file:///C:/Users/alex/Pictures/Lock%20Screen/x.png"),
                Some(PathBuf::from(r"C:\Users\alex\Pictures\Lock Screen\x.png"))
            );
            assert_eq!(path_from_file_uri("file://server/share/x.jpg"), Some(PathBuf::from(r"\\server\share\x.jpg")));
        } else {
            assert_eq!(path_from_file_uri("file:///home/a/My%20Pictures/x.png"), Some(PathBuf::from("/home/a/My Pictures/x.png")));
            assert_eq!(file_uri(Path::new("/home/a/My Pictures/x.png")), "file:///home/a/My%20Pictures/x.png");
        }
        assert_eq!(path_from_file_uri("ms-appdata:///local/x.png"), None);
    }
}
