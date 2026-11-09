// The buttons at the top of Tools → Controls, as a desktop's quick settings
// have them: a screenshot, the system's settings (or one page of them), lock,
// and the power menu (suspend, restart, power off, log out). And the user's
// own apps beside them: GNOME's dock favourites on Linux, the apps pinned to
// the taskbar on Windows, with their icons, opened in one click.
//
// The island asks the user to click twice before restart, power off and log
// out; here they are done at once, the desktop's own way (GNOME lets the apps
// save first).

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    /// The .desktop id on Linux, the shortcut's path on Windows.
    pub id: String,
    pub name: String,
    /// A data: URL, or None when no icon was found.
    pub icon: Option<String>,
}

/// Icons bigger than this are left out: the row shows them at 26 px.
#[cfg(target_os = "linux")]
const MAX_ICON: usize = 400 * 1024;

#[cfg(target_os = "linux")]
fn data_url(path: &std::path::Path) -> Option<String> {
    use base64::Engine;
    let kind = match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        _ => return None,
    };
    let bytes = std::fs::read(path).ok().filter(|b| b.len() <= MAX_ICON)?;
    Some(format!("data:{kind};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// "screenshot", "settings", "soundSettings", "networkSettings",
/// "powerSettings", "lock", "suspend", "restart", "powerOff", "logOut".
#[tauri::command]
pub async fn system_action(what: String) -> bool {
    let done = tauri::async_runtime::spawn_blocking(move || platform::act(&what).then_some(()).ok_or(what))
        .await
        .unwrap_or(Err(String::new()));
    if let Err(what) = &done {
        crate::log::line(format!("quick: could not {what}"));
    }
    done.is_ok()
}

#[tauri::command]
pub async fn launcher_apps() -> Vec<App> {
    tauri::async_runtime::spawn_blocking(platform::apps).await.unwrap_or_default()
}

/// Opens one of the apps listed: only those, never any path the page sends.
#[tauri::command]
pub async fn launcher_open(id: String) -> bool {
    tauri::async_runtime::spawn_blocking(move || platform::apps().iter().any(|a| a.id == id) && platform::open(&id))
        .await
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
mod platform {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use gtk::gio::prelude::{AppInfoExt, Cast, FileExt};
    use gtk::gio::{AppLaunchContext, DesktopAppInfo, FileIcon, ThemedIcon};

    fn ok(cmd: &str, args: &[&str]) -> bool {
        Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Started and left to run (a window that stays open).
    fn spawn(cmd: &str, args: &[&str]) -> bool {
        Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok()
    }

    fn has(cmd: &str) -> bool {
        crate::platform::find_on_path(cmd).is_some()
    }

    /// GNOME's Settings on a page, else KDE's or Xfce's.
    fn settings(page: &str) -> bool {
        if has("gnome-control-center") {
            return if page.is_empty() { spawn("gnome-control-center", &[]) } else { spawn("gnome-control-center", &[page]) };
        }
        spawn("systemsettings", &[]) || spawn("xfce4-settings-manager", &[])
    }

    pub fn act(what: &str) -> bool {
        // GNOME ends the session itself, letting the apps save first.
        let gnome_quit = |flag: &str| has("gnome-session-quit") && ok("gnome-session-quit", &[flag, "--no-prompt"]);
        match what {
            "screenshot" => {
                crate::gnome_ext::screenshot()
                    || crate::flow::os::send_print()
                    || (has("gnome-screenshot") && spawn("gnome-screenshot", &["--interactive"]))
                    || (has("spectacle") && spawn("spectacle", &[]))
                    || (has("flameshot") && spawn("flameshot", &["gui"]))
            }
            "settings" => settings(""),
            "soundSettings" => settings("sound"),
            "networkSettings" => settings("network"),
            "powerSettings" => settings("power"),
            "lock" => ok("loginctl", &["lock-session"]),
            "suspend" => ok("systemctl", &["suspend"]),
            "restart" => gnome_quit("--reboot") || ok("systemctl", &["reboot"]),
            "powerOff" => gnome_quit("--power-off") || ok("systemctl", &["poweroff"]),
            "logOut" => gnome_quit("--logout") || ok("loginctl", &["terminate-user", &std::env::var("USER").unwrap_or_default()]),
            _ => false,
        }
    }

    fn gsettings(args: &[&str]) -> Option<String> {
        let out = Command::new("gsettings").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Where icon themes live: the user's data folder, then the system's.
    fn data_dirs() -> Vec<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let mut dirs = vec![
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share")),
        ];
        let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
        dirs
    }

    /// The icon theme and those it inherits from, then hicolor, every theme's last resort.
    fn themes(dirs: &[PathBuf]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut next = gsettings(&["get", "org.gnome.desktop.interface", "icon-theme"]).map(|t| t.trim_matches('\'').to_string());
        while let Some(theme) = next.take() {
            if theme.is_empty() || out.contains(&theme) || out.len() > 6 {
                break;
            }
            let index = dirs.iter().find_map(|d| std::fs::read_to_string(d.join("icons").join(&theme).join("index.theme")).ok());
            out.push(theme);
            next = index.and_then(|i| {
                i.lines().find_map(|l| l.strip_prefix("Inherits=")).and_then(|v| v.split(',').next()).map(|t| t.trim().to_string())
            });
        }
        if !out.iter().any(|t| t == "hicolor") {
            out.push("hicolor".into());
        }
        out
    }

    /// Sharp at 26 px on a screen at twice the density, then anything.
    const SIZES: [&str; 8] = ["48x48@2x", "64x64", "96x96", "128x128", "256x256", "scalable", "48x48", "32x32"];

    fn find_icon(name: &str, dirs: &[PathBuf], themes: &[String]) -> Option<PathBuf> {
        for theme in themes {
            for dir in dirs {
                let base = dir.join("icons").join(theme);
                if !base.is_dir() {
                    continue;
                }
                for size in SIZES {
                    for ext in ["svg", "png"] {
                        let p = base.join(size).join("apps").join(format!("{name}.{ext}"));
                        if p.is_file() {
                            return Some(p);
                        }
                    }
                }
            }
        }
        ["png", "svg"].iter().map(|ext| Path::new("/usr/share/pixmaps").join(format!("{name}.{ext}"))).find(|p| p.is_file())
    }

    fn icon_of(info: &DesktopAppInfo, dirs: &[PathBuf], themes: &[String]) -> Option<String> {
        let icon = info.icon()?;
        if let Some(file) = icon.downcast_ref::<FileIcon>() {
            return super::data_url(&file.file().path()?);
        }
        let themed = icon.downcast_ref::<ThemedIcon>()?;
        themed.names().iter().find_map(|n| find_icon(n, dirs, themes)).and_then(|p| super::data_url(&p))
    }

    /// GNOME's dock favourites, in the dock's order.
    pub fn apps() -> Vec<super::App> {
        let Some(list) = gsettings(&["get", "org.gnome.shell", "favorite-apps"]) else { return Vec::new() };
        let dirs = data_dirs();
        let themes = themes(&dirs);
        list.split('\'')
            .skip(1)
            .step_by(2)
            .filter_map(|id| {
                let info = DesktopAppInfo::new(id)?;
                Some(super::App { id: id.to_string(), name: info.display_name().to_string(), icon: icon_of(&info, &dirs, &themes) })
            })
            .collect()
    }

    pub fn open(id: &str) -> bool {
        DesktopAppInfo::new(id).is_some_and(|info| info.launch(&[], None::<&AppLaunchContext>).is_ok())
    }
}

#[cfg(windows)]
mod platform {
    use std::path::PathBuf;
    use std::process::Command;

    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::System::Power::SetSuspendState;
    use windows::Win32::UI::Shell::{ShellExecuteW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, SW_SHOWNORMAL};

    fn open_uri(uri: &str) -> bool {
        crate::platform::open_url(uri);
        true
    }

    fn run(cmd: &str, args: &[&str]) -> bool {
        crate::platform::no_console(Command::new(cmd).args(args)).spawn().is_ok()
    }

    pub fn act(what: &str) -> bool {
        match what {
            // The Snipping Tool's overlay, as Win+Shift+S opens it.
            "screenshot" => open_uri("ms-screenclip:"),
            "settings" => open_uri("ms-settings:"),
            "soundSettings" => open_uri("ms-settings:sound"),
            // Windows' own list of networks, beside the taskbar.
            "networkSettings" => open_uri("ms-availablenetworks:"),
            "powerSettings" => open_uri("ms-settings:powersleep"),
            "lock" => run("rundll32.exe", &["user32.dll,LockWorkStation"]),
            "suspend" => unsafe { SetSuspendState(false, false, false) },
            "restart" => run("shutdown.exe", &["/r", "/t", "0"]),
            "powerOff" => run("shutdown.exe", &["/s", "/t", "0"]),
            "logOut" => run("shutdown.exe", &["/l"]),
            _ => false,
        }
    }

    /// The shortcuts of the apps pinned to the taskbar.
    fn pinned() -> Vec<PathBuf> {
        let Some(appdata) = std::env::var_os("APPDATA") else { return Vec::new() };
        let dir = PathBuf::from(appdata).join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar");
        let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"))).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    /// The shortcut's icon as Explorer shows it, 32 px, as a PNG.
    // ponytail: 32 px (SHGFI_LARGEICON); the system image list's 48 px is sharper on a dense screen.
    fn icon_of(path: &PathBuf) -> Option<String> {
        let mut info = SHFILEINFOW::default();
        let wide = HSTRING::from(path.as_os_str());
        let got = unsafe {
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_LARGEICON,
            )
        };
        if got == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let png = png_of(info.hIcon);
        unsafe {
            let _ = DestroyIcon(info.hIcon);
        }
        png
    }

    fn png_of(icon: HICON) -> Option<String> {
        use base64::Engine;
        let mut ii = ICONINFO::default();
        unsafe { GetIconInfo(icon, &mut ii) }.ok()?;
        let free = |ii: &ICONINFO| unsafe {
            let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        };
        let mut bm = BITMAP::default();
        let size = std::mem::size_of::<BITMAP>() as i32;
        if ii.hbmColor.is_invalid() || unsafe { GetObjectW(HGDIOBJ(ii.hbmColor.0), size, Some(&mut bm as *mut BITMAP as _)) } == 0 {
            free(&ii);
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut header = BITMAPINFO::default();
        header.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            // Negative: rows from the top.
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let dc = unsafe { CreateCompatibleDC(None) };
        let rows = unsafe { GetDIBits(dc, ii.hbmColor, 0, h as u32, Some(pixels.as_mut_ptr() as _), &mut header, DIB_RGB_COLORS) };
        unsafe {
            let _ = DeleteDC(dc);
        }
        free(&ii);
        if rows == 0 {
            return None;
        }
        // BGRA to RGBA; an old icon without alpha is opaque.
        let opaque = pixels.chunks(4).all(|p| p[3] == 0);
        for p in pixels.chunks_mut(4) {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        let image = image::RgbaImage::from_raw(w as u32, h as u32, pixels)?;
        let mut png = Vec::new();
        image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
        Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
    }

    pub fn apps() -> Vec<super::App> {
        pinned()
            .into_iter()
            .map(|p| super::App {
                id: p.display().to_string(),
                name: p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
                icon: icon_of(&p),
            })
            .collect()
    }

    pub fn open(id: &str) -> bool {
        let file = HSTRING::from(id);
        let done = unsafe { ShellExecuteW(None, windows::core::w!("open"), PCWSTR(file.as_ptr()), None, None, SW_SHOWNORMAL) };
        // Above 32: it opened.
        done.0 as isize > 32
    }
}
