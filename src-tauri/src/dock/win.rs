// The dock on Windows: its window, and a hidden window of its own that hears
// what changes the row, on a thread that waits in GetMessage between messages:
//
//   * a shell hook (RegisterShellHookWindow): a window opens, closes, comes to
//     the front or flashes. The row is read again a moment later (EnumWindows,
//     the windows a taskbar would show);
//   * Explorer's app bar notices: the dock is an app bar at the bottom of the
//     main screen, so maximized windows stop above it, and it steps aside while
//     a full-screen app (a game, a video) is in front;
//   * Explorer starting again ("TaskbarCreated"), the screens changing: the
//     app bar is set again, the taskbar hidden again if it was;
//   * the speakers' volume (Core Audio's callback), the network (WinRT's
//     NetworkStatusChanged) and the battery (WM_POWERBROADCAST): the icons in
//     the bar's corner.
//
// The thread and its windows (this one, and the previews: preview.rs) exist
// only while the dock is on.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use windows::core::{implement, w, Interface, BOOL, HSTRING, PCSTR, PCWSTR, PWSTR};
use windows::Networking::Connectivity::{NetworkInformation, NetworkStatusChangedEventHandler};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    CreateRectRgn, CreateRoundRectRgn, GetMonitorInfoW, MonitorFromPoint, SetWindowRgn, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, AUDIO_VOLUME_NOTIFICATION_DATA};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_A, VK_B,
    VK_D, VK_LWIN, VK_N, VK_TAB, VK_W,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHAppBarMessage, ShellExecuteW, ShellLink, ABE_BOTTOM, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    ABN_FULLSCREENAPP, ABN_POSCHANGED, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DeregisterShellHookWindow, DestroyMenu, DestroyWindow,
    DispatchMessageW, EnumChildWindows, EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, GetMessageW,
    GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, KillTimer, PostMessageW, PostQuitMessage, RegisterClassW, RegisterShellHookWindow,
    RegisterWindowMessageW, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TrackPopupMenuEx, TranslateMessage,
    GWL_EXSTYLE, GW_OWNER, HWND_TOPMOST, MF_SEPARATOR, MF_STRING, MSG, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SW_MINIMIZE,
    SW_SHOWNORMAL, TPM_BOTTOMALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_NULL, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW, WS_EX_APPWINDOW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
};

use super::{
    preview, taskbar, DockState, Entry, Open, Pin, SystemIcons, BAR_H, GAP, HEAD, LABEL, RADIUS, SIDE, STATE_EVENT,
    SYSTEM_EVENT,
};
use crate::platform::{self, DesktopMode};

/// Off-screen spot while the dock is off or steps aside: the page stays loaded.
const PARKED: (i32, i32) = (-32000, -32000);

/// Explorer's app bar notices.
const WM_APPBAR: u32 = WM_APP + 1;
/// Place the dock again (its width, the taskbar, the screens).
const WM_PLACE: u32 = WM_APP + 2;
/// The taskbar was shown for a moment: watch it go.
const WM_REVEALED: u32 = WM_APP + 3;
/// Read the row again (from another thread: a timer is set by its window's own).
const WM_REFRESH: u32 = WM_APP + 4;
/// The speakers, the network or the battery changed.
const WM_SYSTEM: u32 = WM_APP + 5;
/// A right click: the menu (`MENU_FOR`).
const WM_MENU: u32 = WM_APP + 6;
/// The page asks for a preview, or for it to go (`PREVIEW_ASK`).
const WM_PREVIEW: u32 = WM_APP + 7;
const REFRESH_TIMER: usize = 1;
const REVEAL_TIMER: usize = 2;
const SYSTEM_TIMER: usize = 3;
/// Windows open and close in bursts: the row is read once they settle.
const REFRESH_MS: u32 = 120;
/// The volume changes many times a second while a slider moves.
const SYSTEM_MS: u32 = 60;
/// WM_POWERBROADCAST: the power source or the battery's charge changed.
const PBT_APMPOWERSTATUSCHANGE: usize = 0x000A;

// The shell hook's codes (winuser.h).
const HSHELL_WINDOWCREATED: u32 = 1;
const HSHELL_WINDOWDESTROYED: u32 = 2;
const HSHELL_WINDOWACTIVATED: u32 = 4;
const HSHELL_WINDOWREPLACED: u32 = 13;
const HSHELL_MONITORCHANGED: u32 = 16;
const HSHELL_RUDEAPPACTIVATED: u32 = 0x8004;
const HSHELL_FLASH: u32 = 0x8006;

static APP: OnceLock<AppHandle> = OnceLock::new();
/// The hidden window while the dock is on, and the dock's own window.
static WATCHER: AtomicIsize = AtomicIsize::new(0);
static DOCK: AtomicIsize = AtomicIsize::new(0);
static STARTING: Mutex<()> = Mutex::new(());
static SHELLHOOK: AtomicU32 = AtomicU32::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// The page's bar width, CSS pixels (f64 bits).
static WIDTH: AtomicU64 = AtomicU64::new(0);
/// The user wants the taskbar hidden while the dock shows.
static HIDE_TASKBAR: AtomicBool = AtomicBool::new(false);
/// A full-screen app is in front: the dock is out of its way.
static FULLSCREEN: AtomicBool = AtomicBool::new(false);
static SHOWN: AtomicBool = AtomicBool::new(false);
/// The row as last read, with the windows behind each app.
static ENTRIES: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
static PUBLISHED: Mutex<Option<DockState>> = Mutex::new(None);
static ORDER: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Windows flashing for attention, until they come to the front.
static FLASHING: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// Read once per file: icons, names, shortcuts' programs.
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);
static NAMES: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static TARGETS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);
/// Ticks of the reveal timer the taskbar has been left alone.
static UNUSED_TICKS: AtomicU32 = AtomicU32::new(0);
/// The room last asked of Explorer: asked again only when it changes, as each
/// change of the work area comes back as a WM_SETTINGCHANGE.
static ROOM: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
/// wParam of the WM_SETTINGCHANGE that says the work area changed (winuser.h).
const SPI_SETWORKAREA: usize = 0x002F;
/// The dock's own pins (Settings), none while they are the taskbar's.
static PINS: Mutex<Option<Vec<String>>> = Mutex::new(None);
/// The icons of the corner, as last read.
static SYSTEM: Mutex<Option<SystemIcons>> = Mutex::new(None);
/// The right click waiting for its menu: an app's id, or none for the bar's.
static MENU_FOR: Mutex<Option<Option<String>>> = Mutex::new(None);
/// The preview asked for: an app's id with its icon's x and the dock's
/// colours, or none for it to go.
#[allow(clippy::type_complexity)]
static PREVIEW_ASK: Mutex<Option<Option<(String, f64, bool)>>> = Mutex::new(None);
/// The bar on screen while it shows (left, top, right, bottom, physical
/// pixels), and the scale it was placed at (f64 bits).
static BAR: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);
static SCALE: AtomicU64 = AtomicU64::new(0);

fn cached<V: Clone>(cache: &Mutex<Option<HashMap<String, V>>>, key: &str, make: impl FnOnce() -> V) -> V {
    if let Some(v) = cache.lock().unwrap().get_or_insert_with(HashMap::new).get(key) {
        return v.clone();
    }
    let v = make();
    cache.lock().unwrap().get_or_insert_with(HashMap::new).insert(key.to_string(), v.clone());
    v
}

fn page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/dock.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("dock.html".into())
}

pub fn setup(app: &AppHandle) {
    let _ = APP.set(app.clone());
    // Ciel stopped last time with the taskbar hidden: it comes back first.
    taskbar::restore();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if taskbar::hidden() {
            taskbar::restore();
        }
        previous(info);
    }));
    let built = WebviewWindowBuilder::new(app, LABEL, page_url(app))
        .additional_browser_args(crate::BROWSER_ARGS)
        .title("Ciel")
        .inner_size(600.0 + 2.0 * SIDE, BAR_H + HEAD)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .disable_drag_drop_handler()
        .focusable(false)
        .visible(true)
        .position(PARKED.0 as f64, PARKED.1 as f64)
        .build();
    match built {
        Ok(win) => {
            // Windows doesn't keep the builder's spot: off screen it goes.
            let _ = win.set_position(PhysicalPosition::new(PARKED.0, PARKED.1));
            platform::prepare_pill_window(&win, DesktopMode::Poll);
            if let Ok(hwnd) = win.hwnd() {
                DOCK.store(hwnd.0 as isize, Ordering::SeqCst);
            }
        }
        Err(err) => crate::log::line(format!("dock window failed: {err}")),
    }
}

pub fn sync(app: &AppHandle, prefs: &crate::settings::DockPrefs) {
    let _ = APP.set(app.clone());
    let on = prefs.on;
    HIDE_TASKBAR.store(prefs.hide_taskbar, Ordering::SeqCst);
    let pins_changed = {
        let mut pins = PINS.lock().unwrap();
        let changed = *pins != prefs.pins;
        pins.clone_from(&prefs.pins);
        changed
    };
    let _guard = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    let running = WATCHER.load(Ordering::SeqCst);
    if on && running != 0 && pins_changed {
        post(running, WM_REFRESH);
    }
    if on && running == 0 {
        if DOCK.load(Ordering::SeqCst) == 0 {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        match std::thread::Builder::new().name("ciel-dock".into()).spawn(move || run(tx)) {
            Ok(_) => {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(3));
            }
            Err(err) => crate::log::line(format!("dock: no watch: {err}")),
        }
    } else if on {
        post(running, WM_PLACE);
    } else if running != 0 {
        post(running, WM_CLOSE);
    }
}

pub fn shutdown() {
    taskbar::quit();
    let running = WATCHER.swap(0, Ordering::SeqCst);
    if running != 0 {
        remove_appbar(HWND(running as *mut _));
        post(running, WM_CLOSE);
    }
}

fn post(hwnd: isize, msg: u32) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(0), LPARAM(0));
    }
}

pub fn state() -> DockState {
    PUBLISHED.lock().unwrap().clone().unwrap_or_default()
}

pub fn fit(width: f64) {
    if !width.is_finite() || width <= 0.0 {
        return;
    }
    let before = f64::from_bits(WIDTH.swap(width.to_bits(), Ordering::SeqCst));
    let running = WATCHER.load(Ordering::SeqCst);
    if running != 0 && (before - width).abs() >= 0.5 {
        post(running, WM_PLACE);
    }
}

// ── The watch ─────────────────────────────────────────────────────────────────

fn run(ready: std::sync::mpsc::Sender<()>) {
    unsafe {
        // The shortcuts' programs and the system image list are COM.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("CielDock");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        RegisterClassW(&wc);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
        SHELLHOOK.store(RegisterWindowMessageW(w!("SHELLHOOK")), Ordering::SeqCst);
        // Top-level (not message-only), never shown: broadcasts and the shell hook reach it.
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
            class,
            w!("Ciel dock"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        ) {
            Ok(h) => h,
            Err(err) => {
                crate::log::line(format!("dock: no watch window: {err}"));
                let _ = ready.send(());
                return;
            }
        };
        if !RegisterShellHookWindow(hwnd).as_bool() {
            crate::log::line("dock: no shell hook, the row is read when the dock is clicked");
        }
        WATCHER.store(hwnd.0 as isize, Ordering::SeqCst);
        FULLSCREEN.store(false, Ordering::SeqCst);
        // Whoever started the watch waits no longer: reading the row asks
        // Ciel's own windows their titles, on the thread that may be waiting.
        let _ = ready.send(());
        preview::create(instance.into());
        let sound = listen_volume();
        let network = NetworkInformation::NetworkStatusChanged(&NetworkStatusChangedEventHandler::new(|_| {
            post(WATCHER.load(Ordering::SeqCst), WM_SYSTEM);
            Ok(())
        }))
        .ok();
        new_appbar(hwnd);
        apply_taskbar();
        read_system();
        place(hwnd);
        refresh();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if let Some((volume, callback)) = sound {
            let _ = volume.UnregisterControlChangeNotify(&callback);
        }
        if let Some(token) = network {
            let _ = NetworkInformation::RemoveNetworkStatusChanged(token);
        }
        preview::destroy();
        *SYSTEM.lock().unwrap() = None;
        let _ = WATCHER.compare_exchange(hwnd.0 as isize, 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// Core Audio calls this on a thread of its own when the speakers' volume or
/// mute changes: the watch reads it again.
#[implement(IAudioEndpointVolumeCallback)]
struct SoundHeard;

impl IAudioEndpointVolumeCallback_Impl for SoundHeard_Impl {
    fn OnNotify(&self, _data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        post(WATCHER.load(Ordering::SeqCst), WM_SYSTEM);
        Ok(())
    }
}

/// The default speakers, told to call `SoundHeard`; none without speakers.
fn listen_volume() -> Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)> {
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let volume: IAudioEndpointVolume = devices.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?.Activate(CLSCTX_ALL, None).ok()?;
        let callback: IAudioEndpointVolumeCallback = SoundHeard.into();
        volume.RegisterControlChangeNotify(&callback).ok()?;
        Some((volume, callback))
    }
}

/// The speakers, the network and the battery, with Tools → Controls' readers
/// (the quick ones only).
fn read_system() {
    let (volume, muted) = crate::controls::volume().map_or((None, false), |(v, m)| (Some(v), m));
    let network = crate::controls::network();
    let battery = crate::controls::battery();
    let icons = SystemIcons {
        volume,
        muted,
        network: network.as_ref().map(|n| n.kind.clone()).unwrap_or_default(),
        network_name: network.map(|n| n.name).unwrap_or_default(),
        battery: battery.as_ref().map(|b| b.percent),
        charging: battery.is_some_and(|b| b.charging),
    };
    let changed = SYSTEM.lock().unwrap().replace(icons.clone()).as_ref() != Some(&icons);
    if !changed {
        return;
    }
    // On its own: the row, with every app's icon, isn't sent again for a volume step.
    if let Some(state) = PUBLISHED.lock().unwrap().as_mut() {
        state.system = icons.clone();
    }
    if let Some(app) = APP.get() {
        let _ = app.emit_to(LABEL, SYSTEM_EVENT, icons);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        m if m != 0 && m == SHELLHOOK.load(Ordering::Relaxed) => {
            let code = wparam.0 as u32;
            let target = lparam.0;
            match code {
                HSHELL_FLASH => {
                    let mut flashing = FLASHING.lock().unwrap();
                    if !flashing.contains(&target) {
                        flashing.push(target);
                    }
                }
                HSHELL_WINDOWACTIVATED | HSHELL_RUDEAPPACTIVATED | HSHELL_WINDOWDESTROYED => {
                    FLASHING.lock().unwrap().retain(|w| *w != target);
                }
                _ => {}
            }
            if matches!(
                code,
                HSHELL_WINDOWCREATED
                    | HSHELL_WINDOWDESTROYED
                    | HSHELL_WINDOWACTIVATED
                    | HSHELL_RUDEAPPACTIVATED
                    | HSHELL_WINDOWREPLACED
                    | HSHELL_MONITORCHANGED
                    | HSHELL_FLASH
            ) {
                taskbar::keep_hidden();
                unsafe { SetTimer(Some(hwnd), REFRESH_TIMER, REFRESH_MS, None) };
            }
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == REFRESH_TIMER => {
            let _ = unsafe { KillTimer(Some(hwnd), REFRESH_TIMER) };
            refresh();
            return LRESULT(0);
        }
        WM_REFRESH => {
            unsafe { SetTimer(Some(hwnd), REFRESH_TIMER, REFRESH_MS, None) };
            return LRESULT(0);
        }
        WM_SYSTEM => {
            unsafe { SetTimer(Some(hwnd), SYSTEM_TIMER, SYSTEM_MS, None) };
            return LRESULT(0);
        }
        WM_POWERBROADCAST => {
            if wparam.0 == PBT_APMPOWERSTATUSCHANGE {
                unsafe { SetTimer(Some(hwnd), SYSTEM_TIMER, SYSTEM_MS, None) };
            }
            return LRESULT(1);
        }
        WM_TIMER if wparam.0 == SYSTEM_TIMER => {
            let _ = unsafe { KillTimer(Some(hwnd), SYSTEM_TIMER) };
            read_system();
            return LRESULT(0);
        }
        WM_MENU => {
            let asked = MENU_FOR.lock().unwrap().take();
            if let Some(id) = asked {
                show_menu(hwnd, id);
            }
            return LRESULT(0);
        }
        WM_PREVIEW => {
            let asked = PREVIEW_ASK.lock().unwrap().take();
            match asked {
                Some(Some((id, x, light))) => show_preview(&id, x, light),
                Some(None) => preview::leave(),
                None => {}
            }
            return LRESULT(0);
        }
        WM_TIMER if wparam.0 == REVEAL_TIMER => {
            if reveal_over() {
                let _ = unsafe { KillTimer(Some(hwnd), REVEAL_TIMER) };
                taskbar::end_reveal();
                place(hwnd);
            }
            return LRESULT(0);
        }
        WM_REVEALED => {
            UNUSED_TICKS.store(0, Ordering::Relaxed);
            // The taskbar comes up where the dock is: the dock steps aside.
            place(hwnd);
            unsafe { SetTimer(Some(hwnd), REVEAL_TIMER, 700, None) };
            return LRESULT(0);
        }
        WM_APPBAR => {
            match wparam.0 as u32 {
                ABN_FULLSCREENAPP => {
                    FULLSCREEN.store(lparam.0 != 0, Ordering::SeqCst);
                    place(hwnd);
                }
                ABN_POSCHANGED => place(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        WM_PLACE => {
            apply_taskbar();
            place(hwnd);
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE => {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_PLACE, WPARAM(0), LPARAM(0));
            }
        }
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA => {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_PLACE, WPARAM(0), LPARAM(0));
            }
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe {
                let _ = DeregisterShellHookWindow(hwnd);
            }
            remove_appbar(hwnd);
            taskbar::restore();
            preview::hide();
            park();
            *PUBLISHED.lock().unwrap() = None;
            ENTRIES.lock().unwrap().clear();
            FLASHING.lock().unwrap().clear();
            unsafe { PostQuitMessage(0) };
            return LRESULT(0);
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            // Explorer started again: a new taskbar, and the app bars forgotten.
            crate::log::line("dock: Explorer started again");
            FULLSCREEN.store(false, Ordering::SeqCst);
            new_appbar(hwnd);
            apply_taskbar();
            // Its new taskbar shows: hidden again if it was.
            taskbar::keep_hidden();
            place(hwnd);
            unsafe { SetTimer(Some(hwnd), REFRESH_TIMER, REFRESH_MS, None) };
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// The taskbar hidden or shown as the user wants it.
fn apply_taskbar() {
    if HIDE_TASKBAR.load(Ordering::SeqCst) {
        if !taskbar::hidden() {
            taskbar::hide();
        }
    } else if taskbar::hidden() {
        taskbar::restore();
    }
    // The ⌃ for the hidden taskbar comes or goes.
    publish(None);
}

// ── Where it sits ─────────────────────────────────────────────────────────────

fn appbar(hwnd: HWND) -> APPBARDATA {
    APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, hWnd: hwnd, ..Default::default() }
}

fn new_appbar(hwnd: HWND) {
    *ROOM.lock().unwrap() = None;
    let mut abd = appbar(hwnd);
    abd.uCallbackMessage = WM_APPBAR;
    unsafe {
        // Again after Explorer restarts; a bar already known is refused harmlessly.
        SHAppBarMessage(ABM_NEW, &mut abd);
    }
}

fn remove_appbar(hwnd: HWND) {
    *ROOM.lock().unwrap() = None;
    let mut abd = appbar(hwnd);
    unsafe {
        SHAppBarMessage(ABM_REMOVE, &mut abd);
    }
}

/// The main screen and its scale (1.0 at 96 dpi).
fn main_screen() -> Option<(RECT, f64)> {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Some((info.rcMonitor, dx.max(96) as f64 / 96.0))
    }
}

/// Keeps the bottom of the main screen for the dock (above the taskbar when it
/// shows), and puts the dock there, centred; off screen while a full-screen app
/// is in front, or while the hidden taskbar is shown for a moment in its place.
fn place(hwnd: HWND) {
    let Some((screen, scale)) = main_screen() else { return };
    let band = ((BAR_H + 2.0 * GAP) * scale).round() as i32;
    let gap = (GAP * scale).round() as i32;
    let mut abd = appbar(hwnd);
    abd.uEdge = ABE_BOTTOM;
    abd.rc = RECT { left: screen.left, right: screen.right, top: screen.bottom - band, bottom: screen.bottom };
    unsafe {
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        // Explorer moved the bottom up past the taskbar, if it shows: same height.
        abd.rc.top = abd.rc.bottom - band;
        let room = (abd.rc.left, abd.rc.top, abd.rc.right, abd.rc.bottom);
        let mut last = ROOM.lock().unwrap();
        if *last != Some(room) {
            *last = Some(room);
            SHAppBarMessage(ABM_SETPOS, &mut abd);
        }
    }
    if FULLSCREEN.load(Ordering::SeqCst) || taskbar::is_revealed() {
        park();
        return;
    }
    let width = f64::from_bits(WIDTH.load(Ordering::SeqCst));
    let screen_w = screen.right - screen.left;
    let bar_w = if width > 0.0 { (width * scale).round() as i32 } else { (600.0 * scale) as i32 }.clamp(1, screen_w - 2 * gap);
    let bar_h = (BAR_H * scale).round() as i32;
    let (side, head) = ((SIDE * scale).round() as i32, (HEAD * scale).round() as i32);
    let x = screen.left + (screen_w - bar_w) / 2;
    let y = abd.rc.bottom - gap - bar_h;
    // The window has room around the bar, for the icons that grow (the page
    // keeps the bar at its bottom, in the middle).
    move_dock(x - side, y - head, bar_w + 2 * side, bar_h + head);
    *BAR.lock().unwrap() = Some((x, y, x + bar_w, y + bar_h));
    SCALE.store(scale.to_bits(), Ordering::SeqCst);
    show(true);
}

fn park() {
    *BAR.lock().unwrap() = None;
    preview::hide();
    move_dock(PARKED.0, PARKED.1, 0, 0);
    show(false);
}

fn scale() -> f64 {
    let s = f64::from_bits(SCALE.load(Ordering::SeqCst));
    if s > 0.0 { s } else { 1.0 }
}

/// What takes clicks in the dock's window: all of it while the pointer is on
/// the bar (the icons grown into the room above), else the bar alone, the
/// room around it letting clicks through to what is behind. From the page, on
/// the main thread, which owns the window: each time the pointer comes or
/// goes, and each time the window changes size.
pub fn region(hover: bool) {
    let dock = DOCK.load(Ordering::SeqCst);
    if dock == 0 {
        return;
    }
    let hwnd = HWND(dock as *mut _);
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    let s = scale();
    let (side, head) = ((SIDE * s).round() as i32, (HEAD * s).round() as i32);
    let round = (2.0 * RADIUS * s).round() as i32;
    unsafe {
        let rgn = if hover { CreateRectRgn(0, 0, w, h) } else { CreateRoundRectRgn(side, head, w - side + 1, h + 1, round, round) };
        // The window owns the region from here on.
        let _ = SetWindowRgn(hwnd, Some(rgn), true);
    }
}

fn move_dock(x: i32, y: i32, w: i32, h: i32) {
    let dock = DOCK.load(Ordering::SeqCst);
    if dock == 0 {
        return;
    }
    let mut flags = SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS;
    if w == 0 {
        flags |= windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE;
    }
    unsafe {
        // Async: the window's thread is the app's main one, never waited on from here.
        let _ = SetWindowPos(HWND(dock as *mut _), Some(HWND_TOPMOST), x, y, w, h, flags);
    }
}

fn show(visible: bool) {
    if SHOWN.swap(visible, Ordering::SeqCst) != visible {
        publish(None);
    }
}

// ── The row ───────────────────────────────────────────────────────────────────

/// Reads the pins and the open windows again, and tells the page if the row changed.
fn refresh() {
    let pins = pins();
    let open = open_apps();
    let entries = super::merge(&pins, &open, &mut ORDER.lock().unwrap());
    publish(Some(entries));
}

fn publish(entries: Option<Vec<Entry>>) {
    let apps = {
        let mut kept = ENTRIES.lock().unwrap();
        if let Some(entries) = entries {
            *kept = entries;
        }
        kept.iter().map(|e| e.app.clone()).collect()
    };
    let state = DockState {
        apps,
        taskbar_hidden: taskbar::hidden(),
        shown: SHOWN.load(Ordering::SeqCst),
        system: SYSTEM.lock().unwrap().clone().unwrap_or_default(),
    };
    let changed = {
        let mut last = PUBLISHED.lock().unwrap();
        let changed = last.as_ref() != Some(&state);
        *last = Some(state.clone());
        changed
    };
    if changed {
        if let Some(app) = APP.get() {
            let _ = app.emit_to(LABEL, STATE_EVENT, state);
        }
    }
}

/// The dock's own pins when the user made them, else the taskbar's; those
/// whose file is gone are left out.
fn pin_paths() -> Vec<PathBuf> {
    match PINS.lock().unwrap().clone() {
        Some(list) => list.into_iter().map(PathBuf::from).filter(|p| p.exists()).collect(),
        None => crate::quick::pinned(),
    }
}

fn pins() -> Vec<Pin> {
    pin_paths()
        .into_iter()
        .map(|path| {
            let id = path.display().to_string();
            let shortcut = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"));
            let (name, target) = if shortcut {
                let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                (name, cached(&TARGETS, &id, || link_target(&path)))
            } else {
                // A program pinned from the dock: named and matched as when it is open.
                let key = id.to_lowercase();
                (cached(&NAMES, &key, || program_name(&id)), Some(key))
            };
            Pin { name, icon: cached(&ICONS, &id, || crate::quick::dock_icon(&path)), target, id }
        })
        .collect()
}

/// The pins as the row has them now, in their order: what a change starts from.
fn current_pins() -> Vec<String> {
    ENTRIES.lock().unwrap().iter().filter(|e| e.app.pinned).map(|e| e.launch.clone()).collect()
}

/// A store app's program can't be started by its path: it isn't offered as a pin.
fn pinnable(entry: &Entry) -> bool {
    entry.app.pinned || !entry.launch.to_ascii_lowercase().contains("\\windowsapps\\")
}

/// The pins once the app `id` of the row is moved, or pinned, to `to`.
pub fn pin_moved(id: &str, to: usize) -> Option<Vec<String>> {
    let entry = ENTRIES.lock().unwrap().iter().find(|e| e.app.id == id).cloned()?;
    pinnable(&entry).then(|| super::pin_at(&current_pins(), &entry.launch, to))
}

/// The program a shortcut starts, its path in lowercase; none for a store app's.
fn link_target(path: &Path) -> Option<String> {
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
        let file: IPersistFile = link.cast().ok()?;
        file.Load(&HSTRING::from(path.as_os_str()), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        let target = String::from_utf16_lossy(&buf[..len]);
        (!target.is_empty()).then(|| target.to_lowercase())
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut name = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

/// A window the taskbar would show: visible, on this virtual desktop, titled,
/// nobody's dialog (unless it asks for a button), no tool window.
fn on_taskbar(hwnd: HWND) -> bool {
    unsafe {
        // Styles first: they ask nothing of the window. Its title is a message
        // to its thread, Ciel's main one for Ciel's own windows.
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let app_window = ex & WS_EX_APPWINDOW.0 != 0;
        if !app_window && (ex & WS_EX_TOOLWINDOW.0 != 0 || ex & WS_EX_NOACTIVATE.0 != 0 || GetWindow(hwnd, GW_OWNER).is_ok()) {
            return false;
        }
        if GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let attr = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, std::mem::size_of::<u32>() as u32);
        if attr.is_ok() && cloaked != 0 {
            return false;
        }
    }
    !matches!(class_of(hwnd).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// A window's program; for a store app, the one inside its frame rather than the frame's.
fn program_of(hwnd: HWND) -> Option<String> {
    let pid = pid_of(hwnd);
    let path = process_path(pid)?;
    if !path.to_ascii_lowercase().ends_with("\\applicationframehost.exe") {
        return Some(path);
    }
    unsafe extern "system" fn inner(child: HWND, lparam: LPARAM) -> BOOL {
        let found = unsafe { &mut *(lparam.0 as *mut (u32, u32)) };
        let pid = pid_of(child);
        if pid != found.0 {
            found.1 = pid;
            return false.into();
        }
        true.into()
    }
    let mut found = (pid, 0u32);
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(inner), LPARAM(&mut found as *mut _ as isize));
    }
    if found.1 != 0 {
        process_path(found.1).or(Some(path))
    } else {
        Some(path)
    }
}

/// The name a program gives itself (its file description), else its file's name.
fn program_name(path: &str) -> String {
    let stem = Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    file_description(path).unwrap_or_else(|| crate::flow::display_name(&stem))
}

fn file_description(path: &str) -> Option<String> {
    unsafe {
        let wide = HSTRING::from(path);
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr() as *mut _).ok()?;
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(data.as_ptr() as *const _, w!("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let page = *(ptr as *const u16).add(1);
        let key = HSTRING::from(format!("\\StringFileInfo\\{lang:04x}{page:04x}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as *const _, PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = text.iter().position(|c| *c == 0).unwrap_or(text.len());
        let name = String::from_utf16_lossy(&text[..end]).trim().to_string();
        (!name.is_empty()).then_some(name)
    }
}

/// The programs with a window on the taskbar, the one nearest the front first.
fn open_apps() -> Vec<Open> {
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        if on_taskbar(hwnd) {
            list.push(hwnd);
        }
        true.into()
    }
    let mut windows: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut windows as *mut _ as isize));
    }
    // Ciel's own windows: the ones Windows keeps off its taskbar (the island,
    // the dock, the cloud, the pill...) look like any other from outside, so
    // only those that have a button there count.
    let me = std::process::id();
    let mine = own_on_taskbar();
    windows.retain(|hwnd| pid_of(*hwnd) != me || mine.contains(&(hwnd.0 as isize)));
    let front = unsafe { GetForegroundWindow() };
    let flashing = FLASHING.lock().unwrap().clone();
    let mut out: Vec<Open> = Vec::new();
    for hwnd in windows {
        let Some(path) = program_of(hwnd) else { continue };
        let key = path.to_lowercase();
        let raw = hwnd.0 as isize;
        let at = match out.iter().position(|o| o.key == key) {
            Some(at) => at,
            None => {
                out.push(Open {
                    name: cached(&NAMES, &key, || program_name(&path)),
                    icon: cached(&ICONS, &key, || crate::quick::dock_icon(Path::new(&path))),
                    key,
                    path,
                    windows: Vec::new(),
                    active: false,
                    attention: false,
                });
                out.len() - 1
            }
        };
        let o = &mut out[at];
        o.windows.push(raw);
        o.active |= hwnd == front;
        o.attention |= flashing.contains(&raw);
    }
    out
}

/// Ciel's windows that have a button on the Windows taskbar: Settings and the
/// games. Every other one is built with `skip_taskbar`.
fn own_on_taskbar() -> Vec<isize> {
    use tauri::Manager;
    let Some(app) = APP.get() else { return Vec::new() };
    [crate::HUB_LABEL, crate::play::LABEL]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .filter_map(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .collect()
}

// ── What the page asks ───────────────────────────────────────────────────────

fn shell_open(target: &str) -> bool {
    let file = HSTRING::from(target);
    let done = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(file.as_ptr()), None, None, SW_SHOWNORMAL) };
    // Above 32: it opened.
    done.0 as isize > 32
}

pub fn open(id: &str, new_window: bool) -> bool {
    let Some(entry) = ENTRIES.lock().unwrap().iter().find(|e| e.app.id == id).cloned() else { return false };
    let windows: Vec<isize> =
        entry.windows.iter().copied().filter(|w| unsafe { IsWindow(Some(HWND(*w as *mut _))) }.as_bool()).collect();
    if new_window {
        return shell_open(&entry.launch);
    }
    let front = unsafe { GetForegroundWindow() }.0 as isize;
    let done = match super::click(&windows, front) {
        super::Click::Launch => shell_open(&entry.launch),
        super::Click::Front(w) => platform::bring_forward(w),
        super::Click::Minimize(w) => {
            let _ = unsafe { ShowWindow(HWND(w as *mut _), SW_MINIMIZE) };
            true
        }
    };
    post(WATCHER.load(Ordering::SeqCst), WM_REFRESH);
    done
}

/// Windows' own panels, as their keys open them.
pub fn panel(what: &str) -> bool {
    let letter = match what {
        "taskView" => VK_TAB,
        "widgets" => VK_W,
        "quickSettings" => VK_A,
        "notifications" => VK_N,
        "desktop" => VK_D,
        _ => return false,
    };
    send(&[key(VK_LWIN, false), key(letter, false), key(letter, true), key(VK_LWIN, true)])
}

pub fn turn_volume(notches: f64) -> bool {
    if !notches.is_finite() {
        return false;
    }
    let Some((level, _)) = crate::controls::volume() else { return false };
    let done = crate::controls::set_volume(super::volume_after(level, notches.clamp(-10.0, 10.0)));
    post(WATCHER.load(Ordering::SeqCst), WM_SYSTEM);
    done
}

pub fn menu(id: Option<String>) {
    *MENU_FOR.lock().unwrap() = Some(id);
    post(WATCHER.load(Ordering::SeqCst), WM_MENU);
}

pub fn preview(id: Option<String>, x: f64, light: bool) {
    *PREVIEW_ASK.lock().unwrap() = Some(id.map(|id| (id, x, light)));
    post(WATCHER.load(Ordering::SeqCst), WM_PREVIEW);
}

/// The app's windows above its icon (`x`: CSS pixels across the dock's window).
fn show_preview(id: &str, x: f64, light: bool) {
    let Some(bar) = *BAR.lock().unwrap() else { return };
    let Some(windows) = ENTRIES.lock().unwrap().iter().find(|e| e.app.id == id).map(|e| e.windows.clone()) else {
        preview::hide();
        return;
    };
    let Some((screen, _)) = main_screen() else { return };
    let s = scale();
    let left = bar.0 - (SIDE * s).round() as i32;
    let center = left + (x.clamp(0.0, 10_000.0) * s).round() as i32;
    // Above the room where the icons grow.
    let bottom = bar.1 - (HEAD * s).round() as i32 + (4.0 * s).round() as i32;
    preview::show(id, &windows, center, bottom, screen, s, light);
}

/// Explorer's menus follow the system's dark mode once the app allows it
/// (uxtheme's SetPreferredAppMode, by ordinal: it has no name; Windows 10 1903 on).
fn allow_dark_menus() {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| unsafe {
        let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(set) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
            let set: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set);
            // 1: AllowDark, the system's choice.
            set(1);
        }
        if let Some(flush) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    });
}

const MENU_OPEN: usize = 1;
const MENU_PIN: usize = 2;
const MENU_UNPIN: usize = 3;
const MENU_CLOSE: usize = 4;
const MENU_TASK_MANAGER: usize = 10;
const MENU_SETTINGS: usize = 11;

/// The menu of a right click, where the pointer is: an app's (start another,
/// pin or unpin, close its windows), or the bar's (Task Manager, the dock's
/// settings). Owned by the watch's window, which comes to the front for it,
/// so a click elsewhere closes it, as the taskbar's.
fn show_menu(hwnd: HWND, id: Option<String>) {
    preview::hide();
    let entry = id.and_then(|id| ENTRIES.lock().unwrap().iter().find(|e| e.app.id == id).cloned());
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else { return };
    let add = |id: usize, text: &str| unsafe {
        let text = HSTRING::from(text);
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(text.as_ptr()));
    };
    match &entry {
        Some(e) => {
            add(MENU_OPEN, &e.app.name);
            unsafe {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            if e.app.pinned {
                add(MENU_UNPIN, &crate::i18n::t("Unpin from the dock"));
            } else if pinnable(e) {
                add(MENU_PIN, &crate::i18n::t("Pin to the dock"));
            }
            match e.windows.len() {
                0 => {}
                1 => add(MENU_CLOSE, &crate::i18n::t("Close window")),
                _ => add(MENU_CLOSE, &crate::i18n::t("Close all windows")),
            }
        }
        None => {
            add(MENU_TASK_MANAGER, &crate::i18n::t("Task Manager"));
            add(MENU_SETTINGS, &crate::i18n::t("Dock settings"));
        }
    }
    allow_dark_menus();
    let mut at = POINT::default();
    let chosen = unsafe {
        let _ = GetCursorPos(&mut at);
        // Allowed: the right click on the dock, Ciel's, was the last input.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN).0, at.x, at.y, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        chosen.0 as usize
    };
    let Some(app) = APP.get() else { return };
    match (chosen, entry) {
        (MENU_OPEN, Some(e)) => {
            shell_open(&e.launch);
        }
        (MENU_PIN, Some(e)) => {
            let pins = current_pins();
            let at = pins.len();
            super::save_pins(app, super::pin_at(&pins, &e.launch, at));
        }
        (MENU_UNPIN, Some(e)) => super::save_pins(app, super::unpin(&current_pins(), &e.launch)),
        (MENU_CLOSE, Some(e)) => {
            for w in e.windows {
                unsafe {
                    let _ = PostMessageW(Some(HWND(w as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
        (MENU_TASK_MANAGER, None) => {
            shell_open("taskmgr.exe");
        }
        (MENU_SETTINGS, None) => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || crate::open_settings(&handle, "dock"));
        }
        _ => {}
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(keys: &[INPUT]) -> bool {
    unsafe { SendInput(keys, std::mem::size_of::<INPUT>() as i32) as usize == keys.len() }
}

pub fn start_menu() -> bool {
    send(&[key(VK_LWIN, false), key(VK_LWIN, true)])
}

/// The hidden taskbar comes up with the keyboard on its notification area
/// (Windows+B), and hides again once the pointer and the keyboard leave it.
pub fn reveal_taskbar() -> bool {
    if !taskbar::reveal() {
        return false;
    }
    send(&[key(VK_LWIN, false), key(VK_B, false), key(VK_B, true), key(VK_LWIN, true)]);
    let running = WATCHER.load(Ordering::SeqCst);
    if running != 0 {
        post(running, WM_REVEALED);
    }
    true
}

/// The taskbar has been left alone a moment: the pointer is off it and Explorer
/// (the taskbar, its menus, the hidden icons) isn't in front.
fn reveal_over() -> bool {
    let Some((rect, pid)) = taskbar::revealed() else { return true };
    let mut p = POINT::default();
    let over = unsafe { GetCursorPos(&mut p) }.is_ok()
        && p.x >= rect.left
        && p.x < rect.right
        && p.y >= rect.top
        && p.y < rect.bottom;
    let front = unsafe { GetForegroundWindow() };
    let explorer_ui = !front.is_invalid()
        && pid_of(front) == pid
        && !matches!(class_of(front).as_str(), "CabinetWClass" | "Progman" | "WorkerW");
    if over || explorer_ui {
        UNUSED_TICKS.store(0, Ordering::Relaxed);
        return false;
    }
    UNUSED_TICKS.fetch_add(1, Ordering::Relaxed) >= 2
}
