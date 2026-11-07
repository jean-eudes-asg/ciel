// Quick controls (Tools → Controls): the speakers' volume, every microphone at
// once (with a key of its own, Settings → Shortcuts), the screens' brightness,
// Wi-Fi, Bluetooth, night light where the desktop has one, and keeping the
// computer awake.
//
// Read when the island asks (the controls coming on screen, the island
// opening), never on a timer. Windows: the Core Audio endpoints, WMI for a
// laptop's screen and DDC/CI for the monitors, Windows.Devices.Radios, and an
// execution state held by a thread of its own. Linux: PulseAudio's pactl,
// logind's SetBrightness on /sys/class/backlight, NetworkManager's nmcli,
// bluetoothctl, GNOME's night light, and systemd-inhibit.

use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const EVENT: &str = "controls";

/// What the controls show. None: this computer has no such thing, or it can't be reached.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Controls {
    /// 0…1.
    pub volume: Option<f64>,
    pub muted: Option<bool>,
    /// Every microphone muted (as far as the default one says).
    pub mic_muted: Option<bool>,
    pub mics: u32,
    /// 0…1.
    pub brightness: Option<f64>,
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub night_light: Option<bool>,
    /// Kept awake: until then (Unix ms), or for good (f64::MAX → `awakeForever`).
    pub awake: bool,
    pub awake_until: Option<f64>,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
/// The last read, so a change from one control comes back with the others as they were.
static LAST: Mutex<Option<Controls>> = Mutex::new(None);
/// Keep awake: until when, and the generation that ends it.
static AWAKE: Mutex<Option<(f64, u64)>> = Mutex::new(None);
static AWAKE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

pub fn setup(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

fn read_all() -> Controls {
    let (volume, muted) = platform::volume().map_or((None, None), |(v, m)| (Some(v), Some(m)));
    let (mic_muted, mics) = platform::mics();
    let (wifi, bluetooth) = platform::radios();
    let awake = *AWAKE.lock().unwrap();
    let c = Controls {
        volume,
        muted,
        mic_muted,
        mics,
        brightness: platform::brightness(),
        wifi,
        bluetooth,
        night_light: platform::night_light(),
        awake: awake.is_some(),
        awake_until: awake.map(|(until, _)| until).filter(|u| *u < f64::MAX),
    };
    *LAST.lock().unwrap() = Some(c.clone());
    c
}

fn emit(c: &Controls) {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, c);
    }
}

/// All of them, or with `light` only the sound and the microphones (at
/// launch: DDC/CI and the radios take a moment, and only "Mic off" matters then).
#[tauri::command]
pub async fn controls_state(light: Option<bool>) -> Controls {
    let c = tauri::async_runtime::spawn_blocking(move || if light == Some(true) { read_light() } else { read_all() })
        .await
        .unwrap_or_default();
    emit(&c);
    c
}

fn read_light() -> Controls {
    let mut c = LAST.lock().unwrap().clone().unwrap_or_default();
    if let Some((v, m)) = platform::volume() {
        c.volume = Some(v);
        c.muted = Some(m);
    }
    (c.mic_muted, c.mics) = platform::mics();
    *LAST.lock().unwrap() = Some(c.clone());
    c
}

/// "volume" (0…1), "muted", "mic", "brightness" (0…1), "wifi", "bluetooth",
/// "nightLight" (1 on, 0 off). Returns the controls as they are after.
#[tauri::command]
pub async fn control_set(what: String, value: f64) -> Controls {
    let c = tauri::async_runtime::spawn_blocking(move || {
        let on = value >= 0.5;
        let done = match what.as_str() {
            "volume" => platform::set_volume(value.clamp(0.0, 1.0)),
            "muted" => platform::set_muted(on),
            "mic" => platform::set_mics_muted(on),
            "brightness" => platform::set_brightness(value.clamp(0.0, 1.0)),
            "wifi" => platform::set_wifi(on),
            "bluetooth" => platform::set_bluetooth(on),
            "nightLight" => platform::set_night_light(on),
            _ => false,
        };
        if !done {
            crate::log::line(format!("controls: could not set {what}"));
        }
        // Only what changed is read again: DDC/CI and the radios are slow.
        let mut c = LAST.lock().unwrap().clone().unwrap_or_else(read_all);
        match what.as_str() {
            "volume" | "muted" => {
                if let Some((v, m)) = platform::volume() {
                    c.volume = Some(v);
                    c.muted = Some(m);
                }
            }
            "mic" => (c.mic_muted, c.mics) = platform::mics(),
            "brightness" if done => c.brightness = Some(value.clamp(0.0, 1.0)),
            "wifi" | "bluetooth" => (c.wifi, c.bluetooth) = platform::radios(),
            "nightLight" => c.night_light = platform::night_light(),
            _ => {}
        }
        *LAST.lock().unwrap() = Some(c.clone());
        c
    })
    .await
    .unwrap_or_default();
    emit(&c);
    c
}

/// The microphone key: every microphone muted, or back on. Returns the new state.
#[tauri::command]
pub async fn mic_toggle() -> Option<bool> {
    let c = tauri::async_runtime::spawn_blocking(|| {
        let (muted, _) = platform::mics();
        let next = !muted.unwrap_or(false);
        platform::set_mics_muted(next);
        let mut c = LAST.lock().unwrap().clone().unwrap_or_default();
        (c.mic_muted, c.mics) = platform::mics();
        *LAST.lock().unwrap() = Some(c.clone());
        c
    })
    .await
    .ok()?;
    emit(&c);
    c.mic_muted
}

/// Keeps the computer (and its screens) awake for `minutes`; 0 lets it sleep
/// again, a negative number keeps it awake until turned off.
#[tauri::command]
pub fn awake_set(minutes: f64) -> Controls {
    let generation = AWAKE_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    if minutes == 0.0 {
        *AWAKE.lock().unwrap() = None;
        platform::stay_awake(false);
    } else {
        let until = if minutes < 0.0 { f64::MAX } else { now_ms() + minutes.min(24.0 * 60.0) * 60_000.0 };
        *AWAKE.lock().unwrap() = Some((until, generation));
        platform::stay_awake(true);
        if until < f64::MAX {
            let wait = (until - now_ms()).max(0.0) as u64;
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                let mine = matches!(*AWAKE.lock().unwrap(), Some((_, g)) if g == generation);
                if mine {
                    awake_set(0.0);
                }
            });
        }
    }
    let mut c = LAST.lock().unwrap().clone().unwrap_or_default();
    let awake = *AWAKE.lock().unwrap();
    c.awake = awake.is_some();
    c.awake_until = awake.map(|(u, _)| u).filter(|u| *u < f64::MAX);
    *LAST.lock().unwrap() = Some(c.clone());
    emit(&c);
    c
}

/// The brain's `controls` tool: the microphones, the volume, keeping awake.
pub fn for_brain(input: &serde_json::Value) -> (String, bool) {
    let action = input["action"].as_str().unwrap_or("status");
    let value = input["value"].as_f64().unwrap_or(0.0);
    let (said, acted) = match action {
        "mute_mic" => (if platform::set_mics_muted(true) { "Every microphone is muted." } else { "The microphones could not be muted." }, true),
        "unmute_mic" => (if platform::set_mics_muted(false) { "The microphones are on again." } else { "The microphones could not be turned on." }, true),
        "volume" => (if platform::set_volume((value / 100.0).clamp(0.0, 1.0)) { "Volume set." } else { "The volume could not be set." }, true),
        "mute" => (if platform::set_muted(true) { "Sound muted." } else { "The sound could not be muted." }, true),
        "unmute" => (if platform::set_muted(false) { "Sound back on." } else { "The sound could not be turned on." }, true),
        "keep_awake" => {
            awake_set(if value == 0.0 { 0.0 } else if value < 0.0 { -1.0 } else { value });
            (if value == 0.0 { "The computer may sleep again." } else { "The computer stays awake." }, true)
        }
        _ => ("", false),
    };
    let c = read_all();
    emit(&c);
    if acted {
        return (said.to_string(), true);
    }
    let pct = |v: Option<f64>| v.map(|x| (x * 100.0).round());
    (
        serde_json::json!({
            "volume": pct(c.volume), "soundMuted": c.muted, "microphonesMuted": c.mic_muted, "microphones": c.mics,
            "brightness": pct(c.brightness), "wifi": c.wifi, "bluetooth": c.bluetooth, "keptAwake": c.awake,
        })
        .to_string(),
        false,
    )
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod platform {
    use windows::core::{w, BSTR};
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_ALL, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
    };
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };
    use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
    use windows::Win32::System::Variant::{VARIANT, VT_UI1};
    use windows::Win32::System::Wmi::{
        IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
        WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
    };

    fn com() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
    }

    fn devices() -> Option<IMMDeviceEnumerator> {
        com();
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
    }

    fn speakers() -> Option<IAudioEndpointVolume> {
        unsafe { devices()?.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()?.Activate(CLSCTX_ALL, None).ok() }
    }

    pub fn volume() -> Option<(f64, bool)> {
        let v = speakers()?;
        unsafe { Some((v.GetMasterVolumeLevelScalar().ok()? as f64, v.GetMute().ok()?.as_bool())) }
    }

    pub fn set_volume(level: f64) -> bool {
        let Some(v) = speakers() else { return false };
        unsafe {
            // Turning the sound up brings it back from mute, as the taskbar's slider does.
            if level > 0.0 {
                let _ = v.SetMute(false, std::ptr::null());
            }
            v.SetMasterVolumeLevelScalar(level as f32, std::ptr::null()).is_ok()
        }
    }

    pub fn set_muted(on: bool) -> bool {
        speakers().is_some_and(|v| unsafe { v.SetMute(on, std::ptr::null()).is_ok() })
    }

    /// Every microphone plugged in.
    fn all_mics() -> Vec<IAudioEndpointVolume> {
        let mut out = Vec::new();
        let Some(devices) = devices() else { return out };
        unsafe {
            let Ok(list) = devices.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) else { return out };
            for i in 0..list.GetCount().unwrap_or(0) {
                if let Some(v) = list.Item(i).ok().and_then(|d| d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()) {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Whether the microphones are muted (the default one for calls says), and how many there are.
    pub fn mics() -> (Option<bool>, u32) {
        let all = all_mics();
        let default = devices().and_then(|d| unsafe {
            d.GetDefaultAudioEndpoint(eCapture, eCommunications).ok()?.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
        });
        let muted = default.or_else(|| all.first().cloned()).and_then(|v| unsafe { v.GetMute().ok() }).map(|b| b.as_bool());
        (muted, all.len() as u32)
    }

    pub fn set_mics_muted(on: bool) -> bool {
        let all = all_mics();
        let mut done = false;
        for v in &all {
            done |= unsafe { v.SetMute(on, std::ptr::null()).is_ok() };
        }
        done
    }

    // A laptop's own screen, through WMI.

    fn wmi() -> Option<IWbemServices> {
        com();
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
            let empty = BSTR::new();
            let services =
                locator.ConnectServer(&BSTR::from("ROOT\\WMI"), &empty, &empty, &empty, 0, &empty, None).ok()?;
            let _ = CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                None,
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            );
            Some(services)
        }
    }

    fn first(services: &IWbemServices, query: &str) -> Option<IWbemClassObject> {
        unsafe {
            let list = services
                .ExecQuery(
                    &BSTR::from("WQL"),
                    &BSTR::from(query),
                    WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_FORWARD_ONLY.0 | WBEM_FLAG_RETURN_IMMEDIATELY.0),
                    None,
                )
                .ok()?;
            let mut row = [None];
            let mut got = 0u32;
            let _ = list.Next(WBEM_INFINITE, &mut row, &mut got);
            if got == 0 {
                return None;
            }
            row[0].take()
        }
    }

    fn laptop_brightness() -> Option<f64> {
        let services = wmi()?;
        let row = first(&services, "SELECT CurrentBrightness FROM WmiMonitorBrightness")?;
        let mut value = VARIANT::default();
        unsafe {
            row.Get(w!("CurrentBrightness"), 0, &mut value, None, None).ok()?;
            let level = if value.vt() == VT_UI1 {
                value.Anonymous.Anonymous.Anonymous.bVal as u32
            } else {
                u32::try_from(&value).ok()?
            };
            Some((level.min(100)) as f64 / 100.0)
        }
    }

    fn set_laptop_brightness(level: f64) -> bool {
        let Some(services) = wmi() else { return false };
        let Some(row) = first(&services, "SELECT * FROM WmiMonitorBrightnessMethods") else { return false };
        unsafe {
            let mut path = VARIANT::default();
            if row.Get(w!("__PATH"), 0, &mut path, None, None).is_err() {
                return false;
            }
            let Ok(path) = BSTR::try_from(&path) else { return false };
            let mut class = None;
            if services
                .GetObject(&BSTR::from("WmiMonitorBrightnessMethods"), WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)
                .is_err()
            {
                return false;
            }
            let Some(class) = class else { return false };
            let mut input = None;
            if class.GetMethod(w!("WmiSetBrightness"), 0, &mut input, std::ptr::null_mut()).is_err() {
                return false;
            }
            let Some(params) = input.and_then(|s| s.SpawnInstance(0).ok()) else { return false };
            let timeout = VARIANT::from(0i32);
            let brightness = VARIANT::from((level * 100.0).round().clamp(0.0, 100.0) as u8);
            if params.Put(w!("Timeout"), 0, &timeout, 0).is_err() || params.Put(w!("Brightness"), 0, &brightness, 0).is_err() {
                return false;
            }
            services
                .ExecMethod(&path, &BSTR::from("WmiSetBrightness"), WBEM_GENERIC_FLAG_TYPE(0), None, &params, None, None)
                .is_ok()
        }
    }

    // Monitors, through DDC/CI (slow: tens of ms each).

    fn physical_monitors() -> Vec<Vec<PHYSICAL_MONITOR>> {
        unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
            let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
            list.push(monitor);
            true.into()
        }
        let mut monitors: Vec<HMONITOR> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut monitors as *mut _ as isize));
        }
        let mut out = Vec::new();
        for monitor in monitors {
            let mut count = 0u32;
            unsafe {
                if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count).is_err() || count == 0 {
                    continue;
                }
                let mut list = vec![PHYSICAL_MONITOR::default(); count as usize];
                if GetPhysicalMonitorsFromHMONITOR(monitor, &mut list).is_ok() {
                    out.push(list);
                }
            }
        }
        out
    }

    fn with_monitors<T>(f: impl Fn(&PHYSICAL_MONITOR) -> Option<T>) -> Vec<T> {
        let mut out = Vec::new();
        for list in physical_monitors() {
            for m in &list {
                if let Some(v) = f(m) {
                    out.push(v);
                }
            }
            unsafe {
                let _ = DestroyPhysicalMonitors(&list);
            }
        }
        out
    }

    pub fn brightness() -> Option<f64> {
        laptop_brightness().or_else(|| {
            with_monitors(|m| {
                let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
                let ok = unsafe { GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max) } != 0;
                (ok && max > min).then(|| (cur.saturating_sub(min)) as f64 / (max - min) as f64)
            })
            .into_iter()
            .next()
        })
    }

    pub fn set_brightness(level: f64) -> bool {
        let laptop = set_laptop_brightness(level);
        let monitors = with_monitors(|m| {
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            unsafe {
                if GetMonitorBrightness(m.hPhysicalMonitor, &mut min, &mut cur, &mut max) == 0 || max <= min {
                    return None;
                }
                let value = min + ((max - min) as f64 * level).round() as u32;
                Some(SetMonitorBrightness(m.hPhysicalMonitor, value) != 0)
            }
        });
        laptop || monitors.into_iter().any(|ok| ok)
    }

    // Wi-Fi and Bluetooth.

    fn radio_list() -> Vec<Radio> {
        Radio::GetRadiosAsync().and_then(|op| op.get()).map(|v| v.into_iter().collect()).unwrap_or_default()
    }

    pub fn radios() -> (Option<bool>, Option<bool>) {
        let (mut wifi, mut bt) = (None, None);
        for r in radio_list() {
            let on = r.State().ok().map(|s| s == RadioState::On);
            match r.Kind() {
                Ok(RadioKind::WiFi) => wifi = wifi.or(on),
                Ok(RadioKind::Bluetooth) => bt = bt.or(on),
                _ => {}
            }
        }
        (wifi, bt)
    }

    fn set_radio(kind: RadioKind, on: bool) -> bool {
        let _ = Radio::RequestAccessAsync().and_then(|op| op.get());
        let state = if on { RadioState::On } else { RadioState::Off };
        let mut done = false;
        for r in radio_list() {
            if r.Kind().ok() == Some(kind) {
                done |= r.SetStateAsync(state).and_then(|op| op.get()).is_ok_and(|s| s == RadioAccessStatus::Allowed);
            }
        }
        done
    }

    pub fn set_wifi(on: bool) -> bool {
        set_radio(RadioKind::WiFi, on)
    }

    pub fn set_bluetooth(on: bool) -> bool {
        set_radio(RadioKind::Bluetooth, on)
    }

    // Windows has no night light for apps to switch.

    pub fn night_light() -> Option<bool> {
        None
    }

    pub fn set_night_light(_: bool) -> bool {
        false
    }

    // Awake: an execution state lives with the thread that set it.

    static HOLDER: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>> = std::sync::Mutex::new(None);

    pub fn stay_awake(on: bool) {
        let mut holder = HOLDER.lock().unwrap();
        if let Some(stop) = holder.take() {
            let _ = stop.send(());
        }
        if !on {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let spawned = std::thread::Builder::new().name("ciel-awake".into()).spawn(move || {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED);
            }
            let _ = rx.recv();
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        });
        if spawned.is_ok() {
            *holder = Some(tx);
        }
    }
}

// ── Linux ─────────────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod platform {
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
    }

    fn ok(cmd: &str, args: &[&str]) -> bool {
        Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// "Volume: front-left: 26214 /  40% / …" → 0.4.
    pub fn first_percent(text: &str) -> Option<f64> {
        let at = text.find('%')?;
        let digits: String = text[..at].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        digits.parse::<f64>().ok().map(|p| p / 100.0)
    }

    pub fn volume() -> Option<(f64, bool)> {
        let level = first_percent(&run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?)?;
        let muted = run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])?.contains("yes");
        Some((level, muted))
    }

    pub fn set_volume(level: f64) -> bool {
        let percent = format!("{}%", (level * 100.0).round() as u32);
        if level > 0.0 {
            ok("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        }
        ok("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &percent])
    }

    pub fn set_muted(on: bool) -> bool {
        ok("pactl", &["set-sink-mute", "@DEFAULT_SINK@", if on { "1" } else { "0" }])
    }

    /// The microphones: every source that is not a speaker's monitor.
    fn sources() -> Vec<String> {
        run("pactl", &["list", "short", "sources"])
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
            .filter(|name| !name.ends_with(".monitor"))
            .collect()
    }

    pub fn mics() -> (Option<bool>, u32) {
        let count = sources().len() as u32;
        let muted = run("pactl", &["get-source-mute", "@DEFAULT_SOURCE@"]).map(|o| o.contains("yes"));
        (muted, count)
    }

    pub fn set_mics_muted(on: bool) -> bool {
        let flag = if on { "1" } else { "0" };
        let mut done = false;
        for name in sources() {
            done |= ok("pactl", &["set-source-mute", &name, flag]);
        }
        done
    }

    /// The first backlight: its name, level and maximum.
    fn backlight() -> Option<(String, u32, u32)> {
        let dir = std::fs::read_dir("/sys/class/backlight").ok()?;
        for entry in dir.flatten() {
            let read = |f: &str| std::fs::read_to_string(entry.path().join(f)).ok()?.trim().parse::<u32>().ok();
            if let (Some(level), Some(max)) = (read("brightness"), read("max_brightness")) {
                if max > 0 {
                    return Some((entry.file_name().to_string_lossy().to_string(), level, max));
                }
            }
        }
        None
    }

    pub fn brightness() -> Option<f64> {
        backlight().map(|(_, level, max)| level as f64 / max as f64)
    }

    /// logind lets the session's own user set the backlight, no root needed.
    pub fn set_brightness(level: f64) -> bool {
        let Some((name, _, max)) = backlight() else { return false };
        // Never fully dark: the screen would look off.
        let value = ((max as f64 * level).round() as u32).max(1);
        let Ok(conn) = dbus::blocking::Connection::new_system() else { return false };
        let proxy = conn.with_proxy(
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            std::time::Duration::from_secs(2),
        );
        proxy
            .method_call::<(), _, _, _>("org.freedesktop.login1.Session", "SetBrightness", ("backlight", name.as_str(), value))
            .is_ok()
    }

    pub fn radios() -> (Option<bool>, Option<bool>) {
        let wifi = run("nmcli", &["radio", "wifi"]).map(|o| o.trim() == "enabled");
        let bt = run("bluetoothctl", &["show"]).filter(|o| o.contains("Controller")).map(|o| o.contains("Powered: yes"));
        (wifi, bt)
    }

    pub fn set_wifi(on: bool) -> bool {
        ok("nmcli", &["radio", "wifi", if on { "on" } else { "off" }])
    }

    pub fn set_bluetooth(on: bool) -> bool {
        ok("bluetoothctl", &["power", if on { "on" } else { "off" }])
    }

    const NIGHT: [&str; 2] = ["org.gnome.settings-daemon.plugins.color", "night-light-enabled"];

    pub fn night_light() -> Option<bool> {
        run("gsettings", &["get", NIGHT[0], NIGHT[1]]).map(|o| o.trim() == "true")
    }

    pub fn set_night_light(on: bool) -> bool {
        ok("gsettings", &["set", NIGHT[0], NIGHT[1], if on { "true" } else { "false" }])
    }

    static INHIBIT: Mutex<Option<Child>> = Mutex::new(None);

    /// systemd-inhibit holds off sleep and the screen going blank while it runs.
    pub fn stay_awake(on: bool) {
        let mut slot = INHIBIT.lock().unwrap();
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if !on {
            return;
        }
        let child = Command::new("systemd-inhibit")
            .args(["--what=idle:sleep", "--who=Ciel", "--why=Keep awake", "--mode=block", "sleep", "infinity"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(child) => *slot = Some(child),
            Err(err) => crate::log::line(format!("controls: no systemd-inhibit: {err}")),
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn pactl_volumes_read_as_a_fraction() {
            assert_eq!(super::first_percent("Volume: front-left: 26214 /  40% / -23.88 dB,"), Some(0.4));
            assert_eq!(super::first_percent("Volume: mono: 65536 / 100% / 0.00 dB"), Some(1.0));
            assert_eq!(super::first_percent("nothing"), None);
        }
    }
}
