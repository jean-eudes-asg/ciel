// The shelf (Tools → Shelf): the files dropped on the island, kept in Ciel's
// inbox for a week (files.rs), to drag out again into any app, open, show in
// their folder or copy.
//
// Only files in the inbox are ever touched here: every path the page sends
// is checked to be one of them, so a script in the page can't reach others.
// Dragging out is Windows' own drag (SHDoDragDrop, on the window's thread);
// on Linux the page drags them itself (text/uri-list, which WebKitGTK hands
// to the desktop).

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use tauri::AppHandle;

use crate::files::inbox_dir;

/// Pictures up to this size come with a preview.
const PREVIEW_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ShelfFile {
    pub name: String,
    pub path: String,
    pub size: u64,
    /// Unix ms: when it was dropped.
    pub at: f64,
    /// A picture's preview, as a data URL.
    pub preview: Option<String>,
}

fn mime_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// `path`, if it is a file in the inbox.
fn in_inbox(path: &str) -> Option<PathBuf> {
    let inbox = inbox_dir().canonicalize().ok()?;
    let file = Path::new(path).canonicalize().ok()?;
    (file.parent() == Some(inbox.as_path()) && file.is_file()).then_some(file)
}

/// What the shelf holds, newest first.
#[tauri::command]
pub fn shelf_list() -> Vec<ShelfFile> {
    use base64::Engine;
    let Ok(entries) = std::fs::read_dir(inbox_dir()) else { return Vec::new() };
    let mut out: Vec<ShelfFile> = entries
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let at = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64() * 1000.0;
            let preview = mime_of(&name).filter(|_| meta.len() <= PREVIEW_LIMIT).and_then(|mime| {
                let bytes = std::fs::read(e.path()).ok()?;
                Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
            });
            Some(ShelfFile { name, path: e.path().to_string_lossy().to_string(), size: meta.len(), at, preview })
        })
        .collect();
    out.sort_by(|a, b| b.at.total_cmp(&a.at));
    out
}

#[tauri::command]
pub fn shelf_remove(paths: Vec<String>) -> usize {
    paths.iter().filter_map(|p| in_inbox(p)).filter(|p| std::fs::remove_file(p).is_ok()).count()
}

#[tauri::command]
pub fn shelf_open(path: String) -> bool {
    let Some(file) = in_inbox(&path) else { return false };
    crate::platform::open_url(&file.to_string_lossy());
    true
}

/// The file, selected in its folder.
#[tauri::command]
pub fn shelf_reveal(path: String) -> bool {
    let Some(file) = in_inbox(&path) else { return false };
    platform::reveal(&file);
    true
}

/// The files on the clipboard, to paste in a folder or a mail.
#[tauri::command]
pub fn shelf_copy(paths: Vec<String>) -> bool {
    let files: Vec<PathBuf> = paths.iter().filter_map(|p| in_inbox(p)).collect();
    if files.is_empty() {
        return false;
    }
    arboard::Clipboard::new().and_then(|mut b| b.set().file_list(&files)).is_ok()
}

/// Drags the files out of the island, while the button that started it is down.
#[tauri::command]
pub fn shelf_drag(app: AppHandle, paths: Vec<String>) -> bool {
    let files: Vec<PathBuf> = paths.iter().filter_map(|p| in_inbox(p)).collect();
    if files.is_empty() {
        return false;
    }
    platform::drag(&app, files)
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use tauri::AppHandle;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, ILCreateFromPathW, ILFree, IShellItemArray, SHCreateShellItemArrayFromIDLists, SHDoDragDrop,
        SHOpenFolderAndSelectItems,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub fn reveal(file: &Path) {
        let name = wide(file);
        unsafe {
            let pidl = ILCreateFromPathW(PCWSTR(name.as_ptr()));
            if pidl.is_null() {
                return;
            }
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            ILFree(Some(pidl));
        }
    }

    pub fn drag(app: &AppHandle, files: Vec<PathBuf>) -> bool {
        let Some(win) = crate::island::window(app) else { return false };
        let Ok(hwnd) = win.hwnd() else { return false };
        let hwnd = hwnd.0 as isize;
        // OLE's drag runs a loop of its own on the window's thread until the button goes up.
        app.run_on_main_thread(move || unsafe {
            let names: Vec<Vec<u16>> = files.iter().map(|f| wide(f)).collect();
            let pidls: Vec<*mut ITEMIDLIST> = names.iter().map(|n| ILCreateFromPathW(PCWSTR(n.as_ptr()))).filter(|p| !p.is_null()).collect();
            if pidls.is_empty() {
                return;
            }
            let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| *p as *const ITEMIDLIST).collect();
            let data: Option<IDataObject> = SHCreateShellItemArrayFromIDLists(&list)
                .ok()
                .and_then(|items: IShellItemArray| items.BindToHandler(None, &BHID_DataObject).ok());
            if let Some(data) = data {
                let _ = SHDoDragDrop(
                    Some(windows::Win32::Foundation::HWND(hwnd as *mut _)),
                    &data,
                    None,
                    DROPEFFECT_COPY | DROPEFFECT_LINK,
                );
            }
            for p in pidls {
                ILFree(Some(p));
            }
        })
        .is_ok()
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::{Path, PathBuf};

    use tauri::AppHandle;

    /// The file manager's own ShowItems, else its folder.
    pub fn reveal(file: &Path) {
        let uri = format!("file://{}", file.to_string_lossy());
        let shown = dbus::blocking::Connection::new_session().ok().is_some_and(|conn| {
            conn.with_proxy("org.freedesktop.FileManager1", "/org/freedesktop/FileManager1", std::time::Duration::from_secs(2))
                .method_call::<(), _, _, _>("org.freedesktop.FileManager1", "ShowItems", (vec![uri.as_str()], ""))
                .is_ok()
        });
        if !shown {
            if let Some(dir) = file.parent() {
                crate::platform::reveal_folder(&dir.to_string_lossy());
            }
        }
    }

    /// The page drags them itself on Linux.
    pub fn drag(_: &AppHandle, _: Vec<PathBuf>) -> bool {
        false
    }
}
