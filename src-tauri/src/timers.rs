// Timers, reminders and Focus rounds (Tools → Timers), and « OK Ciel, remind
// me… ». Rust keeps them so they ring with the island hidden, or the window
// asleep: one thread waits for the next one due and nothing else (a condition
// variable, woken when the list changes, and at most every 30 s while
// something is due so a computer that slept catches up).
//
// They are kept in timers.json beside settings.json. A reminder missed while
// Ciel was not running rings when it starts; a timer that ended meanwhile
// shows as done; a Focus round does not survive a restart.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, LazyLock, Mutex, OnceLock};
use std::time::Duration;

use chrono::{Datelike, Local, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::notify::{self, Button, Notification, Target};

const EVENT: &str = "timers";
/// The longest the thread sleeps while something is due: a computer that slept wakes it on time.
const NAP: Duration = Duration::from_secs(30);
/// How many may be set at once.
const MAX: usize = 40;
/// A timer that ended while Ciel was off this long ago is forgotten.
const STALE_MS: f64 = 6.0 * 3_600_000.0;
const MINUTE: f64 = 60_000.0;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Timer,
    Reminder,
    Focus,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    pub id: String,
    pub kind: Kind,
    pub label: String,
    /// Unix ms: when it rings (or rang). 0 while paused.
    pub due: f64,
    /// A timer's length, the current Focus phase's, in ms (0 for a reminder).
    pub duration: f64,
    /// Ms left, while a timer is paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    /// A reminder's repeat: "", "daily", "weekdays" or "weekly".
    #[serde(default)]
    pub repeat: String,
    /// It rang and nobody has answered yet.
    #[serde(default)]
    pub ringing: bool,
    /// Focus: "focus" or "break", and how many focus rounds are done.
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub round: u32,
    #[serde(default)]
    pub focus_ms: f64,
    #[serde(default)]
    pub break_ms: f64,
}

struct Book {
    timers: Vec<Timer>,
    loaded: bool,
}

static BOOK: LazyLock<Mutex<Book>> = LazyLock::new(|| Mutex::new(Book { timers: Vec::new(), loaded: false }));
static WAKE: Condvar = Condvar::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(1);

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

fn path() -> PathBuf {
    crate::settings::config_dir().join("timers.json")
}

fn new_id() -> String {
    format!("t{}-{}", SEQ.fetch_add(1, Ordering::Relaxed), now_ms() as u64 % 1_000_000)
}

// ── When a reminder comes back ────────────────────────────────────────────────

/// The next time a repeating reminder rings after `after` (Unix ms), at the
/// time of day `of` had (and, weekly, its weekday). None for no repeat.
pub fn next_due(repeat: &str, of: f64, after: f64) -> Option<f64> {
    let base = Local.timestamp_millis_opt(of as i64).single()?;
    let time = NaiveTime::from_hms_opt(base.hour(), base.minute(), 0)?;
    let start = Local.timestamp_millis_opt(after as i64).single()?.date_naive();
    for day in 0..15 {
        let date = start + chrono::Days::new(day);
        let ok = match repeat {
            "daily" => true,
            "weekdays" => !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
            "weekly" => date.weekday() == base.weekday(),
            _ => return None,
        };
        if !ok {
            continue;
        }
        // A time skipped by a clock change: the first one after it.
        let at = Local.from_local_datetime(&date.and_time(time)).earliest()?;
        let ms = at.timestamp_millis() as f64;
        if ms > after {
            return Some(ms);
        }
    }
    None
}

// ── Keeping them ──────────────────────────────────────────────────────────────

fn load(book: &mut Book) {
    if book.loaded {
        return;
    }
    book.loaded = true;
    let Ok(bytes) = std::fs::read(path()) else { return };
    let Ok(timers) = serde_json::from_slice::<Vec<Timer>>(&bytes) else { return };
    let now = now_ms();
    for mut t in timers {
        match t.kind {
            // A round of Focus is now or never.
            Kind::Focus => continue,
            Kind::Timer if t.left.is_none() && t.due < now - STALE_MS => continue,
            _ => {}
        }
        if t.left.is_none() && t.due <= now {
            t.ringing = true;
        }
        book.timers.push(t);
    }
}

fn save(book: &Book) {
    let kept: Vec<&Timer> = book.timers.iter().filter(|t| t.kind != Kind::Focus).collect();
    let dir = crate::settings::config_dir();
    if crate::platform::ensure_private_dir(&dir).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_vec_pretty(&kept) {
        let tmp = path().with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
}

fn sorted(book: &Book) -> Vec<Timer> {
    let mut list = book.timers.clone();
    // Ringing first, then the soonest; paused ones after.
    list.sort_by(|a, b| {
        b.ringing.cmp(&a.ringing).then_with(|| {
            let key = |t: &Timer| if t.left.is_some() { f64::MAX } else { t.due };
            key(a).total_cmp(&key(b))
        })
    });
    list
}

fn changed(book: &Book) {
    save(book);
    WAKE.notify_all();
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, sorted(book));
    }
}

// ── The thread ────────────────────────────────────────────────────────────────

pub fn start(app: &AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    {
        let mut book = BOOK.lock().unwrap();
        load(&mut book);
    }
    let _ = std::thread::Builder::new().name("ciel-timers".into()).spawn(run);
}

fn run() {
    let mut book = BOOK.lock().unwrap();
    // What rang while Ciel was away rings now.
    let missed: Vec<Timer> = book.timers.iter().filter(|t| t.ringing).cloned().collect();
    if !missed.is_empty() {
        drop(book);
        for t in &missed {
            announce(t, true);
        }
        book = BOOK.lock().unwrap();
        if let Some(app) = APP.get() {
            let _ = app.emit(EVENT, sorted(&book));
        }
    }
    loop {
        let now = now_ms();
        let fired = fire_due(&mut book, now);
        if !fired.is_empty() {
            changed(&book);
            drop(book);
            for t in &fired {
                announce(t, false);
            }
            book = BOOK.lock().unwrap();
            continue;
        }
        let next = book.timers.iter().filter(|t| !t.ringing && t.left.is_none()).map(|t| t.due).fold(f64::MAX, f64::min);
        book = if next == f64::MAX {
            WAKE.wait(book).unwrap()
        } else {
            let wait = Duration::from_millis(((next - now).max(0.0) as u64).min(NAP.as_millis() as u64));
            WAKE.wait_timeout(book, wait).unwrap().0
        };
    }
}

/// What is due rings, or moves on (a Focus round). Returns what to announce.
fn fire_due(book: &mut Book, now: f64) -> Vec<Timer> {
    let mut fired = Vec::new();
    for t in book.timers.iter_mut() {
        if t.ringing || t.left.is_some() || t.due > now {
            continue;
        }
        match t.kind {
            Kind::Focus => {
                if t.phase == "focus" {
                    t.round += 1;
                    t.phase = "break".into();
                    // A longer break after every fourth round.
                    t.duration = if t.round % 4 == 0 { t.break_ms * 3.0 } else { t.break_ms };
                } else {
                    t.phase = "focus".into();
                    t.duration = t.focus_ms;
                }
                t.due = now + t.duration;
            }
            _ => t.ringing = true,
        }
        fired.push(t.clone());
    }
    fired
}

/// The island's card for what rang (or, for Focus, what starts).
fn announce(timer: &Timer, missed: bool) {
    use crate::i18n::{t, tf};
    let Some(app) = APP.get() else { return };
    let (title, body, buttons) = match timer.kind {
        Kind::Timer => (
            if timer.label.is_empty() { t("Time's up") } else { timer.label.clone() },
            if missed { t("This timer ended while Ciel was closed.") } else { t("Your timer is done.") },
            vec![Button { id: "again".into(), label: t("Again") }, Button { id: "done".into(), label: t("Stop") }],
        ),
        Kind::Reminder => (
            if timer.label.is_empty() { t("Reminder") } else { timer.label.clone() },
            if missed { t("Missed while Ciel was closed.") } else { t("Reminder") },
            vec![Button { id: "snooze".into(), label: t("Snooze 10 min") }, Button { id: "done".into(), label: t("Done") }],
        ),
        Kind::Focus => {
            let minutes = ((timer.duration / MINUTE).round() as u64).to_string();
            if timer.phase == "break" {
                (
                    t("Time for a break"),
                    tf("{minutes} min to stretch, drink, look far away.", &[("minutes", &minutes)]),
                    vec![Button { id: "done".into(), label: t("Stop Focus") }],
                )
            } else {
                (
                    t("Back to focus"),
                    tf("Round {round}: {minutes} min.", &[("round", &(timer.round + 1).to_string()), ("minutes", &minutes)]),
                    vec![Button { id: "done".into(), label: t("Stop Focus") }],
                )
            }
        }
    };
    notify::show(
        app,
        Notification {
            id: notify::next_id("timers"),
            source: "timers".into(),
            app: t("Timers"),
            title,
            body,
            icon: None,
            at: now_ms(),
            tag: Some(format!("timer-{}", timer.id)),
            buttons,
            // A ringing timer stays up until answered; a Focus change just shows.
            until: if timer.kind == Kind::Focus { None } else { Some(now_ms() + 10.0 * MINUTE) },
            time: None,
        },
        Target::Timer(timer.id.clone()),
    );
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "timer-rang", timer);
}

// ── What can be done to one ───────────────────────────────────────────────────

/// "pause", "resume", "cancel", "done", "snooze", "again", "add" (`value` ms).
pub fn act(id: &str, action: &str, value: Option<f64>) -> bool {
    let mut book = BOOK.lock().unwrap();
    let now = now_ms();
    let Some(i) = book.timers.iter().position(|t| t.id == id) else { return false };
    let t = &mut book.timers[i];
    let mut remove = false;
    match action {
        "pause" if t.left.is_none() && !t.ringing && t.kind != Kind::Reminder => {
            t.left = Some((t.due - now).max(0.0));
            t.due = 0.0;
        }
        "resume" => {
            if let Some(left) = t.left.take() {
                t.due = now + left;
            }
        }
        "add" => {
            let ms = value.unwrap_or(MINUTE).clamp(-24.0 * 60.0 * MINUTE, 24.0 * 60.0 * MINUTE);
            if let Some(left) = t.left.as_mut() {
                *left = (*left + ms).max(1000.0);
            } else {
                t.due = (t.due.max(now) + ms).max(now + 1000.0);
            }
            t.duration = (t.duration + ms).max(1000.0);
            t.ringing = false;
        }
        "again" if t.kind == Kind::Timer => {
            t.ringing = false;
            t.left = None;
            t.due = now + t.duration;
        }
        "snooze" => {
            t.ringing = false;
            t.left = None;
            t.due = now + value.unwrap_or(10.0 * MINUTE).max(MINUTE);
        }
        "done" | "dismiss" | "open" => {
            // A repeating reminder comes back; anything else goes.
            match next_due(&t.repeat, t.due, now) {
                Some(next) if t.kind == Kind::Reminder => {
                    t.ringing = false;
                    t.due = next;
                }
                // Open on a card that no longer rings (moved on since) leaves it be.
                _ => remove = action != "open" || t.ringing,
            }
        }
        "cancel" => remove = true,
        _ => return false,
    }
    if remove {
        book.timers.remove(i);
    }
    changed(&book);
    true
}

fn add(t: Timer) -> Option<Timer> {
    let mut book = BOOK.lock().unwrap();
    load(&mut book);
    if book.timers.len() >= MAX {
        return None;
    }
    // One Focus at a time.
    if t.kind == Kind::Focus {
        book.timers.retain(|x| x.kind != Kind::Focus);
    }
    book.timers.push(t.clone());
    changed(&book);
    Some(t)
}

pub fn add_timer(label: &str, duration_ms: f64) -> Option<Timer> {
    let duration = duration_ms.clamp(1000.0, 24.0 * 60.0 * MINUTE);
    add(Timer {
        id: new_id(),
        kind: Kind::Timer,
        label: label.trim().chars().take(80).collect(),
        due: now_ms() + duration,
        duration,
        left: None,
        repeat: String::new(),
        ringing: false,
        phase: String::new(),
        round: 0,
        focus_ms: 0.0,
        break_ms: 0.0,
    })
}

pub fn add_reminder(label: &str, due: f64, repeat: &str) -> Option<Timer> {
    let repeat = match repeat {
        "daily" | "weekdays" | "weekly" => repeat,
        _ => "",
    };
    let now = now_ms();
    // A time already gone today is tomorrow's (or the repeat's next).
    let due = if due > now { due } else { next_due(if repeat.is_empty() { "daily" } else { repeat }, due, now)? };
    add(Timer {
        id: new_id(),
        kind: Kind::Reminder,
        label: label.trim().chars().take(120).collect(),
        due,
        duration: 0.0,
        left: None,
        repeat: repeat.to_string(),
        ringing: false,
        phase: String::new(),
        round: 0,
        focus_ms: 0.0,
        break_ms: 0.0,
    })
}

pub fn add_focus(focus_ms: f64, break_ms: f64) -> Option<Timer> {
    let focus_ms = focus_ms.clamp(5.0 * MINUTE, 120.0 * MINUTE);
    let break_ms = break_ms.clamp(MINUTE, 30.0 * MINUTE);
    add(Timer {
        id: new_id(),
        kind: Kind::Focus,
        label: String::new(),
        due: now_ms() + focus_ms,
        duration: focus_ms,
        left: None,
        repeat: String::new(),
        ringing: false,
        phase: "focus".into(),
        round: 0,
        focus_ms,
        break_ms,
    })
}

/// What is set, for « OK Ciel » and the island.
pub fn list() -> Vec<Timer> {
    let mut book = BOOK.lock().unwrap();
    load(&mut book);
    sorted(&book)
}

// ── « OK Ciel » ───────────────────────────────────────────────────────────────

/// "18:30" (local time, today or tomorrow) → Unix ms.
fn today_at(text: &str) -> Option<f64> {
    let (h, m) = text.trim().split_once(':')?;
    let time = NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)?;
    let date = Local::now().date_naive();
    Some(Local.from_local_datetime(&date.and_time(time)).earliest()?.timestamp_millis() as f64)
}

fn local_hm(ms: f64) -> String {
    Local.timestamp_millis_opt(ms as i64).single().map(|d| d.format("%a %H:%M").to_string()).unwrap_or_default()
}

/// The brain's `timer` tool: what goes back to the model, and whether something changed.
pub fn for_brain(input: &serde_json::Value) -> (String, bool) {
    let label = input["label"].as_str().unwrap_or("").trim().to_string();
    match input["action"].as_str().unwrap_or("") {
        "start" => {
            let minutes = input["minutes"].as_f64().unwrap_or(0.0) + input["seconds"].as_f64().unwrap_or(0.0) / 60.0;
            if minutes <= 0.0 {
                return ("Say how long: minutes (and seconds).".into(), false);
            }
            match add_timer(&label, minutes * MINUTE) {
                Some(_) => (format!("Timer set for {minutes} min{}.", if label.is_empty() { String::new() } else { format!(" ({label})") }), true),
                None => ("Too many timers are set already.".into(), false),
            }
        }
        "remind" => {
            let due = match (input["at"].as_str(), input["in_minutes"].as_f64()) {
                (Some(at), _) => today_at(at),
                (None, Some(m)) if m > 0.0 => Some(now_ms() + m * MINUTE),
                _ => None,
            };
            let Some(due) = due else { return ("Say when: a time (HH:MM) or in how many minutes.".into(), false) };
            match add_reminder(&label, due, input["repeat"].as_str().unwrap_or("")) {
                Some(t) => (format!("Reminder set for {}{}.", local_hm(t.due), if t.repeat.is_empty() { String::new() } else { format!(", {}", t.repeat) }), true),
                None => ("That reminder could not be set.".into(), false),
            }
        }
        "focus" => {
            let focus = input["minutes"].as_f64().unwrap_or(25.0);
            let pause = input["break_minutes"].as_f64().unwrap_or(5.0);
            match add_focus(focus * MINUTE, pause * MINUTE) {
                Some(t) => (format!("Focus started: {} min, then {} min of break.", (t.focus_ms / MINUTE).round(), (t.break_ms / MINUTE).round()), true),
                None => ("Focus could not start.".into(), false),
            }
        }
        "cancel" => {
            let wanted = label.to_lowercase();
            let ids: Vec<String> = list()
                .into_iter()
                .filter(|t| wanted.is_empty() || wanted == "all" || t.label.to_lowercase().contains(&wanted) || (wanted == "focus" && t.kind == Kind::Focus))
                .map(|t| t.id)
                .collect();
            let n = ids.iter().filter(|id| act(id, "cancel", None)).count();
            (if n == 0 { "Nothing like that is set.".into() } else { format!("{n} cancelled.") }, n > 0)
        }
        _ => {
            let now = now_ms();
            let items: Vec<serde_json::Value> = list()
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "kind": t.kind,
                        "label": t.label,
                        "ringing": t.ringing,
                        "paused": t.left.is_some(),
                        "minutesLeft": if t.kind == Kind::Reminder { serde_json::Value::Null } else { serde_json::json!(((t.left.unwrap_or(t.due - now)).max(0.0) / MINUTE * 10.0).round() / 10.0) },
                        "at": if t.kind == Kind::Reminder { serde_json::json!(local_hm(t.due)) } else { serde_json::Value::Null },
                        "repeat": t.repeat,
                        "phase": t.phase,
                    })
                })
                .collect();
            (serde_json::to_string(&items).unwrap_or_else(|_| "[]".into()), false)
        }
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn timers_list() -> Vec<Timer> {
    list()
}

#[tauri::command]
pub fn timer_start(label: String, duration_ms: f64) -> Option<Timer> {
    add_timer(&label, duration_ms)
}

#[tauri::command]
pub fn reminder_add(label: String, due: f64, repeat: String) -> Option<Timer> {
    add_reminder(&label, due, &repeat)
}

#[tauri::command]
pub fn focus_start(focus_ms: f64, break_ms: f64) -> Option<Timer> {
    add_focus(focus_ms, break_ms)
}

#[tauri::command]
pub fn timer_action(id: String, action: String, value: Option<f64>) -> bool {
    act(&id, &action, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> f64 {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest().unwrap().timestamp_millis() as f64
    }

    #[test]
    fn a_daily_reminder_comes_back_the_next_day_at_its_time() {
        // Saturday 7 November 2026, 9:00.
        let first = at(2026, 11, 7, 9, 0);
        assert_eq!(next_due("daily", first, first), Some(at(2026, 11, 8, 9, 0)));
        assert_eq!(next_due("daily", first, at(2026, 11, 7, 8, 0)), Some(first));
        assert_eq!(next_due("", first, first), None);
    }

    #[test]
    fn weekdays_skip_the_weekend_and_weekly_keeps_its_day() {
        let saturday = at(2026, 11, 7, 9, 0);
        assert_eq!(next_due("weekdays", saturday, saturday), Some(at(2026, 11, 9, 9, 0)));
        assert_eq!(next_due("weekly", saturday, saturday), Some(at(2026, 11, 14, 9, 0)));
        let friday = at(2026, 11, 6, 18, 30);
        assert_eq!(next_due("weekdays", friday, friday), Some(at(2026, 11, 9, 18, 30)));
    }

    #[test]
    fn focus_moves_from_round_to_break_with_a_long_break_every_fourth() {
        let mut book = Book { timers: Vec::new(), loaded: true };
        let mut t = Timer {
            id: "f".into(),
            kind: Kind::Focus,
            label: String::new(),
            due: 0.0,
            duration: 25.0 * MINUTE,
            left: None,
            repeat: String::new(),
            ringing: false,
            phase: "focus".into(),
            round: 3,
            focus_ms: 25.0 * MINUTE,
            break_ms: 5.0 * MINUTE,
        };
        book.timers.push(t.clone());
        let fired = fire_due(&mut book, 1000.0);
        assert_eq!(fired.len(), 1);
        t = book.timers[0].clone();
        assert_eq!((t.phase.as_str(), t.round, t.duration), ("break", 4, 15.0 * MINUTE));
        assert!(!t.ringing);
        book.timers[0].due = 0.0;
        fire_due(&mut book, 2000.0);
        assert_eq!(book.timers[0].phase, "focus");
        assert_eq!(book.timers[0].duration, 25.0 * MINUTE);
    }

    #[test]
    fn a_timer_rings_once_and_a_paused_one_never() {
        let mut book = Book { timers: Vec::new(), loaded: true };
        let base = Timer {
            id: "a".into(),
            kind: Kind::Timer,
            label: "tea".into(),
            due: 500.0,
            duration: 240_000.0,
            left: None,
            repeat: String::new(),
            ringing: false,
            phase: String::new(),
            round: 0,
            focus_ms: 0.0,
            break_ms: 0.0,
        };
        book.timers.push(base.clone());
        book.timers.push(Timer { id: "b".into(), left: Some(1000.0), due: 0.0, ..base.clone() });
        assert_eq!(fire_due(&mut book, 1000.0).len(), 1);
        assert!(book.timers[0].ringing);
        assert!(!book.timers[1].ringing);
        assert!(fire_due(&mut book, 2000.0).is_empty());
    }
}
