// The System pill, `integration_system`: this computer's CPU, memory, disk and
// temperature as rings in the island, the processes that weigh most, and a
// warning when one runs high (after SysDash, github.com/1Batuhan1/Health-Dashboard).
//
// Ciel looks only when it is asked to: every 2 s while the card is on screen
// (the island says when), every 30 s while the pill is on and its warnings are
// wanted (Settings → Integrations → System, off by default), and not at all
// otherwise or while paused. A warning is a notification naming what weighs
// most, at most once an hour per measure, and the pill's error state; going
// back under the limit clears it. Nothing is kept past the last two minutes,
// nothing leaves the computer.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::json;
use sysinfo::{Components, Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::{AppHandle, Manager};

use crate::integrations::{IntegrationEvent, IntegrationUpdate};
use crate::notify::{self, Notification, Target};

pub const PILL_ID: &str = "integration_system";

const WATCHED: Duration = Duration::from_secs(2);
const WARNING_EVERY: Duration = Duration::from_secs(30);
/// One warning per measure at most this often.
const QUIET: Duration = Duration::from_secs(3600);
/// Samples kept for the sparklines: two minutes while watched.
const HISTORY: usize = 60;
const TOP: usize = 5;

/// The island shows the card.
static WATCHING: AtomicBool = AtomicBool::new(false);
static WAKE: LazyLock<(Mutex<bool>, std::sync::Condvar)> = LazyLock::new(|| (Mutex::new(false), std::sync::Condvar::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Measure {
    Cpu,
    Ram,
    Disk,
    Temp,
}

impl Measure {
    const ALL: [Measure; 4] = [Measure::Cpu, Measure::Ram, Measure::Disk, Measure::Temp];

    /// SysDash's limits: orange from the first, red from the second.
    fn limits(self) -> (f32, f32) {
        match self {
            Measure::Cpu => (70.0, 90.0),
            Measure::Ram => (80.0, 92.0),
            Measure::Disk => (80.0, 90.0),
            // Above SysDash's 70 / 85: today's processors run to 95 °C by design.
            Measure::Temp => (80.0, 95.0),
        }
    }

    pub fn level(self, value: f32) -> Level {
        let (warn, crit) = self.limits();
        if value >= crit {
            Level::Critical
        } else if value >= warn {
            Level::Warning
        } else {
            Level::Ok
        }
    }

    fn key(self) -> &'static str {
        match self {
            Measure::Cpu => "cpu",
            Measure::Ram => "ram",
            Measure::Disk => "disk",
            Measure::Temp => "temp",
        }
    }
}

/// One look at the computer.
#[derive(Debug, Clone, Default)]
struct Sample {
    cpu: f32,
    ram: f32,
    ram_used: u64,
    ram_total: u64,
    disk: f32,
    disk_used: u64,
    disk_total: u64,
    disk_mount: String,
    temp: Option<f32>,
}

impl Sample {
    fn value(&self, m: Measure) -> Option<f32> {
        match m {
            Measure::Cpu => Some(self.cpu),
            Measure::Ram => Some(self.ram),
            Measure::Disk => (self.disk_total > 0).then_some(self.disk),
            Measure::Temp => self.temp,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Top {
    pid: u32,
    name: String,
    cpu: f32,
    memory: u64,
    /// Ours: End may close it.
    own: bool,
}

/// Settings saved or Ciel paused: the watch looks again at what it should do.
pub fn sync() {
    wake();
}

fn wake() {
    let (lock, cv) = &*WAKE;
    *lock.lock().unwrap() = true;
    cv.notify_all();
}

/// The island shows the card, or no longer does.
#[tauri::command]
pub fn system_watch(watching: bool) {
    if WATCHING.swap(watching, Ordering::Relaxed) != watching {
        wake();
    }
}

/// End a process of the user's, from the card (after its second click).
#[tauri::command]
pub fn system_end(pid: u32) -> bool {
    let mut sys = System::new();
    let me = sysinfo::get_current_pid().ok();
    let pids: Vec<Pid> = [Some(Pid::from_u32(pid)), me].into_iter().flatten().collect();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, ProcessRefreshKind::nothing().with_user(UpdateKind::Always));
    let ours = me.and_then(|p| sys.process(p)).and_then(|p| p.user_id()).cloned();
    let Some(process) = sys.process(Pid::from_u32(pid)) else { return false };
    if me == Some(process.pid()) || ours.is_none() || process.user_id() != ours.as_ref() {
        return false;
    }
    let done = process.kill_with(sysinfo::Signal::Term).unwrap_or_else(|| process.kill());
    crate::log::line(format!("system: ended a process ({})", if done { "done" } else { "refused" }));
    done
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("ciel-system".into()).spawn(move || run(&app));
    if let Err(err) = spawned {
        crate::log::line(format!("system: no thread: {err}"));
    }
}

struct Watch {
    sys: System,
    disks: Disks,
    components: Components,
    history: VecDeque<Sample>,
    levels: [Level; 4],
    warned: [Option<Instant>; 4],
}

fn run(app: &AppHandle) {
    let mut w = Watch {
        sys: System::new(),
        disks: Disks::new_with_refreshed_list(),
        components: Components::new_with_refreshed_list(),
        history: VecDeque::new(),
        levels: [Level::Ok; 4],
        warned: [None; 4],
    };
    loop {
        let (declared, warnings) = app
            .try_state::<crate::Shared>()
            .map(|s| {
                let s = s.settings.lock().unwrap();
                (s.active_integrations.iter().any(|id| id == PILL_ID), s.system_warnings)
            })
            .unwrap_or((false, false));
        let paused = crate::integrations::PAUSED.load(Ordering::Relaxed);
        let watched = WATCHING.load(Ordering::Relaxed);
        let every = match (declared && !paused, watched, warnings) {
            (false, _, _) => None,
            (true, true, _) => Some(WATCHED),
            (true, false, true) => Some(WARNING_EVERY),
            (true, false, false) => None,
        };
        let Some(every) = every else {
            // Nothing to look at: wait to be told otherwise.
            w.history.clear();
            sleep_until_woken(None);
            continue;
        };
        let sample = look(&mut w);
        let top = if watched { top(&mut w.sys) } else { Vec::new() };
        w.history.push_back(sample.clone());
        while w.history.len() > HISTORY {
            w.history.pop_front();
        }
        let event = check(app, &mut w, &sample, warnings);
        report(app, &w, &sample, &top, event);
        sleep_until_woken(Some(every));
    }
}

fn sleep_until_woken(limit: Option<Duration>) {
    let (lock, cv) = &*WAKE;
    let mut woken = lock.lock().unwrap();
    if !*woken {
        woken = match limit {
            Some(d) => cv.wait_timeout(woken, d).unwrap().0,
            None => cv.wait(woken).unwrap(),
        };
    }
    *woken = false;
}

fn look(w: &mut Watch) -> Sample {
    // CPU use is the difference between two readings: the first one after a pause reads twice.
    if w.history.is_empty() {
        w.sys.refresh_cpu_usage();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    }
    w.sys.refresh_cpu_usage();
    w.sys.refresh_memory();
    w.disks.refresh(true);
    w.components.refresh(true);
    let ram_total = w.sys.total_memory();
    let ram_used = w.sys.used_memory();
    let (disk_mount, disk_used, disk_total) = home_disk(&w.disks);
    Sample {
        cpu: w.sys.global_cpu_usage(),
        ram: percent(ram_used, ram_total),
        ram_used,
        ram_total,
        disk: percent(disk_used, disk_total),
        disk_used,
        disk_total,
        disk_mount,
        temp: hottest(&w.components.list().iter().filter_map(|c| Some((c.label().to_string(), c.temperature()?))).collect::<Vec<_>>()),
    }
}

fn percent(used: u64, total: u64) -> f32 {
    if total == 0 { 0.0 } else { (used as f64 / total as f64 * 100.0) as f32 }
}

/// The disk the user's files are on: the longest mount point their home is under.
fn home_disk(disks: &Disks) -> (String, u64, u64) {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    let home = Path::new(&home);
    disks
        .list()
        .iter()
        .filter(|d| home.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| {
            let total = d.total_space();
            (d.mount_point().display().to_string(), total.saturating_sub(d.available_space()), total)
        })
        .unwrap_or_default()
}

/// The processor's own temperature: its dies (AMD's Tccd and Tdie, Intel's
/// package), else what a sensor says is the CPU, else the hottest reading.
/// AMD's Tctl is left for last: a control value, set above the die's.
pub fn hottest(sensors: &[(String, f32)]) -> Option<f32> {
    let readings = |keys: &[&str]| {
        sensors
            .iter()
            .filter(|(label, t)| t.is_finite() && *t > 0.0 && *t < 150.0 && keys.iter().any(|k| label.to_lowercase().contains(k)))
            .map(|(_, t)| *t)
            .reduce(f32::max)
    };
    readings(&["tccd", "tdie", "package"])
        .or_else(|| readings(&["cpu", "core", "k10temp", "coretemp"]))
        .or_else(|| readings(&["tctl"]))
        .or_else(|| readings(&[""]))
}

/// The processes that weigh most, by CPU then memory.
fn top(sys: &mut System) -> Vec<Top> {
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory().with_user(UpdateKind::OnlyIfNotSet),
    );
    let cores = sys.cpus().len().max(1) as f32;
    let me = sysinfo::get_current_pid().ok().and_then(|p| sys.process(p)).and_then(|p| p.user_id()).cloned();
    let mut list: Vec<Top> = sys
        .processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| Top {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().into_owned(),
            // As a share of the whole computer, like the ring.
            cpu: p.cpu_usage() / cores,
            memory: p.memory(),
            own: me.is_some() && p.user_id() == me.as_ref(),
        })
        .collect();
    list.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then(b.memory.cmp(&a.memory)));
    list.truncate(TOP);
    list
}

/// A measure that went up a level warns, once an hour at most; one back down clears.
fn check(app: &AppHandle, w: &mut Watch, s: &Sample, warnings: bool) -> Option<IntegrationEvent> {
    let mut event = None;
    for (i, m) in Measure::ALL.into_iter().enumerate() {
        let Some(value) = s.value(m) else { continue };
        let level = m.level(value);
        let before = std::mem::replace(&mut w.levels[i], level);
        if !warnings || level <= before || level == Level::Ok {
            continue;
        }
        if w.warned[i].is_some_and(|at| at.elapsed() < QUIET) {
            continue;
        }
        w.warned[i] = Some(Instant::now());
        let (title, detail) = warning(m, s, &heaviest(&mut w.sys, m));
        crate::log::line(format!("system: {} at {:.0}", m.key(), value));
        notify::show(
            app,
            Notification {
                id: notify::next_id("system"),
                source: "system".into(),
                app: crate::i18n::t("This computer"),
                title: title.clone(),
                body: detail.clone(),
                icon: None,
                at: now_ms(),
                tag: Some(format!("system-{}", m.key())),
                buttons: Vec::new(),
                until: None,
                time: None,
            },
            Target::None,
        );
        event = Some(IntegrationEvent { success: false, label: title, detail: Some(detail) });
    }
    event
}

/// What weighs most for that measure, for the warning ("Chrome uses 4.1 GB").
fn heaviest(sys: &mut System, m: Measure) -> Option<Top> {
    if m == Measure::Disk {
        return None;
    }
    // CPU use of a process is also a difference: two readings.
    top(sys);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    let mut list = top(sys);
    if m == Measure::Ram {
        list.sort_by(|a, b| b.memory.cmp(&a.memory));
    }
    list.into_iter().next()
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1e9)
}

fn warning(m: Measure, s: &Sample, heavy: &Option<Top>) -> (String, String) {
    use crate::i18n::tf;
    match m {
        Measure::Cpu => (
            tf("CPU at {pct}%", &[("pct", &format!("{:.0}", s.cpu))]),
            heavy.as_ref().map(|t| tf("{name} uses the most", &[("name", &t.name)])).unwrap_or_default(),
        ),
        Measure::Ram => (
            tf("Memory at {pct}%", &[("pct", &format!("{:.0}", s.ram))]),
            heavy.as_ref().map(|t| tf("{name} uses {size}", &[("name", &t.name), ("size", &gb(t.memory))])).unwrap_or_default(),
        ),
        Measure::Disk => (
            tf("Disk at {pct}%", &[("pct", &format!("{:.0}", s.disk))]),
            tf("{free} free on {mount}", &[("free", &gb(s.disk_total.saturating_sub(s.disk_used))), ("mount", &s.disk_mount)]),
        ),
        Measure::Temp => (
            tf("Processor at {deg} °C", &[("deg", &format!("{:.0}", s.temp.unwrap_or_default()))]),
            heavy.as_ref().map(|t| tf("{name} uses the most", &[("name", &t.name)])).unwrap_or_default(),
        ),
    }
}

fn report(app: &AppHandle, w: &Watch, s: &Sample, top: &[Top], event: Option<IntegrationEvent>) {
    let series = |m: Measure| -> Vec<f32> { w.history.iter().filter_map(|h| h.value(m)).collect() };
    let level = |m: Measure| s.value(m).map(|v| m.level(v));
    let data = json!({
        "cpu": s.cpu, "ram": s.ram, "disk": s.disk, "temp": s.temp,
        "ramUsed": s.ram_used, "ramTotal": s.ram_total,
        "diskUsed": s.disk_used, "diskTotal": s.disk_total, "diskMount": s.disk_mount,
        "levels": { "cpu": level(Measure::Cpu), "ram": level(Measure::Ram), "disk": level(Measure::Disk), "temp": level(Measure::Temp) },
        "history": { "cpu": series(Measure::Cpu), "ram": series(Measure::Ram), "disk": series(Measure::Disk), "temp": series(Measure::Temp) },
        "top": top,
    });
    crate::integrations::emit(app, IntegrationUpdate { id: PILL_ID, data, error: None, event });
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysdashs_limits_make_the_levels() {
        assert_eq!(Measure::Cpu.level(69.9), Level::Ok);
        assert_eq!(Measure::Cpu.level(70.0), Level::Warning);
        assert_eq!(Measure::Cpu.level(90.0), Level::Critical);
        assert_eq!(Measure::Ram.level(85.0), Level::Warning);
        assert_eq!(Measure::Ram.level(92.0), Level::Critical);
        assert_eq!(Measure::Disk.level(79.0), Level::Ok);
        assert_eq!(Measure::Temp.level(86.0), Level::Warning);
        assert_eq!(Measure::Temp.level(95.0), Level::Critical);
    }

    #[test]
    fn the_processor_is_read_from_its_dies_before_amds_control_value() {
        let s = |list: &[(&str, f32)]| list.iter().map(|(l, t)| (l.to_string(), *t)).collect::<Vec<_>>();
        // A Ryzen: Tctl runs above the dies.
        assert_eq!(hottest(&s(&[("edge", 80.0), ("Tccd1", 88.25), ("Tctl", 96.25), ("Composite", 52.0)])), Some(88.25));
        assert_eq!(hottest(&s(&[("Package id 0", 71.0), ("Core 0", 65.0)])), Some(71.0));
        assert_eq!(hottest(&s(&[("k10temp Tctl", 60.0)])), Some(60.0));
        assert_eq!(hottest(&s(&[("nvme Composite", 40.0), ("acpitz", 45.0)])), Some(45.0));
        assert_eq!(hottest(&[]), None);
    }

    #[test]
    fn a_look_at_this_computer_reads_something() {
        let mut w = Watch {
            sys: System::new(),
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
            history: VecDeque::new(),
            levels: [Level::Ok; 4],
            warned: [None; 4],
        };
        let s = look(&mut w);
        assert!(s.ram_total > 0 && s.ram > 0.0 && s.ram <= 100.0);
        assert!((0.0..=100.0).contains(&s.cpu));
        assert!(!top(&mut w.sys).is_empty());
    }
}
