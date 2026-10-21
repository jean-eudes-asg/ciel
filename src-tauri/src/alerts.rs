// Alerts, the pill `integration_alerts`: what monitoring tools send shows in
// the island — Grafana, Prometheus' Alertmanager, Uptime Kuma, Zabbix (with
// ZABBIX_SCRIPT below), or anything that can POST JSON or a line of text. A new
// problem is a notification (Open goes to the page its tool sent along), the
// pill lists what is still wrong, worst first, and a recovery takes it off.
//
// Ciel listens over HTTP while the pill is on and Ciel isn't paused: on this
// computer only, or on the network once the user allows it (Settings →
// Integrations → Alerts). Every request carries the pill's token, kept in the
// keychain like a key; the address with it goes to the clipboard from here, so
// the token never reaches a webview. On the network it is plain HTTP, as any
// webhook to an http:// address is: the token travels in the clear.
//
// What is still wrong is kept in memory while Ciel runs; nothing is written.
// The log says how many alerts came and from where, never what they say.

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Notify, Semaphore};

use crate::integrations::{IntegrationEvent, IntegrationUpdate};
use crate::notify::{self, Notification, Target};
use crate::settings::Settings;
use crate::{log, secrets};

pub const PILL_ID: &str = "integration_alerts";
pub const TOKEN_KEY: &str = "alerts-token";
pub const DEFAULT_PORT: u16 = 9797;

/// The request line and headers, at most.
const MAX_HEAD: usize = 16 * 1024;
/// A body, at most: Grafana sends a whole group of alerts at once.
const MAX_BODY: usize = 1024 * 1024;
/// A request has this long to arrive whole.
const READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Requests served at once; one more is closed unanswered.
const MAX_CONNECTIONS: usize = 16;
/// Alerts on the list; past it, the oldest goes.
const MAX_ACTIVE: usize = 100;
/// The last recoveries kept, for the card's "all clear" line.
const RECENT: usize = 5;
/// At most this many notifications for one request.
const MAX_NOTIFY: usize = 5;
const MAX_TEXT: usize = 300;

// ── Alerts ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    /// Grafana's and Alertmanager's `severity` label, Zabbix's six levels (by
    /// name or 0–5), and the usual words. Anything else is a warning.
    fn read(value: Option<&Value>) -> Severity {
        let word = match value {
            Some(Value::String(s)) => s.trim().to_lowercase(),
            Some(Value::Number(n)) => n.to_string(),
            _ => return Severity::Warning,
        };
        match word.as_str() {
            "disaster" | "critical" | "crit" | "high" | "emergency" | "emerg" | "alert" | "fatal" | "error" | "err"
            | "major" | "page" | "p1" | "p2" | "4" | "5" => Severity::Critical,
            "information" | "info" | "informational" | "notice" | "low" | "not classified" | "none" | "debug"
            | "ok" | "p5" | "0" | "1" => Severity::Info,
            _ => Severity::Warning,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// Same problem, same key: its recovery finds it by this.
    pub key: String,
    /// "Grafana", "Alertmanager", "Uptime Kuma", "Zabbix", or what the sender says.
    pub source: String,
    pub title: String,
    /// The host and what the tool says about it, whichever there is.
    pub detail: String,
    pub severity: Severity,
    pub firing: bool,
    /// An http(s) page about it, or none.
    pub url: Option<String>,
    /// Unix ms: when Ciel first heard of it, or of its recovery.
    pub at: f64,
}

/// The alerts in one request's body: JSON in Grafana's or Alertmanager's
/// shape, Uptime Kuma's, or Ciel's own (one object, or a list of them); else a
/// line of text. None when there is nothing to show.
pub fn parse(body: &[u8], now: f64) -> Option<Vec<Alert>> {
    let alerts: Vec<Alert> = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(root)) if root.get("alerts").is_some_and(Value::is_array) => from_alertmanager(&root, now),
        Ok(Value::Object(root)) if root.contains_key("heartbeat") || root.contains_key("monitor") => {
            from_uptime_kuma(&root, now).into_iter().collect()
        }
        Ok(Value::Object(root)) => generic(&root, now).into_iter().collect(),
        Ok(Value::Array(list)) => list.iter().filter_map(Value::as_object).filter_map(|o| generic(o, now)).collect(),
        Ok(Value::String(s)) => from_text(&s, now).into_iter().collect(),
        Ok(_) => vec![],
        Err(_) => from_text(&String::from_utf8_lossy(body), now).into_iter().collect(),
    };
    (!alerts.is_empty()).then_some(alerts)
}

/// Grafana's contact point "Webhook" and Alertmanager's `webhook_configs`:
/// a group, its `alerts` each with labels, annotations and a status.
fn from_alertmanager(root: &Map<String, Value>, now: f64) -> Vec<Alert> {
    // Grafana says which org and gives the group a title; Alertmanager doesn't.
    let source = if root.contains_key("orgId") || root.contains_key("title") { "Grafana" } else { "Alertmanager" };
    let common = root.get("commonLabels").and_then(Value::as_object);
    let empty = Map::new();
    let alerts = root.get("alerts").and_then(Value::as_array).into_iter().flatten();
    alerts
        .filter_map(Value::as_object)
        .map(|a| {
            let labels = a.get("labels").and_then(Value::as_object).unwrap_or(&empty);
            let notes = a.get("annotations").and_then(Value::as_object).unwrap_or(&empty);
            let name = text(labels, &["alertname"])
                .or_else(|| common.and_then(|c| text(c, &["alertname"])))
                .unwrap_or_else(|| source.to_string());
            let title = text(notes, &["summary"]).unwrap_or(name);
            let host = text(labels, &["instance", "host", "hostname", "job"]);
            let about = text(notes, &["description", "message"]).or_else(|| text(a, &["valueString"]));
            let id = text(a, &["fingerprint"]).unwrap_or_else(|| {
                labels.iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default())).collect::<Vec<_>>().join(",")
            });
            Alert {
                key: key(source, &id),
                source: source.into(),
                title: clip(&title, MAX_TEXT),
                detail: detail(host, about),
                severity: Severity::read(labels.get("severity").or_else(|| common.and_then(|c| c.get("severity")))),
                firing: firing(a.get("status").or_else(|| root.get("status"))),
                url: web_url(text(a, &["panelURL", "dashboardURL", "generatorURL"])),
                at: now,
            }
        })
        .collect()
}

/// Uptime Kuma's "Webhook" notification, body "application/json": the
/// monitor and its heartbeat, whose status is 0 down, 1 up, 2 pending and 3
/// maintenance. Without a heartbeat it is a notice (a certificate about to
/// expire), or the Test button when there is no monitor either.
fn from_uptime_kuma(root: &Map<String, Value>, now: f64) -> Option<Alert> {
    const SOURCE: &str = "Uptime Kuma";
    let monitor = root.get("monitor").and_then(Value::as_object);
    let msg = text(root, &["msg"]);
    let id = monitor.and_then(|m| text(m, &["id", "name"]));
    let Some(beat) = root.get("heartbeat").and_then(Value::as_object) else {
        let title = msg.or_else(|| monitor.and_then(|m| text(m, &["name"])))?;
        return Some(Alert {
            key: key(SOURCE, &format!("{}:notice", id.as_deref().unwrap_or("test"))),
            source: SOURCE.into(),
            title: clip(&title, MAX_TEXT),
            detail: String::new(),
            severity: if monitor.is_some() { Severity::Warning } else { Severity::Info },
            firing: true,
            url: monitor.and_then(|m| web_url(text(m, &["url"]))),
            at: now,
        });
    };
    let empty = Map::new();
    let monitor = monitor.unwrap_or(&empty);
    let status = beat.get("status").and_then(Value::as_i64).unwrap_or(0);
    let name = text(monitor, &["name"]).or(msg).unwrap_or_else(|| SOURCE.into());
    // An HTTP monitor has its URL; the others a hostname, and "https://" as URL.
    let host = text(monitor, &["hostname"]).or_else(|| web_url(text(monitor, &["url"])));
    Some(Alert {
        key: key(SOURCE, id.as_deref().unwrap_or(&name)),
        source: SOURCE.into(),
        title: clip(&name, MAX_TEXT),
        detail: detail(host, text(beat, &["msg"])),
        severity: if status == 2 { Severity::Warning } else { Severity::Critical },
        firing: status == 0 || status == 2,
        url: web_url(text(monitor, &["url"])),
        at: now,
    })
}

/// Ciel's own shape, which the Zabbix script sends and any script can:
/// `{"title", "status", "severity", "host", "message", "id", "url", "source"}`,
/// each also under its usual other names. Only a title (or a message) is needed.
fn generic(obj: &Map<String, Value>, now: f64) -> Option<Alert> {
    let message = text(obj, &["message", "description", "text", "body", "detail", "msg"]);
    let title = text(obj, &["title", "name", "summary", "subject", "alertname"])
        .or_else(|| message.as_deref().map(first_line))
        .filter(|t| !t.is_empty())?;
    let source = text(obj, &["source", "tool", "from"]).unwrap_or_else(|| "Webhook".into());
    let host = text(obj, &["host", "hostname", "instance", "server"]);
    let id = text(obj, &["id", "key", "fingerprint", "event_id", "eventid", "incident"])
        .unwrap_or_else(|| format!("{}/{title}", host.as_deref().unwrap_or("")));
    // A message that only says the title again says nothing more.
    let about = message.filter(|m| first_line(m) != title);
    Some(Alert {
        key: key(&source, &id),
        source: clip(&source, 60),
        title: clip(&title, MAX_TEXT),
        detail: detail(host, about),
        severity: Severity::read(obj.get("severity").or_else(|| obj.get("priority")).or_else(|| obj.get("level"))),
        firing: firing(obj.get("status").or_else(|| obj.get("state")).or_else(|| obj.get("firing"))),
        url: web_url(text(obj, &["url", "link", "href"])),
        at: now,
    })
}

/// A body that isn't JSON: its first line is the alert (`curl -d "Disk full on srv1"`).
fn from_text(body: &str, now: f64) -> Option<Alert> {
    let title = first_line(body);
    if title.is_empty() {
        return None;
    }
    let rest = body.trim().lines().skip(1).collect::<Vec<_>>().join(" ");
    Some(Alert {
        key: key("Webhook", &title),
        source: "Webhook".into(),
        title: clip(&title, MAX_TEXT),
        detail: clip(&rest, MAX_TEXT),
        severity: Severity::Warning,
        firing: true,
        url: None,
        at: now,
    })
}

/// Whether a status says the problem is still there: "resolved", "ok", "up",
/// 0, false and the like say it is over; anything else, or nothing, says it's on.
fn firing(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !matches!(
            s.trim().to_lowercase().as_str(),
            "resolved" | "ok" | "up" | "recovery" | "recovered" | "inactive" | "normal" | "closed" | "cleared" | "0"
                | "false"
        ),
        _ => true,
    }
}

/// The first of `keys` that `obj` has as a non-empty string, or a number.
fn text(obj: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| match obj.get(*k)? {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn key(source: &str, id: &str) -> String {
    clip(&format!("{}:{id}", source.to_lowercase()), 200)
}

/// "host · what it says", whichever of them there is.
fn detail(host: Option<String>, about: Option<String>) -> String {
    let parts: Vec<String> = [host, about].into_iter().flatten().collect();
    clip(&parts.join(" · "), MAX_TEXT)
}

fn first_line(s: &str) -> String {
    s.trim().lines().next().unwrap_or("").trim().to_string()
}

/// On one line, and `max` characters at most.
fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s,
    }
}

/// A page Open may go to: http(s), with a host, nothing else.
fn web_url(url: Option<String>) -> Option<String> {
    url.filter(|u| {
        let rest = u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or("");
        !rest.is_empty() && !rest.starts_with('/') && u.len() <= 2048 && !u.chars().any(char::is_whitespace)
    })
}

// ── What is wrong now ─────────────────────────────────────────────────────────

/// The problems still on, newest first, and the last recoveries.
#[derive(Default)]
pub struct Board {
    active: Vec<Alert>,
    resolved: VecDeque<Alert>,
}

/// What a request changed, each a notification.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// A new problem, or one that got worse.
    Fired(Alert),
    Resolved(Alert),
}

impl Board {
    pub fn apply(&mut self, incoming: Vec<Alert>) -> Vec<Change> {
        let mut changes = Vec::new();
        for alert in incoming {
            let known = self.active.iter().position(|a| a.key == alert.key);
            match (alert.firing, known) {
                (true, Some(i)) => {
                    // Sent again (Grafana repeats them): updated, since when it is
                    // kept, and told again only if it got worse.
                    let worse = alert.severity > self.active[i].severity;
                    self.active[i] = Alert { at: self.active[i].at, ..alert.clone() };
                    if worse {
                        changes.push(Change::Fired(alert));
                    }
                }
                (true, None) => {
                    self.resolved.retain(|a| a.key != alert.key);
                    self.active.insert(0, alert.clone());
                    self.active.truncate(MAX_ACTIVE);
                    changes.push(Change::Fired(alert));
                }
                (false, Some(i)) => {
                    let mut done = self.active.remove(i);
                    done.firing = false;
                    done.at = alert.at;
                    self.resolved.push_front(done.clone());
                    self.resolved.truncate(RECENT);
                    changes.push(Change::Resolved(done));
                }
                // The end of something Ciel never saw start: nothing to take off.
                (false, None) => {}
            }
        }
        changes
    }

    /// Taken off the list by hand. False when it wasn't there.
    pub fn dismiss(&mut self, key: &str) -> bool {
        let before = self.active.len();
        self.active.retain(|a| a.key != key);
        self.active.len() != before
    }

    /// What the card shows: the worst first, the newest first among equals.
    pub fn data(&self) -> Value {
        let mut active = self.active.clone();
        active.sort_by(|a, b| b.severity.cmp(&a.severity));
        json!({ "active": active, "resolved": self.resolved })
    }
}

/// The pill's badge and sound: the worst new problem, else a recovery.
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
            success: a.severity == Severity::Info,
            label: a.title.clone(),
            detail: Some(a.detail.clone()).filter(|d| !d.is_empty()),
        });
    }
    changes.iter().find_map(|c| match c {
        Change::Resolved(a) => Some(IntegrationEvent {
            success: true,
            label: a.title.clone(),
            detail: Some(crate::i18n::t("Resolved")),
        }),
        Change::Fired(_) => None,
    })
}

// ── HTTP ──────────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
pub struct Head {
    method: String,
    path: String,
    /// Names in lower case.
    headers: Vec<(String, String)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

/// The request line and the headers; an HTTP status when they won't do.
pub fn parse_head(raw: &[u8]) -> Result<Head, u16> {
    let text = std::str::from_utf8(raw).map_err(|_| 400u16)?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(method), Some(path), Some(version)) = (first.next(), first.next(), first.next()) else {
        return Err(400);
    };
    if !version.starts_with("HTTP/1.") {
        return Err(505);
    }
    let headers = lines
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (name, value) = l.split_once(':').ok_or(400u16)?;
            Ok((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect::<Result<Vec<_>, u16>>()?;
    Ok(Head { method: method.to_string(), path: path.to_string(), headers })
}

/// Whether the request is for the pill and carries its token: at the end of
/// the path (`/alerts/<token>`), or on `/alerts` as a bearer token or
/// `X-Ciel-Token`. 404 for any other path, 401 without the right token.
pub fn authorize(head: &Head, token: &str) -> Result<(), u16> {
    let path = head.path.split('?').next().unwrap_or("").trim_end_matches('/');
    let given = match path.strip_prefix("/alerts") {
        Some("") => head
            .header("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .or_else(|| head.header("x-ciel-token")),
        Some(rest) => rest.strip_prefix('/').ok_or(404u16)?.into(),
        None => return Err(404),
    };
    let ok = given.is_some_and(|g| !token.is_empty() && same(g.trim().as_bytes(), token.as_bytes()));
    if ok { Ok(()) } else { Err(401) }
}

/// Compares in the same time whatever matches, so the token can't be guessed by timing.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// One request, its head checked before its body is read.
async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(stream: &mut S, token: &str) -> Result<(Head, Vec<u8>), u16> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(431);
        }
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = parse_head(&buf[..end])?;
    authorize(&head, token)?;
    if head.header("transfer-encoding").is_some_and(|v| !v.eq_ignore_ascii_case("identity")) {
        return Err(411);
    }
    let length = match head.header("content-length") {
        Some(v) => v.parse::<usize>().map_err(|_| 400u16)?,
        None => 0,
    };
    if length > MAX_BODY {
        return Err(413);
    }
    let mut body = buf.split_off(end + 4);
    // curl, and Zabbix through it, waits for this before sending a big body.
    if body.len() < length && head.header("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.map_err(|_| 400u16)?;
    }
    while body.len() < length {
        let n = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok((head, body))
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "HTTP Version Not Supported",
    }
}

async fn respond<S: AsyncWrite + Unpin>(stream: &mut S, status: u16, body: &Value) -> std::io::Result<()> {
    let body = body.to_string();
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

// ── The listener ──────────────────────────────────────────────────────────────

struct Listener {
    generation: u64,
    addr: SocketAddr,
    stop: Arc<Notify>,
}

/// Listening, or why not: what Settings shows.
#[derive(Clone, Default)]
struct Status {
    listening: Option<SocketAddr>,
    error: Option<String>,
}

static LISTENER: LazyLock<Mutex<Option<Listener>>> = LazyLock::new(|| Mutex::new(None));
static GENERATION: AtomicU64 = AtomicU64::new(0);
static BOARD: LazyLock<Mutex<Board>> = LazyLock::new(|| Mutex::new(Board::default()));
static STATUS: LazyLock<Mutex<Status>> = LazyLock::new(|| Mutex::new(Status::default()));
/// The token requests are checked against, from the keychain when listening starts.
static TOKEN: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new()));

/// Where Ciel listens: this computer only, unless the network is allowed.
pub fn bind_address(settings: &Settings) -> SocketAddr {
    let ip = if settings.alerts_lan { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
    let port = if settings.alerts_port == 0 { DEFAULT_PORT } else { settings.alerts_port };
    SocketAddr::new(IpAddr::V4(ip), port)
}

/// Listens when it should — the pill declared, Ciel not paused — where the
/// settings say, and stops otherwise. Switching the pill off forgets its list.
pub fn sync(app: &AppHandle, settings: &Settings) {
    let declared = settings.active_integrations.iter().any(|id| id == PILL_ID);
    let want = declared && !crate::integrations::PAUSED.load(Ordering::Relaxed);
    let addr = bind_address(settings);
    let mut listener = LISTENER.lock().unwrap();
    if listener.as_ref().is_some_and(|l| !want || l.addr != addr) {
        if let Some(l) = listener.take() {
            l.stop.notify_one();
        }
        *STATUS.lock().unwrap() = Status::default();
    }
    if !declared {
        *BOARD.lock().unwrap() = Board::default();
    }
    if !want || listener.is_some() {
        return;
    }
    match token() {
        Ok(t) => *TOKEN.lock().unwrap() = t,
        Err(e) => {
            log::line(format!("alerts: no token ({e})"));
            STATUS.lock().unwrap().error = Some(e);
            report(app, None);
            return;
        }
    }
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let stop = Arc::new(Notify::new());
    *listener = Some(Listener { generation, addr, stop: stop.clone() });
    tauri::async_runtime::spawn(run(app.clone(), generation, addr, stop));
}

fn current(generation: u64) -> bool {
    LISTENER.lock().unwrap().as_ref().is_some_and(|l| l.generation == generation)
}

/// The pill's token, made the first time it is needed.
fn token() -> Result<String, String> {
    if let Some(t) = secrets::get(TOKEN_KEY) {
        return Ok(t);
    }
    let t = new_token()?;
    secrets::set(TOKEN_KEY, &t)?;
    Ok(t)
}

fn new_token() -> Result<String, String> {
    use base64::Engine as _;
    use ring::rand::SecureRandom as _;
    let mut bytes = [0u8; 24];
    ring::rand::SystemRandom::new().fill(&mut bytes).map_err(|_| "no randomness".to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// The address, tried again for a moment while the listener before lets it go.
async fn bind(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let mut tries = 0;
    loop {
        match TcpListener::bind(addr).await {
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && tries < 20 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            other => return other,
        }
    }
}

async fn run(app: AppHandle, generation: u64, addr: SocketAddr, stop: Arc<Notify>) {
    let listener = match bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            if !current(generation) {
                return;
            }
            log::line(format!("alerts: can't listen on {addr}: {e}"));
            let port = addr.port().to_string();
            let error = if e.kind() == std::io::ErrorKind::AddrInUse {
                crate::i18n::tf("Port {port} is already in use", &[("port", &port)])
            } else {
                crate::i18n::tf("Can't listen on port {port}: {error}", &[("port", &port), ("error", &e.to_string())])
            };
            STATUS.lock().unwrap().error = Some(error);
            report(&app, None);
            return;
        }
    };
    if !current(generation) {
        return;
    }
    log::line(format!("alerts: listening on {addr}"));
    *STATUS.lock().unwrap() = Status { listening: Some(addr), error: None };
    report(&app, None);

    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, peer)) = accepted else {
                    // Out of file descriptors, say: no busy loop meanwhile.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                let Ok(slot) = slots.clone().try_acquire_owned() else { continue };
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    serve(&app, stream, peer).await;
                    drop(slot);
                });
            }
            _ = stop.notified() => break,
        }
    }
    log::line(format!("alerts: stopped listening on {addr}"));
}

/// One request, answered and closed.
async fn serve(app: &AppHandle, mut stream: tokio::net::TcpStream, peer: SocketAddr) {
    let token = TOKEN.lock().unwrap().clone();
    let (status, answer) = match tokio::time::timeout(READ_TIMEOUT, read_request(&mut stream, &token)).await {
        Ok(Ok((head, body))) => handle(app, &head, &body, peer),
        Ok(Err(401)) => {
            log::line(format!("alerts: refused a request from {} (wrong token)", peer.ip()));
            (401, json!({ "ok": false }))
        }
        Ok(Err(status)) => (status, json!({ "ok": false })),
        Err(_) => (408, json!({ "ok": false })),
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), respond(&mut stream, status, &answer)).await;
}

fn handle(app: &AppHandle, head: &Head, body: &[u8], peer: SocketAddr) -> (u16, Value) {
    match head.method.as_str() {
        // A look from a browser, or a check from the monitoring tool: Ciel is there.
        "GET" | "HEAD" => return (200, json!({ "ok": true })),
        "POST" | "PUT" => {}
        _ => return (405, json!({ "ok": false })),
    }
    let Some(alerts) = parse(body, now_ms()) else {
        return (400, json!({ "ok": false, "error": "no alert in the body" }));
    };
    let count = alerts.len();
    log::line(format!("alerts: {count} from {}", peer.ip()));
    received(app, alerts);
    (200, json!({ "ok": true, "alerts": count }))
}

/// New alerts in: on the list, a notification for each new problem and each
/// recovery, and the pill told.
fn received(app: &AppHandle, alerts: Vec<Alert>) {
    let changes = BOARD.lock().unwrap().apply(alerts);
    for change in changes.iter().take(MAX_NOTIFY) {
        let (alert, body) = match change {
            Change::Fired(a) => (a, a.detail.clone()),
            Change::Resolved(a) => (a, crate::i18n::t("Resolved")),
        };
        let n = Notification {
            id: notify::next_id("alerts"),
            source: "alerts".into(),
            app: alert.source.clone(),
            title: alert.title.clone(),
            body,
            icon: None,
            at: now_ms(),
            // A recovery replaces its problem's notification if that one still waits.
            tag: Some(alert.key.clone()),
        };
        notify::show(app, n, alert.url.clone().map(Target::Url).unwrap_or(Target::None));
    }
    report(app, event(&changes));
}

fn report(app: &AppHandle, event: Option<IntegrationEvent>) {
    let mut data = BOARD.lock().unwrap().data();
    let status = STATUS.lock().unwrap().clone();
    if let Some(addr) = status.listening {
        data["port"] = json!(addr.port());
    }
    let update = IntegrationUpdate { id: PILL_ID, data, error: status.error, event };
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "integration", update);
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

// ── Settings ──────────────────────────────────────────────────────────────────

/// What Settings shows: where to send alerts, the token hidden, and whether Ciel listens.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertsInfo {
    address: String,
    listening: bool,
    error: Option<String>,
}

/// The address to give the monitoring tool: this computer's on the network
/// when the network is allowed, else 127.0.0.1.
fn address(settings: &Settings, token: &str) -> String {
    let localhost = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let host = if settings.alerts_lan { lan_ip().unwrap_or(localhost) } else { localhost };
    format!("http://{}/alerts/{token}", SocketAddr::new(host, bind_address(settings).port()))
}

/// This computer's address on the network it reaches the rest through. A UDP
/// "connection" only picks the route: nothing is sent.
pub(crate) fn lan_ip() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

#[tauri::command]
pub fn alerts_info(shared: tauri::State<crate::Shared>) -> AlertsInfo {
    let settings = shared.settings.lock().unwrap().clone();
    let status = STATUS.lock().unwrap().clone();
    AlertsInfo { address: address(&settings, "••••••••"), listening: status.listening.is_some(), error: status.error }
}

/// Copies the address, token included, out of the clipboard history.
#[tauri::command]
pub fn alerts_copy_address(shared: tauri::State<crate::Shared>) -> bool {
    let settings = shared.settings.lock().unwrap().clone();
    token().is_ok_and(|t| crate::flow::paste::copy(&address(&settings, &t)))
}

/// A new token: the old address stops working at once, and the new one is copied.
#[tauri::command]
pub fn alerts_new_address(shared: tauri::State<crate::Shared>) -> bool {
    let Ok(t) = new_token() else { return false };
    if secrets::set(TOKEN_KEY, &t).is_err() {
        return false;
    }
    *TOKEN.lock().unwrap() = t.clone();
    let settings = shared.settings.lock().unwrap().clone();
    crate::flow::paste::copy(&address(&settings, &t))
}

#[tauri::command]
pub fn alerts_copy_zabbix_script() -> bool {
    crate::flow::paste::copy(ZABBIX_SCRIPT)
}

/// Another pill's problems, listed here too (the Printers pill's, when asked):
/// on the list and off it as they come and go, told by their own pill.
pub fn merge(app: &AppHandle, alerts: Vec<Alert>) {
    if alerts.is_empty() {
        return;
    }
    BOARD.lock().unwrap().apply(alerts);
    report(app, None);
}

/// Takes every alert whose key starts with `prefix` off the list.
pub fn forget(app: &AppHandle, prefix: &str) {
    let gone = {
        let mut board = BOARD.lock().unwrap();
        let before = board.active.len();
        board.active.retain(|a| !a.key.starts_with(prefix));
        board.resolved.retain(|a| !a.key.starts_with(prefix));
        board.active.len() != before
    };
    if gone {
        report(app, None);
    }
}

/// The ✕ on an alert in the island: off the list, until its tool sends it again.
#[tauri::command]
pub fn alerts_dismiss(app: AppHandle, key: String) {
    if BOARD.lock().unwrap().dismiss(&key) {
        report(&app, None);
    }
}

/// The script of a Zabbix media type of type Webhook; its first lines say how.
pub const ZABBIX_SCRIPT: &str = r#"// Ciel alerts for Zabbix 6 or later.
// Alerts → Media types → Create media type: type Webhook, this script, and
// these parameters (name = value):
//   url         = the address copied from Ciel (Settings → Integrations → Alerts)
//   event_id    = {EVENT.ID}
//   event_value = {EVENT.VALUE}
//   severity    = {EVENT.SEVERITY}
//   title       = {EVENT.NAME}
//   host        = {HOST.NAME}
//   message     = {ALERT.MESSAGE}
//   link        = your Zabbix's address, https://zabbix.example.com (optional)
// Then give a user this media (Send to: anything) and send to it from a
// trigger action, recovery operations included so fixed problems leave Ciel.
var p = JSON.parse(value);
var alert = {
    source: 'Zabbix',
    id: p.event_id,
    status: p.event_value === '0' ? 'resolved' : 'firing',
    severity: p.severity,
    title: p.title,
    host: p.host,
    message: p.message
};
if (p.link) {
    alert.url = p.link.replace(/\/+$/, '') + '/zabbix.php?action=problem.view';
}
var request = new HttpRequest();
request.addHeader('Content-Type: application/json');
request.post(p.url, JSON.stringify(alert));
if (request.getStatus() !== 200) {
    throw 'Ciel answered HTTP ' + request.getStatus();
}
return 'OK';
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_000.0;

    fn one(body: &str) -> Alert {
        let mut alerts = parse(body.as_bytes(), NOW).expect("an alert");
        assert_eq!(alerts.len(), 1);
        alerts.remove(0)
    }

    #[test]
    fn grafana_sends_a_group_of_alerts() {
        let body = r#"{
          "receiver": "ciel", "status": "firing", "orgId": 1, "title": "[FIRING:2] Disk",
          "alerts": [
            { "status": "firing", "labels": { "alertname": "DiskFull", "instance": "srv-files:9100", "severity": "critical" },
              "annotations": { "summary": "Disk almost full on srv-files", "description": "/var at 97%" },
              "fingerprint": "a1b2", "generatorURL": "https://grafana.example.com/alerting/grafana/x/view",
              "panelURL": "https://grafana.example.com/d/abc?viewPanel=3", "dashboardURL": "" },
            { "status": "resolved", "labels": { "alertname": "HighCPU", "instance": "srv-db" },
              "annotations": {}, "fingerprint": "c3d4", "valueString": "[ var='A' value=12 ]" }
          ]
        }"#;
        let alerts = parse(body.as_bytes(), NOW).unwrap();
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0], Alert {
            key: "grafana:a1b2".into(),
            source: "Grafana".into(),
            title: "Disk almost full on srv-files".into(),
            detail: "srv-files:9100 · /var at 97%".into(),
            severity: Severity::Critical,
            firing: true,
            url: Some("https://grafana.example.com/d/abc?viewPanel=3".into()),
            at: NOW,
        });
        assert_eq!(alerts[1].title, "HighCPU");
        assert_eq!(alerts[1].detail, "srv-db · [ var='A' value=12 ]");
        assert_eq!(alerts[1].severity, Severity::Warning);
        assert!(!alerts[1].firing);
    }

    #[test]
    fn alertmanager_is_told_from_grafana_and_keyed_by_labels_without_a_fingerprint() {
        let body = r#"{ "version": "4", "status": "firing", "receiver": "ciel",
          "commonLabels": { "severity": "page" },
          "alerts": [ { "labels": { "alertname": "InstanceDown", "instance": "10.0.0.5" },
                        "annotations": {}, "generatorURL": "http://prometheus:9090/graph?g0.expr=up" } ] }"#;
        let a = one(body);
        assert_eq!(a.source, "Alertmanager");
        assert_eq!(a.key, "alertmanager:alertname=InstanceDown,instance=10.0.0.5");
        assert_eq!(a.severity, Severity::Critical, "the group's severity when the alert has none");
        assert!(a.firing, "the group's status when the alert has none");
        assert_eq!(a.url.as_deref(), Some("http://prometheus:9090/graph?g0.expr=up"));
    }

    #[test]
    fn uptime_kuma_down_up_and_its_test_button() {
        let down = one(r#"{ "heartbeat": { "status": 0, "msg": "timeout of 48000ms exceeded" },
            "monitor": { "id": 7, "name": "Intranet", "url": "https://intranet.example.com", "hostname": null },
            "msg": "[Intranet] [🔴 Down] timeout" }"#);
        assert_eq!(down.key, "uptime kuma:7");
        assert_eq!(down.title, "Intranet");
        assert_eq!(down.detail, "https://intranet.example.com · timeout of 48000ms exceeded");
        assert_eq!((down.severity, down.firing), (Severity::Critical, true));

        let up = one(r#"{ "heartbeat": { "status": 1, "msg": "200 - OK" },
            "monitor": { "id": 7, "name": "Intranet", "url": "https://intranet.example.com" } }"#);
        assert_eq!((up.key.as_str(), up.firing), ("uptime kuma:7", false));

        // A ping monitor: a hostname, and "https://" as its URL, which goes nowhere.
        let ping = one(r#"{ "heartbeat": { "status": 2, "msg": "No response" },
            "monitor": { "id": 9, "name": "Printer 2F", "url": "https://", "hostname": "10.0.2.40" } }"#);
        assert_eq!((ping.detail.as_str(), ping.url.clone()), ("10.0.2.40 · No response", None));
        assert_eq!((ping.severity, ping.firing), (Severity::Warning, true));

        let test = one(r#"{ "heartbeat": null, "monitor": null, "msg": "Uptime Kuma Webhook Test" }"#);
        assert_eq!((test.title.as_str(), test.severity), ("Uptime Kuma Webhook Test", Severity::Info));
    }

    #[test]
    fn zabbix_through_its_script_problem_then_recovery() {
        let problem = one(r#"{ "source": "Zabbix", "id": "4012", "status": "firing", "severity": "High",
            "title": "Unavailable by ICMP ping", "host": "SW-CORE-01", "message": "Unavailable by ICMP ping",
            "url": "https://zabbix.example.com/zabbix.php?action=problem.view" }"#);
        assert_eq!(problem.key, "zabbix:4012");
        assert_eq!(problem.detail, "SW-CORE-01", "a message that repeats the title is left out");
        assert_eq!((problem.severity, problem.firing), (Severity::Critical, true));

        let recovery = one(r#"{ "source": "Zabbix", "id": "4012", "status": "resolved", "severity": "High",
            "title": "Unavailable by ICMP ping", "host": "SW-CORE-01" }"#);
        assert_eq!((recovery.key.as_str(), recovery.firing), ("zabbix:4012", false));
    }

    #[test]
    fn any_script_can_send_json_or_a_line_of_text() {
        let list = parse(br#"[ { "name": "Backup failed", "level": 4 }, { "nothing": "to show" } ]"#, NOW).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].source.as_str(), list[0].severity), ("Webhook", Severity::Critical));
        assert_eq!(list[0].key, "webhook:/Backup failed");

        let text = one("Disk full on srv1\n/var at 100%\n");
        assert_eq!((text.title.as_str(), text.detail.as_str()), ("Disk full on srv1", "/var at 100%"));
        assert!(text.firing);

        assert_eq!(parse(b"", NOW), None);
        assert_eq!(parse(b"  \n ", NOW), None);
        assert_eq!(parse(br#"{ "unrelated": true }"#, NOW), None);
        assert_eq!(parse(b"42", NOW), None);
    }

    #[test]
    fn statuses_and_severities_in_their_many_words() {
        for over in ["resolved", "OK", "up", "Recovery", "0", "false"] {
            assert!(!firing(Some(&json!(over))), "{over}");
        }
        assert!(!firing(Some(&json!(0))));
        assert!(!firing(Some(&json!(false))));
        for on in ["firing", "PROBLEM", "down", "1", "alerting"] {
            assert!(firing(Some(&json!(on))), "{on}");
        }
        assert!(firing(None));

        let read = |v: Value| Severity::read(Some(&v));
        assert_eq!(read(json!("Disaster")), Severity::Critical);
        assert_eq!(read(json!("Average")), Severity::Warning);
        assert_eq!(read(json!("Not classified")), Severity::Info);
        assert_eq!(read(json!(5)), Severity::Critical);
        assert_eq!(read(json!(1)), Severity::Info);
        assert_eq!(Severity::read(None), Severity::Warning);
    }

    #[test]
    fn only_web_pages_can_be_opened() {
        let url = |s: &str| web_url(Some(s.to_string()));
        assert_eq!(url("https://grafana.example.com/d/x"), Some("https://grafana.example.com/d/x".into()));
        assert_eq!(url("http://10.0.0.5:3001"), Some("http://10.0.0.5:3001".into()));
        for bad in ["file:///etc/passwd", "javascript:alert(1)", "https://", "https:///x", "https://a b", "ms-settings:"] {
            assert_eq!(url(bad), None, "{bad}");
        }
    }

    #[test]
    fn long_or_multiline_text_is_clipped_on_one_line() {
        assert_eq!(clip("a\n  b\tc", 10), "a b c");
        assert_eq!(clip("Réunion", 3), "Réu…");
    }

    fn alert(key: &str, severity: Severity, firing: bool, at: f64) -> Alert {
        Alert {
            key: key.into(),
            source: "Zabbix".into(),
            title: key.to_uppercase(),
            detail: String::new(),
            severity,
            firing,
            url: None,
            at,
        }
    }

    #[test]
    fn the_board_keeps_what_is_still_wrong() {
        let mut board = Board::default();
        let a = alert("a", Severity::Warning, true, 1.0);
        assert_eq!(board.apply(vec![a.clone()]), vec![Change::Fired(a.clone())]);

        // Sent again: no new notification, and it keeps its start.
        assert_eq!(board.apply(vec![alert("a", Severity::Warning, true, 5.0)]), vec![]);
        assert_eq!(board.data()["active"][0]["at"], json!(1.0));
        // Worse: told again.
        let worse = alert("a", Severity::Critical, true, 6.0);
        assert_eq!(board.apply(vec![worse.clone()]), vec![Change::Fired(worse)]);

        // Worst first, then newest.
        board.apply(vec![alert("b", Severity::Info, true, 7.0), alert("c", Severity::Critical, true, 8.0)]);
        let keys: Vec<_> = board.data()["active"].as_array().unwrap().iter().map(|a| a["key"].clone()).collect();
        assert_eq!(keys, [json!("c"), json!("a"), json!("b")]);

        // A recovery takes it off and keeps it among the recent ones.
        let changes = board.apply(vec![alert("a", Severity::Warning, false, 9.0)]);
        assert!(matches!(&changes[..], [Change::Resolved(done)] if done.key == "a" && !done.firing && done.at == 9.0));
        assert_eq!(board.data()["resolved"][0]["key"], json!("a"));
        // The end of something never seen: nothing.
        assert_eq!(board.apply(vec![alert("zzz", Severity::Warning, false, 10.0)]), vec![]);

        assert!(board.dismiss("b"));
        assert!(!board.dismiss("b"));
        assert_eq!(board.data()["active"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_board_never_grows_past_its_limits() {
        let mut board = Board::default();
        let many: Vec<Alert> = (0..MAX_ACTIVE + 10).map(|i| alert(&format!("k{i}"), Severity::Warning, true, i as f64)).collect();
        board.apply(many);
        assert_eq!(board.active.len(), MAX_ACTIVE);
        assert_eq!(board.active[0].key, format!("k{}", MAX_ACTIVE + 9), "the oldest went");
        let done: Vec<Alert> = (100..110).map(|i| alert(&format!("k{i}"), Severity::Warning, false, 0.0)).collect();
        board.apply(done);
        assert_eq!(board.resolved.len(), RECENT);
    }

    #[test]
    fn the_worst_new_problem_sets_the_badge() {
        let changes = vec![
            Change::Fired(alert("a", Severity::Warning, true, 0.0)),
            Change::Fired(alert("b", Severity::Critical, true, 0.0)),
            Change::Resolved(alert("c", Severity::Warning, false, 0.0)),
        ];
        let e = event(&changes).unwrap();
        assert_eq!((e.success, e.label.as_str()), (false, "B"));
        let e = event(&[Change::Resolved(alert("c", Severity::Warning, false, 0.0))]).unwrap();
        assert_eq!((e.success, e.label.as_str()), (true, "C"));
        assert!(event(&[]).is_none());
    }

    fn head(path: &str, headers: &[(&str, &str)]) -> Head {
        Head {
            method: "POST".into(),
            path: path.into(),
            headers: headers.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        }
    }

    #[test]
    fn only_requests_with_the_token_get_in() {
        let token = "s3cr3t";
        assert_eq!(authorize(&head("/alerts/s3cr3t", &[]), token), Ok(()));
        assert_eq!(authorize(&head("/alerts/s3cr3t/?from=grafana", &[]), token), Ok(()));
        assert_eq!(authorize(&head("/alerts", &[("authorization", "Bearer s3cr3t")]), token), Ok(()));
        assert_eq!(authorize(&head("/alerts/", &[("x-ciel-token", "s3cr3t")]), token), Ok(()));
        assert_eq!(authorize(&head("/alerts/wrong", &[]), token), Err(401));
        assert_eq!(authorize(&head("/alerts/s3cr3", &[]), token), Err(401));
        assert_eq!(authorize(&head("/alerts", &[]), token), Err(401));
        assert_eq!(authorize(&head("/alertss3cr3t", &[]), token), Err(404));
        assert_eq!(authorize(&head("/", &[]), token), Err(404));
        // No token yet: nothing gets in, not even an empty one.
        assert_eq!(authorize(&head("/alerts/", &[]), ""), Err(401));
        assert_eq!(authorize(&head("/alerts", &[("x-ciel-token", "")]), ""), Err(401));
    }

    #[test]
    fn the_head_is_read_with_lower_case_names() {
        let h = parse_head(b"POST /alerts/x HTTP/1.1\r\nHost: a\r\nContent-Length: 12").unwrap();
        assert_eq!((h.method.as_str(), h.path.as_str()), ("POST", "/alerts/x"));
        assert_eq!(h.header("content-length"), Some("12"));
        assert_eq!(parse_head(b"POST /alerts/x HTTP/2\r\n"), Err(505));
        assert_eq!(parse_head(b"garbage"), Err(400));
        assert_eq!(parse_head(b"POST / HTTP/1.1\r\nno colon"), Err(400));
    }

    #[tokio::test]
    async fn a_request_is_read_whole_and_curls_wait_is_answered() {
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let body = r#"{"title":"Disk full"}"#;
        let request = format!(
            "POST /alerts/tok HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nExpect: 100-continue\r\n\r\n",
            body.len()
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let reading = tokio::spawn(async move { read_request(&mut server, "tok").await });
        let mut cont = [0u8; 25];
        client.read_exact(&mut cont).await.unwrap();
        assert_eq!(&cont, b"HTTP/1.1 100 Continue\r\n\r\n");
        client.write_all(body.as_bytes()).await.unwrap();
        let (head, got) = reading.await.unwrap().unwrap();
        assert_eq!(head.method, "POST");
        assert_eq!(got, body.as_bytes());
    }

    #[tokio::test]
    async fn a_wrong_token_or_a_huge_body_is_refused_before_the_body_is_read() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_all(b"POST /alerts/nope HTTP/1.1\r\nContent-Length: 5\r\n\r\n").await.unwrap();
        assert_eq!(read_request(&mut server, "tok").await.unwrap_err(), 401);

        let (mut client, mut server) = tokio::io::duplex(1024);
        let big = format!("POST /alerts/tok HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1);
        client.write_all(big.as_bytes()).await.unwrap();
        assert_eq!(read_request(&mut server, "tok").await.unwrap_err(), 413);

        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_all(b"POST /alerts/tok HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
        assert_eq!(read_request(&mut server, "tok").await.unwrap_err(), 411);
    }

    #[test]
    fn ciel_listens_on_this_computer_unless_the_network_is_allowed() {
        let mut settings = Settings::default();
        assert_eq!(bind_address(&settings), "127.0.0.1:9797".parse().unwrap());
        settings.alerts_lan = true;
        settings.alerts_port = 8080;
        assert_eq!(bind_address(&settings), "0.0.0.0:8080".parse().unwrap());
        settings.alerts_port = 0;
        assert_eq!(bind_address(&settings).port(), DEFAULT_PORT);
        settings.alerts_lan = false;
        assert_eq!(address(&settings, "tok"), "http://127.0.0.1:9797/alerts/tok");
    }

    #[test]
    fn tokens_are_long_and_never_the_same() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "safe in a URL path");
    }
}
