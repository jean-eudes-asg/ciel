// Printers, the pill `integration_printers`: the building's printers, checked
// every few minutes, their problems in the island as they come and go.
//
// Each printer the user added is asked over SNMP (snmp.rs: v1, v2c, or v3 with
// the keychain's passwords) or, when SNMP doesn't answer, over IPP (ipp.rs);
// reading.rs reads either into the same state, problems and levels. A problem
// is told once when it starts and once when it ends (the Alerts pill's Board);
// a printer counts as not responding after two checks in a row without an
// answer; history.json keeps 30 days of problems for the reports (report.rs).
//
// Finding printers (discover.rs) happens only on the user's click. Nothing is
// asked while the pill is off or Ciel is paused.

pub mod ber;
pub mod discover;
pub mod ipp;
pub mod reading;
pub mod report;
pub mod snmp;
pub mod usm;

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value as Json};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

use crate::alerts::{self, Alert, Board, Change, Severity};
use crate::i18n::{n_, t, tf, tn};
use crate::integrations::{IntegrationEvent, IntegrationUpdate};
use crate::notify::{self, Notification, Target};
use crate::settings::{PrinterEntry, PrintersPrefs, Settings};
use crate::{log, secrets};
use reading::{Problem, Reading};
use report::{History, Line};

pub const PILL_ID: &str = "integration_printers";
pub const COMMUNITY_KEY: &str = "printers-community";
pub const AUTH_KEY: &str = "printers-auth-password";
pub const PRIV_KEY: &str = "printers-priv-password";

/// Printers asked at the same time.
const AT_ONCE: usize = 8;
/// Checks in a row without an answer before a printer is "not responding".
const MISSES: u32 = 2;
/// At most this many notifications for one round of checks.
const MAX_NOTIFY: usize = 5;

/// What Ciel knows of one printer between checks.
#[derive(Default)]
struct Known {
    reading: Option<Reading>,
    hints: Hints,
    misses: u32,
    checked_at: f64,
    /// Its problems now, by their alert key.
    on: BTreeMap<String, Problem>,
    /// When they last changed.
    since: f64,
}

struct Runner {
    generation: u64,
    wake: Arc<Notify>,
    stop: Arc<Notify>,
    prefs: PrintersPrefs,
}

static RUNNER: LazyLock<Mutex<Option<Runner>>> = LazyLock::new(|| Mutex::new(None));
static GENERATION: AtomicU64 = AtomicU64::new(0);
static KNOWN: LazyLock<Mutex<HashMap<String, Known>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static BOARD: LazyLock<Mutex<Board>> = LazyLock::new(|| Mutex::new(Board::default()));
static HISTORY: LazyLock<Mutex<Option<History>>> = LazyLock::new(|| Mutex::new(None));
/// The day the last daily summary was shown.
static SUMMARY_DAY: Mutex<Option<(u32, u32, u32)>> = Mutex::new(None);

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

fn prefs_of(app: &AppHandle) -> Option<PrintersPrefs> {
    app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().printers.clone())
}

// ── Running ───────────────────────────────────────────────────────────────────

/// Checks when it should — the pill declared, Ciel not paused — and stops
/// otherwise. A change to the printers or how they're asked checks at once;
/// switching the pill off forgets what was wrong.
pub fn sync(app: &AppHandle, settings: &Settings) {
    let declared = settings.active_integrations.iter().any(|id| id == PILL_ID);
    let want = declared && !crate::integrations::PAUSED.load(Ordering::Relaxed);
    if !settings.printers.to_alerts || !declared {
        alerts::forget(app, "printer:");
    }
    let mut runner = RUNNER.lock().unwrap();
    if !want {
        if let Some(r) = runner.take() {
            r.stop.notify_one();
        }
        if !declared {
            KNOWN.lock().unwrap().clear();
            *BOARD.lock().unwrap() = Board::default();
        }
        return;
    }
    match runner.as_mut() {
        Some(r) if r.prefs != settings.printers => {
            r.prefs = settings.printers.clone();
            r.wake.notify_one();
        }
        Some(_) => {}
        None => {
            let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
            let (wake, stop) = (Arc::new(Notify::new()), Arc::new(Notify::new()));
            *runner = Some(Runner { generation, wake: wake.clone(), stop: stop.clone(), prefs: settings.printers.clone() });
            tauri::async_runtime::spawn(run(app.clone(), generation, wake, stop));
        }
    }
}

/// Settings' Check now, or new SNMP credentials: a round of checks right away.
pub fn check_now() {
    if let Some(r) = RUNNER.lock().unwrap().as_ref() {
        r.wake.notify_one();
    }
}

fn current(generation: u64) -> bool {
    RUNNER.lock().unwrap().as_ref().is_some_and(|r| r.generation == generation)
}

async fn run(app: AppHandle, generation: u64, wake: Arc<Notify>, stop: Arc<Notify>) {
    tokio::time::sleep(Duration::from_secs(2)).await;
    loop {
        if !current(generation) {
            return;
        }
        check_all(&app).await;
        summary_if_due(&app);
        let minutes = prefs_of(&app).map_or(5, |p| p.interval_minutes.clamp(1, 1440));
        let next = tokio::time::Instant::now() + Duration::from_secs(u64::from(minutes) * 60);
        // A minute at a time, for the daily summary.
        loop {
            let tick = (tokio::time::Instant::now() + Duration::from_secs(60)).min(next);
            tokio::select! {
                _ = tokio::time::sleep_until(tick) => {}
                _ = wake.notified() => break,
                _ = stop.notified() => return,
            }
            summary_if_due(&app);
            if tokio::time::Instant::now() >= next {
                break;
            }
        }
    }
}

// ── Checking ──────────────────────────────────────────────────────────────────

fn credentials(prefs: &PrintersPrefs) -> snmp::Credentials {
    let version = snmp::Version::parse(&prefs.snmp_version);
    let v3 = version == snmp::Version::V3;
    snmp::Credentials {
        version,
        community: secrets::get(COMMUNITY_KEY).unwrap_or_else(|| "public".into()),
        user: prefs.snmp_user.trim().to_string(),
        auth: usm::Auth::parse(&prefs.snmp_auth),
        auth_password: if v3 { secrets::get(AUTH_KEY).unwrap_or_default() } else { String::new() },
        privacy: prefs.snmp_privacy,
        priv_password: if v3 && prefs.snmp_privacy { secrets::get(PRIV_KEY).unwrap_or_default() } else { String::new() },
    }
}

/// What was learnt about how to reach a printer, kept between checks.
#[derive(Debug, Clone, Default)]
struct Hints {
    /// "snmp" or "ipp": what answered last.
    via: String,
    ipp_url: Option<String>,
    /// Whether its agent answers GETBULK, once known.
    bulk: Option<bool>,
    /// IPP didn't answer the last time it was asked only to fill levels in.
    no_ipp: bool,
}

enum Outcome {
    Read(Reading),
    /// SNMPv3 turned the credentials down, and IPP didn't answer either.
    Refused(String),
    Unreachable,
}

async fn read_snmp(host: &str, creds: &snmp::Credentials, bulk: bool) -> Result<(Reading, bool), snmp::Error> {
    let addr = tokio::net::lookup_host((host, 161))
        .await
        .map_err(|e| snmp::Error::Io(e.to_string()))?
        .next()
        .ok_or(snmp::Error::Timeout)?;
    let mut client = snmp::Client::connect(addr, creds.clone()).await?;
    client.set_bulk(bulk);
    let mut vars = reading::Vars::new();
    let scalars = reading::snmp_scalars();
    match client.get(&scalars).await {
        Ok(list) => vars.extend(list),
        // v1 refuses a whole GET for one name it doesn't have: one at a time, then.
        Err(snmp::Error::Agent(_)) => {
            for oid in &scalars {
                if let Ok(list) = client.get(std::slice::from_ref(oid)).await {
                    vars.extend(list);
                }
            }
        }
        Err(e) => return Err(e),
    }
    for table in reading::snmp_tables() {
        match client.walk(&table).await {
            Ok(list) => vars.extend(list),
            Err(snmp::Error::Timeout) => break,
            Err(_) => {}
        }
    }
    Ok((reading::from_snmp(&vars), client.bulk()))
}

async fn ipp_attributes(host: &str, hints: &mut Hints) -> Option<ipp::Attributes> {
    match ipp::printer_attributes(host, hints.ipp_url.as_deref()).await {
        Ok((url, attrs)) => {
            hints.ipp_url = Some(url);
            hints.no_ipp = false;
            Some(attrs)
        }
        Err(_) => {
            hints.no_ipp = true;
            None
        }
    }
}

/// Levels SNMP left blank, from what IPP says of the same cartridge (same
/// kind and colour, or the only one of its kind on both sides).
pub fn fill_levels(snmp: &mut Reading, ipp: &Reading) {
    let consumable = |k: &str| k == "toner" || k == "ink";
    let only = |list: &[reading::Supply], kind: &str| list.iter().filter(|s| s.kind == kind).count() == 1;
    let snmp_list = snmp.supplies.clone();
    for supply in snmp.supplies.iter_mut().filter(|s| s.level.is_none() && consumable(&s.kind)) {
        let twin = ipp.supplies.iter().find(|i| {
            i.kind == supply.kind
                && i.level.is_some()
                && match (&i.color, &supply.color) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    _ => only(&ipp.supplies, &i.kind) && only(&snmp_list, &supply.kind),
                }
        });
        if let Some(twin) = twin {
            supply.level = twin.level;
        }
    }
}

async fn check(p: &PrinterEntry, creds: &snmp::Credentials, mut hints: Hints) -> (Outcome, Hints) {
    let host = p.host.trim();
    let auto = p.protocol != "snmp" && p.protocol != "ipp";
    let order: &[&str] = match p.protocol.as_str() {
        "snmp" => &["snmp"],
        "ipp" => &["ipp"],
        // What answered last time goes first.
        _ if hints.via == "ipp" => &["ipp", "snmp"],
        _ => &["snmp", "ipp"],
    };
    let mut refused = None;
    let mut thin: Option<Reading> = None;
    for protocol in order {
        if *protocol == "snmp" {
            match read_snmp(host, creds, hints.bulk.unwrap_or(true)).await {
                Ok((mut reading, bulk)) => {
                    hints.bulk = Some(bulk);
                    // An agent that knows little of the printer (a print server's
                    // own MIB only): IPP may say more, and is asked too.
                    if auto && reading.state == reading::State::Unknown && reading.supplies.is_empty() {
                        thin = Some(reading);
                        continue;
                    }
                    let blind = reading.supplies.iter().any(|s| s.level.is_none() && (s.kind == "toner" || s.kind == "ink"));
                    if auto && blind && !hints.no_ipp {
                        if let Some(attrs) = ipp_attributes(host, &mut hints).await {
                            fill_levels(&mut reading, &reading::from_ipp(&attrs));
                        }
                    }
                    hints.via = "snmp".into();
                    return (Outcome::Read(reading), hints);
                }
                Err(snmp::Error::Refused(why)) => refused = Some(why.to_string()),
                Err(_) => {}
            }
        } else if let Some(attrs) = ipp_attributes(host, &mut hints).await {
            let mut reading = reading::from_ipp(&attrs);
            // The page counter only SNMP has.
            if let Some(snmp) = &thin {
                reading.pages = reading.pages.or(snmp.pages);
                if reading.serial.is_empty() {
                    reading.serial.clone_from(&snmp.serial);
                }
            }
            hints.via = "ipp".into();
            return (Outcome::Read(reading), hints);
        }
    }
    if let Some(reading) = thin {
        hints.via = "snmp".into();
        return (Outcome::Read(reading), hints);
    }
    (refused.map_or(Outcome::Unreachable, Outcome::Refused), hints)
}

/// One round: every printer on the list, a few at a time.
async fn check_all(app: &AppHandle) {
    let Some(prefs) = prefs_of(app) else { return };
    forget_removed(&prefs);
    if prefs.list.is_empty() {
        emit(app, &prefs, None);
        return;
    }
    let creds = credentials(&prefs);
    let jobs: Vec<(PrinterEntry, Hints)> = {
        let known = KNOWN.lock().unwrap();
        prefs
            .list
            .iter()
            .filter(|p| !p.host.trim().is_empty())
            .map(|p| (p.clone(), known.get(&p.id).map(|k| k.hints.clone()).unwrap_or_default()))
            .collect()
    };
    let outcomes: Vec<(PrinterEntry, Outcome, Hints)> = futures_util::stream::iter(jobs)
        .map(|(p, hints)| {
            let creds = creds.clone();
            async move {
                let (outcome, hints) = check(&p, &creds, hints).await;
                (p, outcome, hints)
            }
        })
        .buffer_unordered(AT_ONCE)
        .collect()
        .await;
    apply(app, &prefs, outcomes);
}

/// Printers taken off the list: their problems end quietly, their history closes.
fn forget_removed(prefs: &PrintersPrefs) {
    let gone: Vec<(String, Vec<String>)> = {
        let mut known = KNOWN.lock().unwrap();
        let ids: Vec<String> = known.keys().filter(|id| !prefs.list.iter().any(|p| &p.id == *id)).cloned().collect();
        ids.into_iter().filter_map(|id| known.remove(&id).map(|k| (id, k.on.into_keys().collect()))).collect()
    };
    if gone.is_empty() {
        return;
    }
    let now = now_ms();
    let mut board = BOARD.lock().unwrap();
    for (_, keys) in &gone {
        for key in keys {
            board.dismiss(key);
        }
    }
    drop(board);
    with_history(|h| {
        for (id, _) in &gone {
            h.close_printer(id, now);
        }
        let _ = h.save(&History::path());
    });
}

fn thresholds(p: &PrinterEntry, prefs: &PrintersPrefs) -> (u8, u8) {
    let warn = if p.toner_warn > 0 { p.toner_warn } else { prefs.toner_warn };
    let critical = if p.toner_critical > 0 { p.toner_critical } else { prefs.toner_critical };
    (warn.min(100), critical.min(warn))
}

pub fn problem_key(printer: &str, p: &Problem) -> String {
    format!("printer:{printer}:{}:{}", p.code, p.sub)
}

fn display_name(p: &PrinterEntry, reading: Option<&Reading>) -> String {
    [p.name.trim(), reading.map_or("", |r| r.model.as_str()), p.host.trim()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn web_page(host: &str) -> String {
    let host = host.trim();
    if host.contains(':') && !host.starts_with('[') { format!("http://[{host}]/") } else { format!("http://{host}/") }
}

/// What one round of checks found, as the island's problem list and the history see it.
fn apply(app: &AppHandle, prefs: &PrintersPrefs, outcomes: Vec<(PrinterEntry, Outcome, Hints)>) {
    let now = now_ms();
    let mut incoming: Vec<Alert> = Vec::new();
    // Who each started problem belongs to, for the history.
    let mut owners: HashMap<String, (String, String, Problem)> = HashMap::new();
    {
        let mut known = KNOWN.lock().unwrap();
        for (p, outcome, hints) in outcomes {
            let k = known.entry(p.id.clone()).or_default();
            k.checked_at = now;
            k.hints = hints;
            let problems: Option<Vec<Problem>> = match outcome {
                Outcome::Read(mut reading) => {
                    k.misses = 0;
                    let (warn, critical) = thresholds(&p, prefs);
                    reading::apply_thresholds(&mut reading, warn, critical);
                    let problems = reading.problems.clone();
                    k.reading = Some(reading);
                    Some(problems)
                }
                Outcome::Refused(why) => {
                    k.misses = 0;
                    // What else is wrong can't be known: it stays as it was.
                    let mut list: Vec<Problem> =
                        k.on.values().filter(|q| q.code != "unreachable" && q.code != "refused").cloned().collect();
                    list.push(reading::problem("refused", why));
                    Some(list)
                }
                Outcome::Unreachable => {
                    k.misses += 1;
                    // One check without an answer says nothing yet.
                    (k.misses >= MISSES).then(|| {
                        let mut list: Vec<Problem> = k.on.values().filter(|q| q.code != "unreachable").cloned().collect();
                        list.push(reading::problem("unreachable", ""));
                        list
                    })
                }
            };
            let Some(problems) = problems else { continue };
            let name = display_name(&p, k.reading.as_ref());
            let place = if p.location.trim().is_empty() {
                k.reading.as_ref().map_or(String::new(), |r| r.location.clone())
            } else {
                p.location.trim().to_string()
            };
            let alert = |key: &str, q: &Problem, firing: bool| Alert {
                key: key.to_string(),
                source: name.clone(),
                title: t(reading::label(&q.code)),
                detail: [q.detail.as_str(), place.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · "),
                severity: q.severity,
                firing,
                url: Some(web_page(&p.host)),
                at: now,
            };
            let now_on: BTreeMap<String, Problem> = problems.into_iter().map(|q| (problem_key(&p.id, &q), q)).collect();
            for (key, q) in &now_on {
                incoming.push(alert(key, q, true));
                owners.insert(key.clone(), (p.id.clone(), name.clone(), q.clone()));
            }
            for (key, q) in &k.on {
                if !now_on.contains_key(key) {
                    incoming.push(alert(key, q, false));
                }
            }
            if !now_on.keys().eq(k.on.keys()) {
                k.since = now;
            }
            k.on = now_on;
        }
    }
    let changes = BOARD.lock().unwrap().apply(incoming.clone());
    remember(&changes, &owners, now);
    for change in changes.iter().take(MAX_NOTIFY) {
        let (a, body) = match change {
            Change::Fired(a) => (a, a.detail.clone()),
            Change::Resolved(a) => (a, t("Resolved")),
        };
        let n = Notification {
            id: notify::next_id("printers"),
            source: "printers".into(),
            app: a.source.clone(),
            title: a.title.clone(),
            body,
            icon: None,
            at: now,
            tag: Some(a.key.clone()),
        };
        notify::show(app, n, a.url.clone().map(Target::Url).unwrap_or(Target::None));
    }
    if prefs.to_alerts {
        alerts::merge(app, incoming);
    }
    emit(app, prefs, event(&changes));
}

/// The pill's badge and sound: the worst new problem, else a fixed one.
fn event(changes: &[Change]) -> Option<IntegrationEvent> {
    let worst = changes
        .iter()
        .filter_map(|c| match c {
            Change::Fired(a) => Some(a),
            Change::Resolved(_) => None,
        })
        .max_by_key(|a| a.severity);
    if let Some(a) = worst {
        return Some(IntegrationEvent {
            success: false,
            label: format!("{} · {}", a.title, a.source),
            detail: Some(a.detail.clone()).filter(|d| !d.is_empty()),
        });
    }
    changes.iter().find_map(|c| match c {
        Change::Resolved(a) => Some(IntegrationEvent {
            success: true,
            label: format!("{} · {}", a.title, a.source),
            detail: Some(t("Resolved")),
        }),
        Change::Fired(_) => None,
    })
}

fn with_history<T>(f: impl FnOnce(&mut History) -> T) -> T {
    let mut slot = HISTORY.lock().unwrap();
    let history = slot.get_or_insert_with(|| History::load(&History::path()));
    f(history)
}

/// Problems that began and ended, into history.json.
fn remember(changes: &[Change], owners: &HashMap<String, (String, String, Problem)>, now: f64) {
    if changes.is_empty() {
        return;
    }
    with_history(|h| {
        for change in changes {
            match change {
                Change::Fired(a) => {
                    if let Some((printer, name, p)) = owners.get(&a.key) {
                        h.opened(&a.key, printer, name, p, a.at);
                    }
                }
                Change::Resolved(a) => h.closed(&a.key, now),
            }
        }
        h.prune(now);
        if let Err(e) = h.save(&History::path()) {
            log::line(format!("printers: could not save the history: {e}"));
        }
    });
}

// ── What is shown ─────────────────────────────────────────────────────────────

/// One printer as the island and Settings show it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    pub name: String,
    pub host: String,
    pub location: String,
    pub model: String,
    pub serial: String,
    pub panel: String,
    /// "idle", "printing", "warmup", "stopped", "unknown", "unreachable", or
    /// "waiting" before its first check.
    pub state: String,
    /// "snmp" or "ipp": what answered.
    pub via: String,
    pub problems: Vec<Problem>,
    pub supplies: Vec<reading::Supply>,
    pub trays: Vec<reading::Tray>,
    pub pages: Option<u64>,
    pub checked_at: f64,
    pub since: f64,
    /// Problems in the last 30 days.
    pub incidents: usize,
    pub web: String,
    pub worst: Option<Severity>,
}

fn views(prefs: &PrintersPrefs) -> Vec<View> {
    let known = KNOWN.lock().unwrap();
    let now = now_ms();
    let counts: HashMap<String, usize> =
        with_history(|h| prefs.list.iter().map(|p| (p.id.clone(), h.count(&p.id, now))).collect());
    let mut out: Vec<View> = prefs
        .list
        .iter()
        .map(|p| {
            let k = known.get(&p.id);
            let r = k.and_then(|k| k.reading.as_ref());
            let problems: Vec<Problem> = k.map(|k| k.on.values().cloned().collect()).unwrap_or_default();
            let state = match k {
                Some(k) if k.misses >= MISSES => "unreachable".to_string(),
                Some(k) if k.checked_at == 0.0 => "waiting".into(),
                None => "waiting".into(),
                Some(_) => r.map_or("unknown".into(), |r| serde_json::to_value(r.state).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()),
            };
            View {
                id: p.id.clone(),
                name: display_name(p, r),
                host: p.host.trim().to_string(),
                location: if p.location.trim().is_empty() { r.map_or(String::new(), |r| r.location.clone()) } else { p.location.trim().to_string() },
                model: r.map_or(String::new(), |r| r.model.clone()),
                serial: r.map_or(String::new(), |r| r.serial.clone()),
                panel: r.map_or(String::new(), |r| r.panel.clone()),
                state,
                via: k.map_or(String::new(), |k| k.hints.via.clone()),
                worst: problems.iter().map(|q| q.severity).max(),
                problems,
                supplies: r.map_or(Vec::new(), |r| r.supplies.clone()),
                trays: r.map_or(Vec::new(), |r| r.trays.clone()),
                pages: r.and_then(|r| r.pages),
                checked_at: k.map_or(0.0, |k| k.checked_at),
                since: k.map_or(0.0, |k| k.since),
                incidents: counts.get(&p.id).copied().unwrap_or(0),
                web: web_page(&p.host),
            }
        })
        .collect();
    // The worst first; those without a problem keep the list's order.
    out.sort_by(|a, b| b.worst.cmp(&a.worst));
    out
}

fn data(prefs: &PrintersPrefs) -> Json {
    let list = views(prefs);
    let checked = list.iter().map(|v| v.checked_at).fold(0.0, f64::max);
    json!({ "printers": list, "intervalMinutes": prefs.interval_minutes, "checkedAt": checked })
}

fn emit(app: &AppHandle, prefs: &PrintersPrefs, event: Option<IntegrationEvent>) {
    let update = IntegrationUpdate { id: PILL_ID, data: data(prefs), error: None, event };
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "integration", update);
}

/// A printer's state in words, for the reports.
fn state_word(state: &str) -> String {
    t(match state {
        "idle" => n_("Idle"),
        "printing" => n_("Printing"),
        "warmup" => n_("Warming up"),
        "stopped" => n_("Stopped"),
        "unreachable" => n_("Not responding"),
        "waiting" => n_("Not checked yet"),
        _ => n_("Unknown"),
    })
}

fn lines(prefs: &PrintersPrefs) -> Vec<Line> {
    views(prefs)
        .into_iter()
        .map(|v| Line {
            supplies: v
                .supplies
                .iter()
                .map(|s| match s.level {
                    Some(l) => format!("{} {l}%", s.name),
                    None => s.name.clone(),
                })
                .collect(),
            state: state_word(&v.state),
            name: v.name,
            location: v.location,
            host: v.host,
            model: v.model,
            serial: v.serial,
            problems: v.problems,
            pages: v.pages,
            checked: v.checked_at,
        })
        .collect()
}

fn local_stamp() -> (String, String) {
    let t = crate::platform::local_time();
    (
        format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute),
        format!("{:04}-{:02}-{:02}-{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute),
    )
}

/// "08:30" → (8, 30).
pub fn parse_time(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// Once a day, at the time set or the first moment after it: how the printers are.
fn summary_if_due(app: &AppHandle) {
    let Some(prefs) = prefs_of(app) else { return };
    let Some(at) = parse_time(&prefs.daily_summary) else { return };
    let now = crate::platform::local_time();
    let today = (now.year, now.month, now.day);
    {
        let mut last = SUMMARY_DAY.lock().unwrap();
        if *last == Some(today) || (now.hour, now.minute) < at {
            return;
        }
        *last = Some(today);
    }
    let list = views(&prefs);
    if list.is_empty() {
        return;
    }
    let wrong: Vec<&View> = list.iter().filter(|v| !v.problems.is_empty()).collect();
    let total = list.len().to_string();
    let title = if wrong.is_empty() {
        tn("{count} printer is fine", "All {count} printers are fine", list.len() as u64, &[])
    } else {
        tn("{count} of {total} printers needs attention", "{count} of {total} printers need attention", wrong.len() as u64, &[("total", &total)])
    };
    let body = wrong
        .iter()
        .map(|v| format!("{}: {}", v.name, v.problems.iter().map(report::describe).collect::<Vec<_>>().join(", ")))
        .collect::<Vec<_>>()
        .join(" · ");
    let n = Notification {
        id: notify::next_id("printers"),
        source: "printers".into(),
        app: "Printers".into(),
        title,
        body,
        icon: None,
        at: now_ms(),
        tag: Some("printers:summary".into()),
    };
    notify::show(app, n, Target::None);
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn prefs_from(shared: &tauri::State<crate::Shared>) -> PrintersPrefs {
    shared.settings.lock().unwrap().printers.clone()
}

/// What the island's card shows, for Settings' list.
#[tauri::command]
pub fn printers_state(shared: tauri::State<crate::Shared>) -> Json {
    data(&prefs_from(&shared))
}

#[tauri::command]
pub fn printers_check_now() {
    check_now();
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverAnswer {
    found: Vec<discover::Found>,
    /// The range swept, or "" without a sweep.
    range: String,
    swept: usize,
}

/// Find printers: this computer's and the announced ones, and with `sweep`,
/// every address of the range asked over SNMP.
#[tauri::command]
pub async fn printers_discover(shared: tauri::State<'_, crate::Shared>, sweep: bool) -> Result<DiscoverAnswer, String> {
    let prefs = prefs_from(&shared);
    let (system, mdns) = tokio::join!(discover::system(), discover::mdns());
    let mut lists = vec![system, mdns];
    let (mut range, mut swept) = (String::new(), 0);
    if sweep {
        range = if prefs.sweep_range.trim().is_empty() {
            discover::default_range().ok_or_else(|| t("This computer's network address isn't known: enter a range."))?
        } else {
            prefs.sweep_range.trim().to_string()
        };
        let hosts = discover::parse_range(&range).ok_or_else(|| tf("Not a range Ciel can sweep: {range}", &[("range", &range)]))?;
        swept = hosts.len();
        let community = secrets::get(COMMUNITY_KEY).unwrap_or_else(|| "public".into());
        let found = discover::sweep(&hosts, snmp::Version::parse(&prefs.snmp_version), &community).await;
        log::line(format!("printers: swept {swept} addresses, {} printers", found.len()));
        lists.push(found);
    }
    let found = discover::merge(lists);
    log::line(format!("printers: found {}", found.len()));
    Ok(DiscoverAnswer { found, range, swept })
}

/// Copies the report, every printer now and the last 30 days.
#[tauri::command]
pub fn printers_copy_report(shared: tauri::State<crate::Shared>) -> bool {
    let prefs = prefs_from(&shared);
    let (when, _) = local_stamp();
    let text = with_history(|h| report::text(&lines(&prefs), h, &tf("Printers, {date}", &[("date", &when)]), now_ms()));
    crate::flow::paste::copy(&text)
}

/// Writes two CSV files (the printers now, the last 30 days' problems) to
/// Ciel's printers folder and shows it. Answers the folder.
#[tauri::command]
pub fn printers_export(shared: tauri::State<crate::Shared>) -> Result<String, String> {
    let prefs = prefs_from(&shared);
    let dir = crate::settings::local_dir().join("printers");
    crate::platform::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    let (_, stamp) = local_stamp();
    // A BOM, so Excel reads the accents right.
    let status = format!("\u{FEFF}{}", report::csv_status(&lines(&prefs)));
    let history = format!("\u{FEFF}{}", with_history(|h| report::csv_history(h)));
    std::fs::write(dir.join(format!("printers-{stamp}.csv")), status).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("printers-history-{stamp}.csv")), history).map_err(|e| e.to_string())?;
    let folder = dir.to_string_lossy().into_owned();
    crate::platform::reveal_folder(&folder);
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The printers around this computer, found and read for real:
    /// `cargo test -p ciel --lib live_printers -- --ignored --nocapture`.
    /// Read-only questions (SNMP v2c "public", IPP Get-Printer-Attributes).
    #[tokio::test]
    #[ignore]
    async fn live_printers() {
        let (system, mdns) = tokio::join!(discover::system(), discover::mdns());
        println!("system: {system:?}\nmdns: {mdns:?}");
        let found = discover::merge(vec![system, mdns]);
        let creds = snmp::Credentials::community(snmp::Version::V2c, "public");
        for f in found {
            println!("\n== {} ({}) {:?}", f.name, f.host, f.sources);
            let started = std::time::Instant::now();
            match read_snmp(&f.host, &creds, true).await {
                Ok((r, _)) => println!("snmp in {:?}: {:?} {:?}\n  problems {:?}\n  supplies {:?}\n  trays {:?}\n  pages {:?} panel {:?}", started.elapsed(), r.state, r.model, r.problems, r.supplies, r.trays, r.pages, r.panel),
                Err(e) => println!("snmp in {:?}: {e:?}", started.elapsed()),
            }
            let started = std::time::Instant::now();
            match ipp::printer_attributes(&f.host, None).await {
                Ok((url, attrs)) => {
                    let r = reading::from_ipp(&attrs);
                    println!("ipp {url} in {:?}: {:?} {:?}\n  problems {:?}\n  supplies {:?}\n  panel {:?}\n  reasons {:?}", started.elapsed(), r.state, r.model, r.problems, r.supplies, r.panel, attrs.get("printer-state-reasons"));
                }
                Err(e) => println!("ipp in {:?}: {e:?}", started.elapsed()),
            }
        }
    }

    #[test]
    fn ipp_fills_the_levels_snmp_left_blank() {
        let supply = |name: &str, kind: &str, color: Option<&str>, level: Option<u8>| reading::Supply {
            name: name.into(),
            kind: kind.into(),
            color: color.map(String::from),
            level,
        };
        // A Brother: SNMP names the cartridge and the drum, IPP knows the toner's level.
        let mut snmp = Reading {
            supplies: vec![supply("Black Toner Cartridge", "toner", Some("#000000"), None), supply("Drum Unit", "drum", None, Some(75))],
            ..Reading::default()
        };
        let ipp = Reading { supplies: vec![supply("BK", "toner", Some("#000000"), Some(40))], ..Reading::default() };
        fill_levels(&mut snmp, &ipp);
        assert_eq!(snmp.supplies[0].level, Some(40));
        assert_eq!(snmp.supplies[1].level, Some(75));
        // Colours that don't match are never mixed up.
        let mut color = Reading { supplies: vec![supply("Cyan", "toner", Some("#00A0E0"), None)], ..Reading::default() };
        fill_levels(&mut color, &ipp);
        assert_eq!(color.supplies[0].level, None);
        // Neither side says a colour, one cartridge each: they're the same.
        let mut mono = Reading { supplies: vec![supply("Toner", "toner", None, None)], ..Reading::default() };
        fill_levels(&mut mono, &Reading { supplies: vec![supply("Black", "toner", None, Some(12))], ..Reading::default() });
        assert_eq!(mono.supplies[0].level, Some(12));
    }

    #[test]
    fn a_printers_own_thresholds_win_and_stay_in_order() {
        let prefs = PrintersPrefs::default();
        let mut p = PrinterEntry::default();
        assert_eq!(thresholds(&p, &prefs), (15, 5));
        p.toner_warn = 30;
        assert_eq!(thresholds(&p, &prefs), (30, 5));
        p.toner_critical = 40;
        assert_eq!(thresholds(&p, &prefs), (30, 30), "critical never above low");
    }

    #[test]
    fn names_fall_back_to_the_model_then_the_address() {
        let mut p = PrinterEntry { host: " 10.0.2.40 ".into(), ..PrinterEntry::default() };
        assert_eq!(display_name(&p, None), "10.0.2.40");
        let r = Reading { model: "Canon MF650C Series".into(), ..Reading::default() };
        assert_eq!(display_name(&p, Some(&r)), "Canon MF650C Series");
        p.name = "Accounting".into();
        assert_eq!(display_name(&p, Some(&r)), "Accounting");
        assert_eq!(web_page("10.0.2.40"), "http://10.0.2.40/");
        assert_eq!(web_page("fe80::1"), "http://[fe80::1]/");
    }

    #[test]
    fn summary_times_are_read_strictly() {
        assert_eq!(parse_time("08:30"), Some((8, 30)));
        assert_eq!(parse_time("8:05"), Some((8, 5)));
        assert_eq!(parse_time(""), None);
        assert_eq!(parse_time("24:00"), None);
        assert_eq!(parse_time("noon"), None);
    }

    #[test]
    fn keys_tell_two_cartridges_apart() {
        let low = |sub: &str| Problem { sub: sub.into(), ..reading::problem("supplyLow", "") };
        assert_ne!(problem_key("p1", &low("Cyan Toner")), problem_key("p1", &low("Black Toner")));
        assert_eq!(problem_key("p1", &reading::problem("jammed", "")), "printer:p1:jammed:");
    }

    #[test]
    fn the_newest_change_sets_the_badge_with_the_printers_name() {
        let alert = |severity, firing| Alert {
            key: "k".into(),
            source: "Canon 2F".into(),
            title: "Paper jam".into(),
            detail: String::new(),
            severity,
            firing,
            url: None,
            at: 0.0,
        };
        let e = event(&[Change::Fired(alert(Severity::Critical, true))]).unwrap();
        assert_eq!((e.success, e.label.as_str()), (false, "Paper jam · Canon 2F"));
        let e = event(&[Change::Resolved(alert(Severity::Critical, false))]).unwrap();
        assert!(e.success);
        assert!(event(&[]).is_none());
    }
}
