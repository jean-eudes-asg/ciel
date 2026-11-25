// The dock: a bar at the bottom of the screen with what the Windows taskbar
// holds, in Ciel's own look (the page: src/dock): Start, task view and widgets,
// the pinned apps and the ones open (a dot under those, a preview of
// their windows on hover, a menu on right click), Ciel's agents, what plays,
// the sound, the network and the battery, the time, and show desktop. Off by
// default, in Settings → Island → Dock. Windows only for now: GNOME has a dock
// of its own, and its Wayland lets no app place a window.
//
// Nothing runs while it is off. While it shows, a hidden window of its own
// (win.rs) waits in GetMessage: Explorer tells it when a window opens,
// closes, comes to the front or flashes (a shell hook), when a full-screen app
// starts (the dock is an app bar, and steps aside), and when Explorer starts
// again; the speakers, the network and the battery tell it when they change.
// It can hide the Windows taskbar and take its place (taskbar.rs).
//
// Its pins are the taskbar's until the user pins, unpins or moves one on the
// dock (Windows lets no app pin to its taskbar): from then on, the dock's own
// list, in Settings (DockPrefs::pins).
//
// The page sends back the id of the app clicked: only ids from the list made
// here are acted on, never a path or a window the page names.

// The row's logic is Windows' for now; elsewhere only its tests use it.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;
use tauri::AppHandle;

#[cfg(windows)]
mod preview;
#[cfg(windows)]
mod taskbar;
#[cfg(windows)]
mod win;

pub const LABEL: &str = "dock";
/// The bar's height and the space under it, in logical pixels.
pub const BAR_H: f64 = 52.0;
pub const GAP: f64 = 8.0;
/// Room in the window above the bar and on each side of it, where the icons
/// grow under the pointer. Clicks go through it to what is behind, but while
/// the pointer is on the bar (a window region, `region`).
pub const HEAD: f64 = 60.0;
pub const SIDE: f64 = 110.0;
/// The bar's corners, as dock.css draws them.
pub const RADIUS: f64 = 16.0;
/// What the page hears, whole, on every change.
pub const STATE_EVENT: &str = "dock-state";
/// The corner's icons alone, as the volume moves (without the row's icons).
pub const SYSTEM_EVENT: &str = "dock-system";

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DockApp {
    /// The pinned shortcut's or program's path, or the program's for an app only open.
    pub id: String,
    pub name: String,
    /// A data: URL, or none when Windows has no icon for it.
    pub icon: Option<String>,
    pub pinned: bool,
    /// How many of its windows are open.
    pub windows: u32,
    /// One of its windows is the one in front.
    pub active: bool,
    /// One of its windows flashes, asking to be looked at.
    pub attention: bool,
}

/// The corner of the bar, as the taskbar's: the speakers, the network, the battery.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemIcons {
    /// 0…1; none without speakers.
    pub volume: Option<f64>,
    pub muted: bool,
    /// "wifi", "wired", "mobile", or "" when not connected.
    pub network: String,
    /// The Wi-Fi's name.
    pub network_name: String,
    /// 0…100; none without a battery.
    pub battery: Option<f64>,
    pub charging: bool,
}

/// What the page draws.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DockState {
    pub apps: Vec<DockApp>,
    /// The Windows taskbar is hidden: the dock offers to show it for a moment
    /// (its notification area and the icons there).
    pub taskbar_hidden: bool,
    /// On screen; false while off or out of a full-screen app's way, when the
    /// page lets its clock rest.
    pub shown: bool,
    pub system: SystemIcons,
}

/// A pin: a shortcut, or a program pinned from the dock.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// Its path: the app's id.
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    /// The program it starts, its path in lowercase; none for a store app.
    pub target: Option<String>,
}

/// A program with windows open.
#[derive(Clone, Debug, PartialEq)]
pub struct Open {
    /// Its path in lowercase, the key pins are matched on.
    pub key: String,
    /// Its path as Windows gives it: the id when no pin starts it.
    pub path: String,
    pub name: String,
    pub icon: Option<String>,
    /// Its windows, the one nearest the front first.
    pub windows: Vec<isize>,
    pub active: bool,
    pub attention: bool,
}

/// One app of the row, with what Rust keeps to act on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub app: DockApp,
    pub windows: Vec<isize>,
    /// What a click opens when none of its windows is open: the shortcut, or
    /// the program for an app that is only open. Also what pinning keeps.
    pub launch: String,
}

/// The row: the pins in their order, each with the windows of the program it
/// starts; then the programs open that no pin starts, in the order they were
/// first seen (`order`, kept from one call to the next), so nothing moves when
/// another window comes to the front. Two pins of the same program (a
/// browser's web apps): the first has its windows.
pub fn merge(pins: &[Pin], open: &[Open], order: &mut Vec<String>) -> Vec<Entry> {
    order.retain(|key| open.iter().any(|o| &o.key == key));
    for o in open {
        if !order.contains(&o.key) {
            order.push(o.key.clone());
        }
    }
    let mut taken: Vec<&str> = Vec::new();
    let mut out: Vec<Entry> = pins
        .iter()
        .map(|pin| {
            let found = pin
                .target
                .as_deref()
                .filter(|t| !taken.contains(t))
                .and_then(|t| open.iter().find(|o| o.key == t));
            if let Some(o) = found {
                taken.push(o.key.as_str());
            }
            Entry {
                app: DockApp {
                    id: pin.id.clone(),
                    name: pin.name.clone(),
                    icon: pin.icon.clone().or_else(|| found.and_then(|o| o.icon.clone())),
                    pinned: true,
                    windows: found.map_or(0, |o| o.windows.len() as u32),
                    active: found.is_some_and(|o| o.active),
                    attention: found.is_some_and(|o| o.attention),
                },
                windows: found.map(|o| o.windows.clone()).unwrap_or_default(),
                launch: pin.id.clone(),
            }
        })
        .collect();
    for key in order.iter() {
        if taken.contains(&key.as_str()) {
            continue;
        }
        let Some(o) = open.iter().find(|o| &o.key == key) else { continue };
        out.push(Entry {
            app: DockApp {
                id: o.path.clone(),
                name: o.name.clone(),
                icon: o.icon.clone(),
                pinned: false,
                windows: o.windows.len() as u32,
                active: o.active,
                attention: o.attention,
            },
            windows: o.windows.clone(),
            launch: o.path.clone(),
        });
    }
    out
}

fn same_path(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The pins with `path` at `to` among them: moved there, or pinned there when
/// it wasn't (`to` past the end: last).
pub fn pin_at(pins: &[String], path: &str, to: usize) -> Vec<String> {
    let mut out: Vec<String> = pins.iter().filter(|p| !same_path(p, path)).cloned().collect();
    out.insert(to.min(out.len()), path.to_string());
    out
}

pub fn unpin(pins: &[String], path: &str) -> Vec<String> {
    pins.iter().filter(|p| !same_path(p, path)).cloned().collect()
}

/// What a click on an app does with its windows, the one nearest the front
/// first: none open, start it; none in front, bring the nearest forward; the
/// only one in front, minimize it; several, one in front, the one furthest
/// back comes forward (each click goes round them).
#[derive(Debug, PartialEq)]
pub enum Click {
    Launch,
    Front(isize),
    Minimize(isize),
}

pub fn click(windows: &[isize], in_front: isize) -> Click {
    match windows {
        [] => Click::Launch,
        [only] if *only == in_front => Click::Minimize(*only),
        all if all.contains(&in_front) => Click::Front(*all.last().unwrap()),
        all => Click::Front(all[0]),
    }
}

/// The volume after a turn of the wheel on the sound icon: steps of 2 %, as
/// the taskbar's, kept between 0 and 1.
pub fn volume_after(level: f64, notches: f64) -> f64 {
    let step = 0.02;
    (((level + notches * step) / step).round() * step).clamp(0.0, 1.0)
}

/// Creates the dock's window, off screen: before the island, for WebView2's
/// sake (see create_settings_window in lib.rs). Puts the taskbar back first,
/// should Ciel have stopped last time without doing it.
pub fn setup(app: &AppHandle) {
    #[cfg(windows)]
    win::setup(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// Shows or takes away the dock as Settings says (at launch, on each save).
pub fn sync(app: &AppHandle, prefs: &crate::settings::DockPrefs) {
    #[cfg(windows)]
    win::sync(app, prefs);
    #[cfg(not(windows))]
    let _ = (app, prefs);
}

/// Ciel quits: the taskbar comes back, the bottom of the screen is freed.
pub fn shutdown() {
    #[cfg(windows)]
    win::shutdown();
}

/// The dock's own pins, kept in Settings and told to every window.
#[cfg(windows)]
fn save_pins(app: &AppHandle, pins: Vec<String>) {
    use tauri::{Emitter, Manager};
    let shared = app.state::<crate::Shared>();
    let updated = {
        let mut current = shared.settings.lock().unwrap();
        current.dock.pins = Some(pins);
        if let Err(err) = crate::settings::save(&current) {
            crate::log::line(format!("could not save settings: {err}"));
        }
        current.clone()
    };
    win::sync(app, &updated.dock);
    let _ = app.emit("settings-changed", updated);
}

#[tauri::command]
pub fn dock_state() -> DockState {
    #[cfg(windows)]
    return win::state();
    #[cfg(not(windows))]
    DockState::default()
}

/// The page's bar is this wide (CSS pixels, its icons at rest): the window
/// takes its size, with the room around it.
#[tauri::command]
pub fn dock_fit(width: f64) {
    #[cfg(windows)]
    win::fit(width);
    #[cfg(not(windows))]
    let _ = width;
}

/// The pointer is on the bar (`hover`): the whole window takes clicks, for the
/// icons grown into the room above; else only the bar does.
#[tauri::command]
pub fn dock_region(hover: bool) {
    #[cfg(windows)]
    win::region(hover);
    #[cfg(not(windows))]
    let _ = hover;
}

/// A click on an app of the row; `new_window` (a middle click) starts another.
#[tauri::command]
pub async fn dock_open(id: String, new_window: bool) -> bool {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(move || win::open(&id, new_window)).await.unwrap_or(false);
    #[cfg(not(windows))]
    {
        let _ = (id, new_window);
        false
    }
}

/// The Start menu, as the Windows key opens it.
#[tauri::command]
pub fn dock_start() -> bool {
    #[cfg(windows)]
    return win::start_menu();
    #[cfg(not(windows))]
    false
}

/// One of Windows' own panels, as its keys open them: "taskView",
/// "widgets", "quickSettings", "notifications", "desktop" (show desktop).
#[tauri::command]
pub fn dock_key(what: String) -> bool {
    #[cfg(windows)]
    return win::panel(&what);
    #[cfg(not(windows))]
    {
        let _ = what;
        false
    }
}

/// The wheel turned on the sound icon, by so many notches (up is positive).
#[tauri::command]
pub async fn dock_volume(notches: f64) -> bool {
    #[cfg(windows)]
    return tauri::async_runtime::spawn_blocking(move || win::turn_volume(notches)).await.unwrap_or(false);
    #[cfg(not(windows))]
    {
        let _ = notches;
        false
    }
}

/// The menu of a right click: an app's (`id`), or the bar's.
#[tauri::command]
pub fn dock_menu(id: Option<String>) {
    #[cfg(windows)]
    win::menu(id);
    #[cfg(not(windows))]
    let _ = id;
}

/// The windows of an app (`id`), in a preview above its icon, centred on `x`
/// (CSS pixels across the dock's window); none: the preview goes, unless the
/// pointer went onto it. `light`: the dock is drawn white.
#[tauri::command]
pub fn dock_preview(id: Option<String>, x: f64, light: bool) {
    #[cfg(windows)]
    win::preview(id, x, light);
    #[cfg(not(windows))]
    let _ = (id, x, light);
}

/// An app dragged to `to` among the pins: moved there, or pinned there.
#[tauri::command]
pub fn dock_pin_move(app: AppHandle, id: String, to: usize) -> bool {
    #[cfg(windows)]
    return match win::pin_moved(&id, to) {
        Some(pins) => {
            save_pins(&app, pins);
            true
        }
        None => false,
    };
    #[cfg(not(windows))]
    {
        let _ = (app, id, to);
        false
    }
}

/// The hidden taskbar, for a moment: its notification area and the icons there.
#[tauri::command]
pub fn dock_taskbar() -> bool {
    #[cfg(windows)]
    return win::reveal_taskbar();
    #[cfg(not(windows))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(id: &str, target: Option<&str>) -> Pin {
        Pin { id: id.into(), name: id.into(), icon: None, target: target.map(Into::into) }
    }

    fn open(key: &str, windows: &[isize], active: bool) -> Open {
        Open {
            key: key.into(),
            path: key.to_uppercase(),
            name: key.into(),
            icon: Some(format!("icon:{key}")),
            windows: windows.to_vec(),
            active,
            attention: false,
        }
    }

    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.app.id.as_str()).collect()
    }

    #[test]
    fn pins_come_first_with_their_windows_then_the_other_open_apps() {
        let pins = [pin("files.lnk", Some("c:\\explorer.exe")), pin("browser.lnk", Some("c:\\browser.exe"))];
        let open = [open("c:\\editor.exe", &[7], true), open("c:\\browser.exe", &[3, 4], false)];
        let mut order = Vec::new();
        let row = merge(&pins, &open, &mut order);
        assert_eq!(ids(&row), ["files.lnk", "browser.lnk", "C:\\EDITOR.EXE"]);
        assert_eq!(row[0].app.windows, 0);
        assert_eq!(row[1].app.windows, 2);
        assert_eq!(row[1].windows, vec![3, 4]);
        assert!(row[2].app.active && !row[2].app.pinned);
        // A pin starts its shortcut, an open app its program.
        assert_eq!(row[0].launch, "files.lnk");
        assert_eq!(row[2].launch, "C:\\EDITOR.EXE");
        // A pin without an icon of its own takes its program's.
        assert_eq!(row[1].app.icon.as_deref(), Some("icon:c:\\browser.exe"));
    }

    #[test]
    fn open_apps_keep_their_place_when_another_comes_to_the_front() {
        let mut order = Vec::new();
        let first = merge(&[], &[open("a", &[1], true), open("b", &[2], false)], &mut order);
        assert_eq!(ids(&first), ["A", "B"]);
        // b comes to the front: Windows now lists it first.
        let then = merge(&[], &[open("b", &[2], true), open("a", &[1], false)], &mut order);
        assert_eq!(ids(&then), ["A", "B"]);
        // a closes, c opens: c goes last, a's place is gone.
        let later = merge(&[], &[open("c", &[5], false), open("b", &[2], true)], &mut order);
        assert_eq!(ids(&later), ["B", "C"]);
    }

    #[test]
    fn two_pins_of_one_program_give_its_windows_to_the_first() {
        let pins = [pin("mail.lnk", Some("c:\\browser.exe")), pin("browser.lnk", Some("c:\\browser.exe"))];
        let row = merge(&pins, &[open("c:\\browser.exe", &[9], false)], &mut Vec::new());
        assert_eq!(ids(&row), ["mail.lnk", "browser.lnk"]);
        assert_eq!((row[0].app.windows, row[1].app.windows), (1, 0));
    }

    #[test]
    fn a_store_app_pin_is_never_running() {
        let row = merge(&[pin("calc.lnk", None)], &[open("c:\\calc.exe", &[1], false)], &mut Vec::new());
        assert_eq!(ids(&row), ["calc.lnk", "C:\\CALC.EXE"]);
    }

    #[test]
    fn a_program_pinned_from_the_dock_takes_its_windows() {
        let pins = [pin("C:\\Tools\\Editor.exe", Some("c:\\tools\\editor.exe"))];
        let row = merge(&pins, &[open("c:\\tools\\editor.exe", &[4], true)], &mut Vec::new());
        assert_eq!(ids(&row), ["C:\\Tools\\Editor.exe"]);
        assert!(row[0].app.pinned && row[0].app.active);
    }

    #[test]
    fn a_click_starts_brings_forward_minimizes_or_goes_round() {
        assert_eq!(click(&[], 0), Click::Launch);
        assert_eq!(click(&[5], 9), Click::Front(5));
        assert_eq!(click(&[5], 5), Click::Minimize(5));
        assert_eq!(click(&[5, 6, 7], 5), Click::Front(7));
        assert_eq!(click(&[5, 6, 7], 1), Click::Front(5));
    }

    #[test]
    fn pins_move_come_and_go_whatever_the_case_of_their_path() {
        let pins: Vec<String> = ["a.lnk", "b.lnk", "c.lnk"].map(String::from).to_vec();
        assert_eq!(pin_at(&pins, "c.lnk", 0), ["c.lnk", "a.lnk", "b.lnk"]);
        assert_eq!(pin_at(&pins, "A.LNK", 2), ["b.lnk", "c.lnk", "A.LNK"]);
        assert_eq!(pin_at(&pins, "d.exe", 1), ["a.lnk", "d.exe", "b.lnk", "c.lnk"]);
        assert_eq!(pin_at(&pins, "d.exe", 99), ["a.lnk", "b.lnk", "c.lnk", "d.exe"]);
        assert_eq!(unpin(&pins, "B.lnk"), ["a.lnk", "c.lnk"]);
        assert_eq!(unpin(&pins, "z.lnk"), pins);
    }

    #[test]
    fn the_wheel_turns_the_sound_in_steps_and_no_further_than_it_goes() {
        assert!((volume_after(0.5, 1.0) - 0.52).abs() < 1e-9);
        assert!((volume_after(0.5, -3.0) - 0.44).abs() < 1e-9);
        assert!((volume_after(0.513, 1.0) - 0.54).abs() < 1e-9);
        assert_eq!(volume_after(0.99, 4.0), 1.0);
        assert_eq!(volume_after(0.01, -2.0), 0.0);
    }
}
