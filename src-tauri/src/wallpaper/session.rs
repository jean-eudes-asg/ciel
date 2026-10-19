// Windows: a hidden window of its own that hears what the wallpapers care
// about, while the lock screen is in use or live mode is on:
//
//   * the session locking and unlocking (WTSRegisterSessionNotification): the
//     island hears `wallpaper-session`, and live windows pause;
//   * the display going off (GUID_CONSOLE_DISPLAY_STATE) and the power source
//     changing: live windows pause or carry on;
//   * Explorer starting again (the "TaskbarCreated" broadcast, which only
//     top-level windows get): live windows go back behind the new icons;
//   * monitors changing (WM_DISPLAYCHANGE): live windows are laid out again.
//
// It waits in GetMessage the rest of the time: nothing runs between messages.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::w;
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    RegisterPowerSettingNotification, UnregisterPowerSettingNotification, HPOWERNOTIFY, POWERBROADCAST_SETTING,
};
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE, MSG, PBT_APMPOWERSTATUSCHANGE,
    PBT_POWERSETTINGCHANGE, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_POWERBROADCAST,
    WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

use super::{live, SESSION_EVENT};

const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;
/// The console display's power state (off, on, dimmed).
const GUID_CONSOLE_DISPLAY_STATE: windows::core::GUID =
    windows::core::GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

static LOCKED: AtomicBool = AtomicBool::new(false);
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);
/// The hidden window while it runs, and its display-state registration.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static DISPLAY_NOTIFY: AtomicIsize = AtomicIsize::new(0);
static STARTING: Mutex<()> = Mutex::new(());
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn locked() -> bool {
    LOCKED.load(Ordering::Relaxed)
}

pub fn display_off() -> bool {
    DISPLAY_OFF.load(Ordering::Relaxed)
}

#[derive(Clone, Serialize)]
struct Session {
    locked: bool,
}

/// Starts or stops following the session.
pub fn follow(app: &AppHandle, on: bool) {
    let _ = APP.set(app.clone());
    let _guard = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    let running = WINDOW.load(Ordering::SeqCst);
    if on && running == 0 {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("ciel-wallpaper-session".into()).spawn(move || run(tx));
        match spawned {
            // Wait for the window, so a stop right after finds it.
            Ok(_) => {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(2));
            }
            Err(err) => crate::log::line(format!("wallpaper: no session watch: {err}")),
        }
    } else if !on && running != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(running as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn run(ready: std::sync::mpsc::Sender<()>) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("CielWallpaperSession");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: class, ..Default::default() };
        // A second registration (the watch started again) fails harmlessly.
        RegisterClassW(&wc);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
        // Top-level (not message-only), never shown: broadcasts reach it.
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
            class,
            w!("Ciel wallpapers"),
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
                crate::log::line(format!("wallpaper: no session window: {err}"));
                let _ = ready.send(());
                return;
            }
        };
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        if let Ok(handle) =
            RegisterPowerSettingNotification(HANDLE(hwnd.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE)
        {
            DISPLAY_NOTIFY.store(handle.0 as isize, Ordering::SeqCst);
        }
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
        let _ = ready.send(());
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        WINDOW.store(0, Ordering::SeqCst);
        LOCKED.store(false, Ordering::Relaxed);
        DISPLAY_OFF.store(false, Ordering::Relaxed);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let app = APP.get();
    match msg {
        WM_WTSSESSION_CHANGE if matches!(wparam.0, WTS_SESSION_LOCK | WTS_SESSION_UNLOCK) => {
            let now_locked = wparam.0 == WTS_SESSION_LOCK;
            LOCKED.store(now_locked, Ordering::Relaxed);
            if let Some(app) = app {
                let _ = app.emit_to(crate::island::WINDOW_LABEL, SESSION_EVENT, Session { locked: now_locked });
            }
            live::poke();
        }
        WM_POWERBROADCAST => {
            if wparam.0 as u32 == PBT_POWERSETTINGCHANGE && lparam.0 != 0 {
                let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
                if setting.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && setting.DataLength >= 1 {
                    // 0 off, 1 on, 2 dimmed.
                    DISPLAY_OFF.store(setting.Data[0] == 0, Ordering::Relaxed);
                }
            }
            if matches!(wparam.0 as u32, PBT_POWERSETTINGCHANGE | PBT_APMPOWERSTATUSCHANGE) {
                live::poke();
            }
            return LRESULT(1);
        }
        WM_DISPLAYCHANGE => {
            if let Some(app) = app {
                live::relayout_soon(app);
            }
        }
        WM_CLOSE => {
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        WM_DESTROY => {
            unsafe {
                let _ = WTSUnRegisterSessionNotification(hwnd);
                let display = DISPLAY_NOTIFY.swap(0, Ordering::SeqCst);
                if display != 0 {
                    let _ = UnregisterPowerSettingNotification(HPOWERNOTIFY(display));
                }
                PostQuitMessage(0);
            }
            return LRESULT(0);
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) => {
            crate::log::line("wallpaper: Explorer started again");
            if let Some(app) = app {
                live::heal_soon(app);
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
