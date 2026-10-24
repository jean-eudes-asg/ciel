// The screensaver (Settings → General → Screensaver): after the minutes chosen
// without a key or the mouse, a flip clock over the sky on the main display
// (screensaver.html); the other displays are left alone. At night, once the
// clock has shown long enough, the screens go off; at the morning hour they
// come back on, with the clock. A key or the mouse sends it away at any time.
//
// One thread decides, every few seconds, from the system's idle time. While
// the clock shows it holds the system back from blanking the screen (GNOME's
// session inhibitor, Windows' display request), and lets go otherwise, so
// outside the clock the system's own power settings rule as before.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::settings::Settings;

pub const LABEL: &str = "screensaver";
const TICK: Duration = Duration::from_secs(5);
/// Input this recent, after the clock came up, is the user back.
const BACK_MS: u64 = 2_000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// No clock: the user is here, or the screensaver is off.
    Away,
    /// The clock shows, since then.
    Shown(Instant),
    /// The screens are off for the night, since then.
    Off(Instant),
}

static PHASE: Mutex<Phase> = Mutex::new(Phase::Away);

/// Minutes since midnight of "HH:MM", or None.
fn minutes(hhmm: &str) -> Option<u32> {
    let (h, m) = hhmm.trim().split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// Whether `now` (minutes since midnight) is in the night from `from` to `until`, across midnight or not.
fn in_night(now: u32, from: u32, until: u32) -> bool {
    if from <= until { (from..until).contains(&now) } else { now >= from || now < until }
}

fn now_minutes() -> u32 {
    let t = crate::platform::local_time();
    t.hour * 60 + t.minute
}

fn night(settings: &Settings) -> Option<(u32, u32)> {
    if !settings.night_off {
        return None;
    }
    Some((minutes(&settings.night_from)?, minutes(&settings.night_until)?))
}

/// What the clock should do next, from where it is, the settings and the idle time.
fn next(phase: Phase, s: &Settings, idle_ms: u64, now_min: u32) -> Phase {
    let since = |at: Instant| at.elapsed().as_millis() as u64;
    // Input after the clock came up (or after the screens went off): the user is back.
    let back = |at: Instant| idle_ms + BACK_MS < since(at);
    match phase {
        _ if s.screensaver_minutes == 0 => Phase::Away,
        Phase::Away if idle_ms >= u64::from(s.screensaver_minutes) * 60_000 => Phase::Shown(Instant::now()),
        Phase::Away => Phase::Away,
        Phase::Shown(at) if back(at) => Phase::Away,
        Phase::Shown(at) => match night(s) {
            Some((from, until)) if in_night(now_min, from, until) && since(at) >= u64::from(s.night_off_after) * 60_000 => {
                Phase::Off(Instant::now())
            }
            _ => Phase::Shown(at),
        },
        Phase::Off(at) if back(at) => Phase::Away,
        Phase::Off(at) => match night(s) {
            // Morning: the screens on, the clock back.
            Some((from, until)) if in_night(now_min, from, until) => Phase::Off(at),
            _ => Phase::Shown(Instant::now()),
        },
    }
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("ciel-screensaver".into()).spawn(move || {
        let mut sys = os::System::new();
        loop {
            std::thread::sleep(TICK);
            let Some(shared) = app.try_state::<crate::Shared>() else { continue };
            let settings = shared.settings.lock().unwrap().clone();
            let before = *PHASE.lock().unwrap();
            let after = match sys.idle_ms() {
                Some(idle) => next(before, &settings, idle, now_minutes()),
                // No idle time on this desktop: nothing comes up on its own.
                None => before,
            };
            apply(&app, &mut sys, before, after);
            // The page may have sent it away meanwhile: what it says wins.
            let mut phase = PHASE.lock().unwrap();
            if *phase == before {
                *phase = after;
            }
        }
    });
    if let Err(err) = spawned {
        crate::log::line(format!("screensaver: no thread: {err}"));
    }
}

fn apply(app: &AppHandle, sys: &mut os::System, before: Phase, after: Phase) {
    match (before, after) {
        (Phase::Off(_), Phase::Shown(_)) => {
            crate::log::line("screensaver: morning, screens on");
            sys.screens(true);
            show(app);
        }
        (Phase::Away, Phase::Shown(_)) => show(app),
        (Phase::Shown(_), Phase::Off(_)) => {
            crate::log::line("screensaver: night, screens off");
            hide(app);
            sys.screens(false);
        }
        (Phase::Shown(_) | Phase::Off(_), Phase::Away) => hide(app),
        _ => {}
    }
    // The system may blank the screen only while the clock isn't up.
    sys.keep_awake(matches!(after, Phase::Shown(_)));
}

fn page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/screensaver.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("screensaver.html".into())
}

/// The clock's window, made hidden at launch and only shown and hidden after,
/// like the Ciel window (lib.rs create_settings_window): on Windows a WebView2
/// window made later may come up blank, and one made in a synchronous command
/// (Preview) hangs the app.
pub fn setup(app: &AppHandle) {
    let built = WebviewWindowBuilder::new(app, LABEL, page_url(app))
        .additional_browser_args(crate::BROWSER_ARGS)
        .title("Ciel")
        .decorations(false)
        .resizable(true)
        .shadow(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .maximizable(false)
        .minimizable(false)
        .closable(false)
        .disable_drag_drop_handler()
        .background_color(tauri::window::Color(5, 7, 13, 255))
        .visible(false)
        .build();
    if let Err(err) = built {
        crate::log::line(format!("screensaver: no window: {err}"));
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// Full screen on the main display only.
fn show(app: &AppHandle) {
    let Some(win) = window(app) else { return };
    let Some(m) = app.primary_monitor().ok().flatten() else { return };
    // Before the window: on Windows, set_focus presses Alt to bring it to the
    // front, and the page must know by then that the clock is up.
    let _ = win.emit_to(LABEL, "screensaver-shown", true);
    let _ = win.set_fullscreen(false);
    let _ = win.set_position(*m.position());
    let _ = win.set_size(*m.size());
    let _ = win.show();
    let _ = win.set_fullscreen(true);
    let _ = win.set_always_on_top(true);
    let _ = win.set_focus();
}

fn hide(app: &AppHandle) {
    if let Some(win) = window(app) {
        let _ = win.set_fullscreen(false);
        let _ = win.hide();
        let _ = win.emit_to(LABEL, "screensaver-hidden", true);
    }
}

/// A key, a click or the mouse on the clock.
#[tauri::command]
pub fn screensaver_dismiss(app: AppHandle) {
    *PHASE.lock().unwrap() = Phase::Away;
    hide(&app);
}

/// Settings' Preview: the clock now, until a key or the mouse. Async: off the
/// main thread, which a window's calls must not wait on from a command.
#[tauri::command]
pub async fn screensaver_preview(app: AppHandle) {
    *PHASE.lock().unwrap() = Phase::Shown(Instant::now());
    show(&app);
}

// ── The system: idle time, staying awake, the screens' power ──────────────────

#[cfg(target_os = "linux")]
mod os {
    use std::process::{Command, Stdio};
    use std::time::Duration;

    use dbus::blocking::Connection;

    const TIMEOUT: Duration = Duration::from_secs(2);

    /// GNOME's services on the session bus, X11's elsewhere. The connection
    /// stays: GNOME lets go of an inhibitor when its connection closes.
    pub struct System {
        bus: Option<Connection>,
        inhibit: Option<u32>,
    }

    impl System {
        pub fn new() -> System {
            System { bus: Connection::new_session().ok(), inhibit: None }
        }

        pub fn idle_ms(&mut self) -> Option<u64> {
            let gnome = self.bus.as_ref().and_then(|c| {
                let p = c.with_proxy("org.gnome.Mutter.IdleMonitor", "/org/gnome/Mutter/IdleMonitor/Core", TIMEOUT);
                p.method_call::<(u64,), _, _, _>("org.gnome.Mutter.IdleMonitor", "GetIdletime", ()).ok().map(|(ms,)| ms)
            });
            gnome.or_else(x11_idle_ms)
        }

        pub fn keep_awake(&mut self, on: bool) {
            let Some(c) = self.bus.as_ref() else { return };
            let p = c.with_proxy("org.gnome.SessionManager", "/org/gnome/SessionManager", TIMEOUT);
            match (on, self.inhibit) {
                (true, None) => {
                    // 8: idle, so the screen neither blanks nor locks.
                    let r: Result<(u32,), _> = p.method_call(
                        "org.gnome.SessionManager",
                        "Inhibit",
                        ("Ciel", 0u32, "The flip clock is showing", 8u32),
                    );
                    self.inhibit = r.ok().map(|(cookie,)| cookie);
                }
                (false, Some(cookie)) => {
                    let _: Result<(), _> = p.method_call("org.gnome.SessionManager", "Uninhibit", (cookie,));
                    self.inhibit = None;
                }
                _ => {}
            }
        }

        pub fn screens(&mut self, on: bool) {
            use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
            let gnome = self.bus.as_ref().is_some_and(|c| {
                let p = c.with_proxy("org.gnome.Mutter.DisplayConfig", "/org/gnome/Mutter/DisplayConfig", TIMEOUT);
                // 0: on, 3: off (DPMS).
                p.set("org.gnome.Mutter.DisplayConfig", "PowerSaveMode", if on { 0i32 } else { 3i32 }).is_ok()
            });
            if !gnome {
                let _ = Command::new("xset")
                    .args(["dpms", "force", if on { "on" } else { "off" }])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    /// X11's screensaver extension, for the desktops other than GNOME.
    fn x11_idle_ms() -> Option<u64> {
        use x11rb::connection::Connection as _;
        use x11rb::protocol::screensaver::ConnectionExt as _;
        let (conn, screen) = x11rb::connect(None).ok()?;
        let root = conn.setup().roots.get(screen)?.root;
        let info = conn.screensaver_query_info(root).ok()?.reply().ok()?;
        Some(u64::from(info.ms_since_user_input))
    }
}

#[cfg(windows)]
mod os {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED};
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetLastInputInfo, SendInput, INPUT, INPUT_0, INPUT_MOUSE, LASTINPUTINFO, MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND};

    pub struct System {
        awake: bool,
    }

    impl System {
        pub fn new() -> System {
            System { awake: false }
        }

        pub fn idle_ms(&mut self) -> Option<u64> {
            let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            unsafe { GetLastInputInfo(&mut info) }.as_bool().then(|| u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime)))
        }

        /// Held by this thread, which is the one deciding.
        pub fn keep_awake(&mut self, on: bool) {
            if on == self.awake {
                return;
            }
            self.awake = on;
            let flags = if on { ES_CONTINUOUS | ES_DISPLAY_REQUIRED } else { ES_CONTINUOUS };
            unsafe { SetThreadExecutionState(flags) };
        }

        pub fn screens(&mut self, on: bool) {
            // 2: off, -1: on.
            let state = if on { -1 } else { 2 };
            let _ = unsafe { PostMessageW(Some(HWND_BROADCAST), WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(state)) };
            if on {
                // A move of nothing: some displays only wake to input.
                let input = INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: MOUSEEVENTF_MOVE, ..Default::default() } },
                };
                unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(minutes: u32, night: Option<(&str, &str, u32)>) -> Settings {
        let mut s = Settings::default();
        s.screensaver_minutes = minutes;
        if let Some((from, until, after)) = night {
            s.night_off = true;
            s.night_from = from.into();
            s.night_until = until.into();
            s.night_off_after = after;
        }
        s
    }

    const MIN: u64 = 60_000;

    #[test]
    fn the_night_crosses_midnight_or_not() {
        let (eleven, six) = (minutes("23:00").unwrap(), minutes("06:00").unwrap());
        assert!(in_night(minutes("23:30").unwrap(), eleven, six));
        assert!(in_night(minutes("02:00").unwrap(), eleven, six));
        assert!(!in_night(minutes("06:00").unwrap(), eleven, six));
        assert!(!in_night(minutes("12:00").unwrap(), eleven, six));
        assert!(in_night(minutes("01:00").unwrap(), minutes("00:30").unwrap(), six));
        assert_eq!(minutes("24:00"), None);
        assert_eq!(minutes("7"), None);
    }

    #[test]
    fn the_clock_comes_after_the_idle_minutes_and_goes_with_the_user() {
        let s = settings(5, None);
        assert_eq!(next(Phase::Away, &s, 4 * MIN, 600), Phase::Away);
        let shown = next(Phase::Away, &s, 5 * MIN, 600);
        assert!(matches!(shown, Phase::Shown(_)));
        // Still idle since before it came up: it stays.
        assert!(matches!(next(shown, &s, 6 * MIN, 600), Phase::Shown(_)));
        // Input after it came up: gone.
        let earlier = Phase::Shown(Instant::now() - Duration::from_secs(60));
        assert_eq!(next(earlier, &s, 500, 600), Phase::Away);
        // Off in Settings: nothing.
        assert_eq!(next(earlier, &settings(0, None), 10 * MIN, 600), Phase::Away);
    }

    #[test]
    fn at_night_the_screens_go_off_after_the_clock_and_come_back_in_the_morning() {
        let s = settings(5, Some(("23:00", "06:00", 60)));
        let night = minutes("23:30").unwrap();
        let long = Phase::Shown(Instant::now() - Duration::from_secs(2 * 3600));
        let short = Phase::Shown(Instant::now() - Duration::from_secs(10 * 60));
        assert!(matches!(next(short, &s, 3 * 3600_000, night), Phase::Shown(_)));
        let off = next(long, &s, 3 * 3600_000, night);
        assert!(matches!(off, Phase::Off(_)));
        // The day, before the night: the clock stays, however long.
        assert!(matches!(next(long, &s, 3 * 3600_000, minutes("20:00").unwrap()), Phase::Shown(_)));
        // Still night: off. Morning: the clock again.
        let asleep = Phase::Off(Instant::now() - Duration::from_secs(3600));
        assert!(matches!(next(asleep, &s, 2 * 3600_000, minutes("03:00").unwrap()), Phase::Off(_)));
        assert!(matches!(next(asleep, &s, 8 * 3600_000, minutes("06:00").unwrap()), Phase::Shown(_)));
        // Someone woke the screens in the night: no clock.
        assert_eq!(next(asleep, &s, 300, minutes("03:00").unwrap()), Phase::Away);
    }
}
