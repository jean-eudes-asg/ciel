// Quick controls (Tools → Controls), what a desktop's quick settings hold and
// more: the speakers' volume and which speakers, every microphone at once
// (with a key of its own, Settings → Shortcuts), the screens' brightness,
// the network and its connections, Wi-Fi, Bluetooth, the power mode, night
// light, dark style, do not disturb, the battery, and keeping the computer
// awake. The buttons beside them (screenshot, lock, power) are quick.rs.
//
// Read when the island asks (the controls coming on screen, the island
// opening), never on a timer. Windows: the Core Audio endpoints (and the
// policy object the sound settings use to pick the speakers), WMI for a
// laptop's screen and DDC/CI for the monitors, Windows.Devices.Radios, the
// connection profile, the power mode overlays, the theme in the registry,
// and an execution state held by a thread of its own. Linux: PipeWire's wpctl
// (PulseAudio's pactl without it), logind's SetBrightness on
// /sys/class/backlight, NetworkManager's nmcli, bluetoothctl,
// power-profiles-daemon, GNOME's settings, /sys/class/power_supply, and
// systemd-inhibit.

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
    /// The speakers to choose from (empty: none to choose, or the system's own settings choose).
    pub outputs: Vec<Device>,
    /// None: no network manager to ask.
    pub network: Option<Network>,
    /// The connections set up (wired, Wi-Fi, VPN), to bring up or down.
    pub connections: Vec<Connection>,
    /// "power-saver", "balanced" or "performance", and those this computer has.
    pub power_mode: Option<String>,
    pub power_modes: Vec<String>,
    pub dark: Option<bool>,
    /// Notifications held: the desktop's banners (GNOME) and Ciel's own cards.
    pub dnd: bool,
    pub battery: Option<Battery>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Network {
    /// "wired", "wifi", "mobile", or "" when not connected.
    pub kind: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub name: String,
    /// "wired", "wifi" or "vpn".
    pub kind: String,
    pub active: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    /// 0…100.
    pub percent: f64,
    pub charging: bool,
}

/// Do not disturb where the desktop has no switch of its own (Windows): Ciel's cards only.
static DND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn dnd() -> bool {
    platform::dnd().unwrap_or_else(|| DND.load(std::sync::atomic::Ordering::Relaxed))
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
    let (network, connections) = platform::network();
    let (power_mode, power_modes) = platform::power_mode().map_or((None, Vec::new()), |(m, all)| (Some(m), all));
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
        outputs: platform::outputs(),
        network,
        connections,
        power_mode,
        power_modes,
        dark: platform::dark(),
        dnd: dnd(),
        battery: platform::battery(),
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
    // Ciel's cards wait from the start when it is on.
    c.dnd = dnd();
    *LAST.lock().unwrap() = Some(c.clone());
    c
}

/// "volume" (0…1), "muted", "mic", "brightness" (0…1), "wifi", "bluetooth",
/// "nightLight", "dark", "dnd" (1 on, 0 off). Returns the controls as they are after.
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
            "dark" => platform::set_dark(on),
            "dnd" => {
                DND.store(on, std::sync::atomic::Ordering::Relaxed);
                platform::set_dnd(on);
                true
            }
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
            "dark" => c.dark = platform::dark(),
            "dnd" => c.dnd = dnd(),
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

/// A choice from a list: "output" (the speakers' id), "powerMode" (its name),
/// "connection" (brought up, or down when it is up). Only what the list holds
/// is taken. Returns the controls as they are after.
#[tauri::command]
pub async fn control_choose(what: String, id: String) -> Controls {
    let c = tauri::async_runtime::spawn_blocking(move || {
        let done = match what.as_str() {
            "output" => platform::outputs().iter().any(|d| d.id == id) && platform::set_output(&id),
            "powerMode" => platform::power_mode().is_some_and(|(_, all)| all.contains(&id)) && platform::set_power_mode(&id),
            "connection" => platform::network().1.iter().find(|x| x.id == id).is_some_and(|x| platform::set_connection(&x.id, !x.active)),
            _ => false,
        };
        if !done {
            crate::log::line(format!("controls: could not choose {what}"));
        }
        let mut c = LAST.lock().unwrap().clone().unwrap_or_else(read_all);
        match what.as_str() {
            "output" => {
                c.outputs = platform::outputs();
                if let Some((v, m)) = platform::volume() {
                    c.volume = Some(v);
                    c.muted = Some(m);
                }
            }
            "powerMode" => (c.power_mode, c.power_modes) = platform::power_mode().map_or((None, Vec::new()), |(m, all)| (Some(m), all)),
            "connection" => (c.network, c.connections) = platform::network(),
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
            "powerMode": c.power_mode, "darkStyle": c.dark, "doNotDisturb": c.dnd,
            "network": c.network.as_ref().map(|n| if n.kind.is_empty() { "not connected".to_string() } else { format!("{} {}", n.kind, n.name) }),
            "battery": c.battery.as_ref().map(|b| format!("{}%{}", b.percent.round(), if b.charging { " charging" } else { "" })),
        })
        .to_string(),
        false,
    )
}

// ── Windows ───────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod platform {
    use windows::core::{s, w, BSTR, GUID, HSTRING, PCSTR, PCWSTR};
    use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::System::Com::{CoTaskMemFree, STGM_READ};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::Registry::{RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    use windows::Win32::Devices::Display::{
        DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
        GetPhysicalMonitorsFromHMONITOR, SetMonitorBrightness, PHYSICAL_MONITOR,
    };
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, eConsole, eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE,
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

    // The speakers: the active ones by name, and the default one. Picking one
    // goes through the policy object Windows' own sound settings use: not
    // documented, but the same since Windows 7 (EarTrumpet and SoundSwitch use it).

    pub fn outputs() -> Vec<super::Device> {
        let Some(devices) = devices() else { return Vec::new() };
        let id_of = |d: &IMMDevice| unsafe {
            let p = d.GetId().ok()?;
            let id = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            id
        };
        unsafe {
            let default = devices.GetDefaultAudioEndpoint(eRender, eMultimedia).ok().and_then(|d| id_of(&d));
            let Ok(list) = devices.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return Vec::new() };
            let all: Vec<super::Device> = (0..list.GetCount().unwrap_or(0))
                .filter_map(|i| {
                    let d = list.Item(i).ok()?;
                    let id = id_of(&d)?;
                    let value = d.OpenPropertyStore(STGM_READ).ok()?.GetValue(&PKEY_Device_FriendlyName).ok()?;
                    let name = BSTR::try_from(&value).ok()?.to_string();
                    Some(super::Device { default: default.as_deref() == Some(id.as_str()), id, name })
                })
                .collect();
            if all.len() < 2 { Vec::new() } else { all }
        }
    }

    #[allow(non_snake_case)]
    mod policy {
        use std::ffi::c_void;

        use windows::core::{HRESULT, PCWSTR};
        use windows::Win32::Media::Audio::ERole;

        #[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
        pub unsafe trait IPolicyConfig: windows::core::IUnknown {
            // Only SetDefaultEndpoint is called; the others hold their places.
            fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut c_void) -> HRESULT;
            fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut c_void) -> HRESULT;
            fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
            fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
            fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, period: *mut i64, minimum: *mut i64) -> HRESULT;
            fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
            fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
            fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
            fn GetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
            fn SetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
            fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
            fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
        }

        /// The macro keeps the methods to this module.
        pub fn set_default(policy: &IPolicyConfig, id: PCWSTR, role: ERole) -> bool {
            unsafe { policy.SetDefaultEndpoint(id, role).is_ok() }
        }
    }
    use policy::IPolicyConfig;

    const POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

    pub fn set_output(id: &str) -> bool {
        com();
        let Ok(policy) = (unsafe { CoCreateInstance::<_, IPolicyConfig>(&POLICY_CONFIG, None, CLSCTX_ALL) }) else { return false };
        let id = HSTRING::from(id);
        // For music, for calls, and for the rest: as the sound settings do.
        [eConsole, eMultimedia, eCommunications]
            .into_iter()
            .all(|role| policy::set_default(&policy, PCWSTR(id.as_ptr()), role))
    }

    /// The connection to the internet: Wi-Fi by its name, or wired. Windows'
    /// own list of networks does the choosing (quick.rs opens it).
    pub fn network() -> (Option<super::Network>, Vec<super::Connection>) {
        let network = match NetworkInformation::GetInternetConnectionProfile() {
            Ok(p) => {
                let kind = if p.IsWlanConnectionProfile().unwrap_or(false) {
                    "wifi"
                } else if p.IsWwanConnectionProfile().unwrap_or(false) {
                    "mobile"
                } else {
                    "wired"
                };
                super::Network { kind: kind.into(), name: p.ProfileName().map(|n| n.to_string()).unwrap_or_default() }
            }
            Err(_) => super::Network { kind: String::new(), name: String::new() },
        };
        (Some(network), Vec::new())
    }

    pub fn set_connection(_: &str, _: bool) -> bool {
        false
    }

    // The power mode (Settings → System → Power): the overlays powrprof.dll
    // switches for the Settings app, looked up by name since they aren't in
    // the headers. They apply on the Balanced plan, which most computers use.

    const BEST_EFFICIENCY: GUID = GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a);
    const BEST_PERFORMANCE: GUID = GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238);
    const MODES: [(&str, GUID); 3] = [("power-saver", BEST_EFFICIENCY), ("balanced", GUID::zeroed()), ("performance", BEST_PERFORMANCE)];

    fn powrprof(name: PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
        unsafe { GetProcAddress(LoadLibraryW(w!("powrprof.dll")).ok()?, name) }
    }

    pub fn power_mode() -> Option<(String, Vec<String>)> {
        type Get = unsafe extern "system" fn(*mut GUID) -> u32;
        let get: Get = unsafe { std::mem::transmute(powrprof(s!("PowerGetEffectiveOverlayScheme"))?) };
        let mut guid = GUID::zeroed();
        if unsafe { get(&mut guid) } != 0 {
            return None;
        }
        let mode = MODES.iter().find(|(_, g)| *g == guid).map_or("balanced", |(m, _)| m);
        Some((mode.to_string(), MODES.iter().map(|(m, _)| m.to_string()).collect()))
    }

    pub fn set_power_mode(mode: &str) -> bool {
        type Set = unsafe extern "system" fn(GUID) -> u32;
        let Some(&(_, guid)) = MODES.iter().find(|(m, _)| *m == mode) else { return false };
        let Some(set) = powrprof(s!("PowerSetActiveOverlayScheme")) else { return false };
        let set: Set = unsafe { std::mem::transmute(set) };
        unsafe { set(guid) == 0 }
    }

    // Dark style: the theme's two values in the registry (apps, and the
    // taskbar with the Start menu), then every window is told, as Settings does.

    const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

    pub fn dark() -> Option<bool> {
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let read = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, PERSONALIZE, w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as _), Some(&mut size))
        };
        read.is_ok().then_some(value == 0)
    }

    pub fn set_dark(on: bool) -> bool {
        let light = u32::from(!on);
        let bytes = light.to_le_bytes();
        let mut done = true;
        for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
            done &= unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, PERSONALIZE, name, REG_DWORD.0, Some(bytes.as_ptr() as _), 4) }.is_ok();
        }
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                200,
                None,
            );
        }
        done
    }

    // Windows' own do not disturb has no switch for apps: Ciel's cards only (controls.rs).

    pub fn dnd() -> Option<bool> {
        None
    }

    pub fn set_dnd(_: bool) {}

    pub fn battery() -> Option<super::Battery> {
        let mut s = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut s) }.ok()?;
        // 128: no battery; 255: unknown.
        if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent == 255 {
            return None;
        }
        Some(super::Battery { percent: s.BatteryLifePercent as f64, charging: s.ACLineStatus == 1 })
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

    /// PipeWire's own wpctl, which Ubuntu and Fedora ship without pactl.
    fn pipewire() -> bool {
        static HAS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *HAS.get_or_init(|| crate::platform::find_on_path("wpctl").is_some())
    }

    /// "Volume: 0.84 [MUTED]" → (0.84, true).
    pub fn wp_volume(text: &str) -> Option<(f64, bool)> {
        let level = text.trim().strip_prefix("Volume:")?.split_whitespace().next()?.parse::<f64>().ok()?;
        Some((level.min(1.0), text.contains("[MUTED]")))
    }

    /// The Audio section's sinks or sources in `wpctl status`: id, name, the default one.
    pub fn wp_nodes(status: &str, section: &str) -> Vec<(u32, String, bool)> {
        let mut out = Vec::new();
        let (mut audio, mut inside) = (false, false);
        for line in status.lines() {
            // "Audio", "Video", "Settings": the top level, at the margin.
            if !line.starts_with([' ', '│', '├', '└']) {
                audio = line.trim() == "Audio";
                inside = false;
                continue;
            }
            let body = line.trim_start_matches([' ', '│', '├', '└', '─']).trim_end();
            if body.ends_with(':') {
                inside = audio && body.strip_suffix(':') == Some(section);
                continue;
            }
            if body.is_empty() {
                inside = false;
                continue;
            }
            if !inside {
                continue;
            }
            let default = body.starts_with('*');
            let Some((id, rest)) = body.trim_start_matches('*').trim_start().split_once(". ") else { continue };
            let Ok(id) = id.parse::<u32>() else { continue };
            let name = rest.split(" [vol:").next().unwrap_or(rest).trim().to_string();
            out.push((id, name, default));
        }
        out
    }

    fn wp_status() -> String {
        run("wpctl", &["status", "--nick"]).or_else(|| run("wpctl", &["status"])).unwrap_or_default()
    }

    pub fn volume() -> Option<(f64, bool)> {
        if pipewire() {
            return wp_volume(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?);
        }
        let level = first_percent(&run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"])?)?;
        let muted = run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"])?.contains("yes");
        Some((level, muted))
    }

    pub fn set_volume(level: f64) -> bool {
        if pipewire() {
            if level > 0.0 {
                ok("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"]);
            }
            return ok("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")]);
        }
        let percent = format!("{}%", (level * 100.0).round() as u32);
        if level > 0.0 {
            ok("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        }
        ok("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &percent])
    }

    pub fn set_muted(on: bool) -> bool {
        let flag = if on { "1" } else { "0" };
        if pipewire() {
            return ok("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", flag]);
        }
        ok("pactl", &["set-sink-mute", "@DEFAULT_SINK@", flag])
    }

    /// The microphones: every source that is not a speaker's monitor.
    fn sources() -> Vec<String> {
        if pipewire() {
            return wp_nodes(&wp_status(), "Sources").into_iter().map(|(id, _, _)| id.to_string()).collect();
        }
        run("pactl", &["list", "short", "sources"])
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
            .filter(|name| !name.ends_with(".monitor"))
            .collect()
    }

    pub fn mics() -> (Option<bool>, u32) {
        let count = sources().len() as u32;
        let muted = if pipewire() {
            run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).map(|o| o.contains("[MUTED]"))
        } else {
            run("pactl", &["get-source-mute", "@DEFAULT_SOURCE@"]).map(|o| o.contains("yes"))
        };
        (muted, count)
    }

    pub fn set_mics_muted(on: bool) -> bool {
        let flag = if on { "1" } else { "0" };
        let mut done = false;
        for name in sources() {
            done |= if pipewire() { ok("wpctl", &["set-mute", &name, flag]) } else { ok("pactl", &["set-source-mute", &name, flag]) };
        }
        done
    }

    /// The speakers, from PipeWire. Plain PulseAudio has only the system's settings for that.
    pub fn outputs() -> Vec<super::Device> {
        if !pipewire() {
            return Vec::new();
        }
        let sinks = wp_nodes(&wp_status(), "Sinks");
        // One pair of speakers: nothing to choose.
        if sinks.len() < 2 {
            return Vec::new();
        }
        sinks.into_iter().map(|(id, name, default)| super::Device { id: id.to_string(), name, default }).collect()
    }

    pub fn set_output(id: &str) -> bool {
        ok("wpctl", &["set-default", id])
    }

    /// `nmcli -t` fields: ':' between them, "\:" inside one.
    pub fn nm_fields(line: &str) -> Vec<String> {
        let mut out = vec![String::new()];
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.last_mut().unwrap().extend(chars.next()),
                ':' => out.push(String::new()),
                _ => out.last_mut().unwrap().push(c),
            }
        }
        out
    }

    /// What the computer is connected through, and the connections set up.
    pub fn network() -> (Option<super::Network>, Vec<super::Connection>) {
        let Some(text) = run("nmcli", &["-t", "-f", "NAME,UUID,TYPE,ACTIVE", "connection", "show"]) else { return (None, Vec::new()) };
        let connections: Vec<super::Connection> = text
            .lines()
            .filter_map(|line| {
                let f = nm_fields(line);
                let kind = match f.get(2)?.as_str() {
                    "802-3-ethernet" => "wired",
                    "802-11-wireless" => "wifi",
                    "vpn" | "wireguard" => "vpn",
                    _ => return None,
                };
                Some(super::Connection { name: f[0].clone(), id: f[1].clone(), kind: kind.into(), active: f.get(3)?.as_str() == "yes" })
            })
            .collect();
        let up = connections.iter().find(|c| c.active && c.kind != "vpn");
        let network = super::Network {
            kind: up.map(|c| c.kind.clone()).unwrap_or_default(),
            name: up.map(|c| c.name.clone()).unwrap_or_default(),
        };
        (Some(network), connections)
    }

    pub fn set_connection(id: &str, up: bool) -> bool {
        ok("nmcli", &["--wait", "20", "connection", if up { "up" } else { "down" }, id])
    }

    /// power-profiles-daemon: the mode, and the ones this computer has ("* performance:" lines).
    pub fn power_mode() -> Option<(String, Vec<String>)> {
        let mode = run("powerprofilesctl", &["get"])?.trim().to_string();
        let all = run("powerprofilesctl", &["list"])?
            .lines()
            .filter_map(|l| l.trim().trim_start_matches('*').trim().strip_suffix(':').map(str::to_string))
            .collect::<Vec<_>>();
        // power-saver, balanced, performance: from the lightest.
        let order = ["power-saver", "balanced", "performance"];
        let mut all: Vec<String> = all.into_iter().filter(|m| order.contains(&m.as_str())).collect();
        all.sort_by_key(|m| order.iter().position(|o| o == m));
        (!all.is_empty()).then_some((mode, all))
    }

    pub fn set_power_mode(mode: &str) -> bool {
        ok("powerprofilesctl", &["set", mode])
    }

    const INTERFACE: &str = "org.gnome.desktop.interface";

    pub fn dark() -> Option<bool> {
        run("gsettings", &["get", INTERFACE, "color-scheme"]).map(|o| o.contains("prefer-dark"))
    }

    /// GNOME's dark style; on Ubuntu the Yaru theme of older GTK apps follows, as Settings does.
    pub fn set_dark(on: bool) -> bool {
        let done = ok("gsettings", &["set", INTERFACE, "color-scheme", if on { "prefer-dark" } else { "default" }]);
        if let Some(theme) = run("gsettings", &["get", INTERFACE, "gtk-theme"]) {
            let theme = theme.trim().trim_matches('\'');
            if theme.starts_with("Yaru") {
                let light = theme.strip_suffix("-dark").unwrap_or(theme);
                let next = if on { format!("{light}-dark") } else { light.to_string() };
                if next != theme {
                    ok("gsettings", &["set", INTERFACE, "gtk-theme", &next]);
                }
            }
        }
        done
    }

    const BANNERS: [&str; 2] = ["org.gnome.desktop.notifications", "show-banners"];

    /// GNOME's banners off, unless Ciel turned them off itself to show the
    /// notifications in the island (notify.rs): that is not do not disturb.
    pub fn dnd() -> Option<bool> {
        let off = run("gsettings", &["get", BANNERS[0], BANNERS[1]])?.trim() == "false";
        Some(off && !crate::platform::config_dir().join(crate::notify::BANNERS_MARKER).exists())
    }

    pub fn set_dnd(on: bool) {
        // Ciel showing the notifications keeps GNOME's banners off either way.
        if crate::platform::config_dir().join(crate::notify::BANNERS_MARKER).exists() {
            return;
        }
        ok("gsettings", &["set", BANNERS[0], BANNERS[1], if on { "false" } else { "true" }]);
    }

    /// The computer's own battery, not a mouse's.
    pub fn battery() -> Option<super::Battery> {
        for entry in std::fs::read_dir("/sys/class/power_supply").ok()?.flatten() {
            let read = |f: &str| std::fs::read_to_string(entry.path().join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
            if read("type") != "Battery" || read("scope") == "Device" {
                continue;
            }
            let Ok(percent) = read("capacity").parse::<f64>() else { continue };
            let status = read("status");
            return Some(super::Battery { percent, charging: status == "Charging" || status == "Full" });
        }
        None
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
        // The radio says "enabled" on a computer without Wi-Fi: a device of that kind first.
        let has_wifi = run("nmcli", &["-t", "-f", "TYPE", "device"]).is_some_and(|o| o.lines().any(|l| l == "wifi"));
        let wifi = if has_wifi { run("nmcli", &["radio", "wifi"]).map(|o| o.trim() == "enabled") } else { None };
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

        #[test]
        fn wpctl_volumes_and_nodes() {
            assert_eq!(super::wp_volume("Volume: 0.84\n"), Some((0.84, false)));
            assert_eq!(super::wp_volume("Volume: 1.30 [MUTED]\n"), Some((1.0, true)));
            assert_eq!(super::wp_volume("nothing"), None);
            let status = "PipeWire 'pipewire-0' [1.0.5, jean@pc, cookie:1]
 └─ Clients:
        32. pipewire                            [1.0.5, jean@pc, pid:7355]

Audio
 ├─ Devices:
 │      46. Rembrandt Radeon High Definition Audio Controller [alsa]
 │  
 ├─ Sinks:
 │  *   50. AERO ULTRA_2.4G Analog Stereo       [vol: 0.84]
 │      52. Radeon HDMI Digital Stereo (HDMI) [vol: 0.90 MUTED]
 │  
 ├─ Sink endpoints:
 │  
 ├─ Sources:
 │  *   51. AERO ULTRA_2.4G Mono                [vol: 1.00]
 │  
 └─ Source endpoints:

Video
 ├─ Sources:
 │  *   60. Integrated Camera (V4L2)
 │  
";
            assert_eq!(
                super::wp_nodes(status, "Sinks"),
                vec![(50, "AERO ULTRA_2.4G Analog Stereo".to_string(), true), (52, "Radeon HDMI Digital Stereo (HDMI)".to_string(), false)]
            );
            // The camera is a Video source, not a microphone.
            assert_eq!(super::wp_nodes(status, "Sources"), vec![(51, "AERO ULTRA_2.4G Mono".to_string(), true)]);
        }

        #[test]
        fn nmcli_fields_keep_escaped_colons() {
            assert_eq!(super::nm_fields(r"Office\:2:abc-1:802-11-wireless:yes"), vec!["Office:2", "abc-1", "802-11-wireless", "yes"]);
            assert_eq!(super::nm_fields("Wired:u:802-3-ethernet:no"), vec!["Wired", "u", "802-3-ethernet", "no"]);
        }
    }
}
