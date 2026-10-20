// The Calendar pill, `integration_calendar`: today's meetings and the next
// days', from the secret iCal links of the user's calendars (Google, Outlook,
// iCloud, Fastmail…), and a card in the island a few minutes before each
// meeting, with Join (its Meet, Teams or Zoom room) and Take notes (the
// Notetaker, told the meeting's title, who is invited and its agenda).
//
// A secret iCal link is read-only and needs no sign-in. It is a secret all
// the same: it lives in the keychain like a key (secrets.rs), never goes in
// the log, and Ciel asks nothing but that address. The meetings stay in
// memory, never on disk, and nothing is fetched while the pill is off or Ciel
// is paused. The loop wakes once a minute at most, and when a card is due.

pub mod ics;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use chrono::{Local, LocalResult, NaiveDateTime, NaiveTime, TimeZone};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::i18n::{t, tf};
use crate::integrations::{self, IntegrationUpdate};
use crate::notify::{self, Button, Notification, Span, Target};
use crate::{log, secrets};
use ics::{Feed, Occurrence};

pub const PILL_ID: &str = "integration_calendar";
/// Up to three calendars, a link each.
pub const LINK_KEYS: [&str; 3] = ["calendar-ics-1", "calendar-ics-2", "calendar-ics-3"];

/// How often the links are read again.
const EVERY_MS: i64 = 10 * 60_000;
const TIMEOUT: Duration = Duration::from_secs(30);
/// A calendar with years behind it is a few megabytes; past this it is refused.
const MAX_FEED: usize = 25 * 1024 * 1024;
/// What is read: today and the six days after.
const DAYS: i64 = 7;
/// The most meetings the island is sent.
const MAX_SHOWN: usize = 60;
/// A meeting's card stays this long after it began…
const CARD_STAYS_MS: i64 = 5 * 60_000;
/// …and is still shown when Ciel learns of the meeting this long after its start.
const LATE_MS: i64 = 2 * 60_000;

struct Known {
    /// The meetings read last, from every calendar, in order.
    events: Vec<Occurrence>,
    /// The meetings whose card was shown.
    told: HashSet<String>,
}

static KNOWN: LazyLock<Mutex<Known>> = LazyLock::new(|| Mutex::new(Known { events: Vec::new(), told: HashSet::new() }));
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// The pill was on at the last save of the settings.
static ON: AtomicBool = AtomicBool::new(false);

/// One occurrence of one meeting.
fn key(o: &Occurrence) -> String {
    format!("{}@{}", o.uid, o.start)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A wall-clock time of the computer as an instant (Unix ms). In the hour
/// the clocks go back, the first one; in the hour they skip, an hour later.
fn local_ms(at: NaiveDateTime) -> i64 {
    match Local.from_local_datetime(&at) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.timestamp_millis(),
        LocalResult::None => Local
            .from_local_datetime(&(at + chrono::Duration::hours(1)))
            .earliest()
            .map_or_else(|| at.and_utc().timestamp_millis(), |t| t.timestamp_millis()),
    }
}

/// The keychain key of one of the calendars' links.
pub fn is_link_key(key: &str) -> bool {
    LINK_KEYS.contains(&key)
}

/// A link changed, or the island's Refresh: the links are read at once.
pub fn wake() {
    WAKE.notify_one();
}

/// The settings were saved: the pill just turned on reads its links at once.
pub fn settings_saved(active: &[String]) {
    let on = active.iter().any(|id| id == PILL_ID);
    if ON.swap(on, Ordering::Relaxed) != on {
        wake();
    }
}

/// How long before a meeting its card shows (Settings); 0 when it doesn't.
fn lead_ms(app: &AppHandle) -> i64 {
    app.try_state::<crate::Shared>()
        .map(|s| s.settings.lock().unwrap().meeting_reminder_minutes as i64 * 60_000)
        .unwrap_or(0)
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(4)).await;
        let mut read_at: Option<i64> = None;
        let mut asked = true;
        loop {
            if integrations::PAUSED.load(Ordering::Relaxed) {
                // Paused means no network and no card; what was read stays.
            } else if !integrations::enabled(&app, PILL_ID) {
                let mut known = KNOWN.lock().unwrap();
                known.events.clear();
                known.told.clear();
                read_at = None;
            } else {
                let now = now_ms();
                // `now < at`: the clock was put back.
                if asked || read_at.is_none_or(|at| now - at >= EVERY_MS || now < at) {
                    read_at = Some(now);
                    poll(&app).await;
                }
                remind(&app);
            }
            asked = tokio::select! {
                _ = tokio::time::sleep(next_wait(&app)) => false,
                _ = WAKE.notified() => true,
            };
        }
    });
}

/// Until the next card is due, a minute at most: a computer that slept
/// catches up within the minute it wakes.
fn next_wait(app: &AppHandle) -> Duration {
    let lead = lead_ms(app);
    let now = now_ms();
    let due = if lead == 0 {
        None
    } else {
        let known = KNOWN.lock().unwrap();
        known.events.iter().filter(|o| !o.all_day && !known.told.contains(&key(o))).map(|o| o.start - lead).filter(|&at| at > now).min()
    };
    Duration::from_millis(due.map_or(60_000, |at| (at - now).clamp(500, 60_000)) as u64)
}

// ── Reading the links ─────────────────────────────────────────────────────────

/// The address to read: webcal:// is https. None for anything else.
fn address(link: &str) -> Option<String> {
    let link = link.trim();
    let scheme = |s: &str| link.len() > s.len() && link[..s.len()].eq_ignore_ascii_case(s);
    if scheme("https://") {
        Some(link.to_string())
    } else if scheme("webcal://") {
        Some(format!("https://{}", &link["webcal://".len()..]))
    } else {
        None
    }
}

/// One calendar's meetings in [from, to). The errors never hold the
/// address, which is a secret.
async fn read(http: &reqwest::Client, link: &str, from: i64, to: i64) -> Result<Vec<Occurrence>, String> {
    let url = address(link).ok_or_else(|| t("A calendar link starts with https:// or webcal://"))?;
    let unreachable = |e: reqwest::Error| if e.is_timeout() { t("The calendar took too long to answer") } else { t("Calendar unreachable") };
    let response = http.get(&url).header("Accept", "text/calendar, */*").send().await.map_err(unreachable)?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 | 404 | 410 => t("This calendar link no longer works"),
            code => tf("The calendar answered {code}", &[("code", &code.to_string())]),
        });
    }
    if response.content_length().is_some_and(|n| n as usize > MAX_FEED) {
        return Err(t("This calendar is too big to read"));
    }
    let bytes = response.bytes().await.map_err(unreachable)?;
    if bytes.len() > MAX_FEED {
        return Err(t("This calendar is too big to read"));
    }
    // A big calendar takes a moment to read: off the async threads.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    tauri::async_runtime::spawn_blocking(move || Feed::parse(&text).map(|feed| feed.between(from, to, &local_ms)))
        .await
        .ok()
        .flatten()
        .ok_or_else(|| t("This link isn't an iCal calendar"))
}

async fn poll(app: &AppHandle) {
    let links: Vec<(usize, String)> = LINK_KEYS.iter().enumerate().filter_map(|(i, k)| secrets::get(k).map(|v| (i + 1, v))).collect();
    let today = Local::now().date_naive();
    let from = local_ms(today.and_time(NaiveTime::MIN));
    let to = local_ms((today + chrono::Duration::days(DAYS)).and_time(NaiveTime::MIN));
    let http = reqwest::Client::builder().timeout(TIMEOUT).https_only(true).build().unwrap_or_default();

    let mut events: Vec<Occurrence> = Vec::new();
    let mut problems: Vec<(usize, String)> = Vec::new();
    for (n, link) in &links {
        match read(&http, link, from, to).await {
            Ok(found) => events.extend(found.into_iter().filter(|o| !o.declined)),
            Err(err) => {
                log::line(format!("calendar {n}: {err}"));
                problems.push((*n, err));
            }
        }
    }
    // A meeting in two of the calendars shows once.
    let mut seen = HashSet::new();
    events.retain(|o| seen.insert(key(o)));
    events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));

    if !links.is_empty() && problems.len() == links.len() {
        // Nothing could be read: the island keeps what it had, with why.
        integrations::emit(app, IntegrationUpdate { id: PILL_ID, data: json!({}), error: Some(problems[0].1.clone()), event: None });
        return;
    }
    let shown: Vec<Value> = events.iter().take(MAX_SHOWN).map(event_json).collect();
    {
        let mut known = KNOWN.lock().unwrap();
        let keys: HashSet<String> = events.iter().map(key).collect();
        known.told.retain(|k| keys.contains(k));
        known.events = events;
    }
    let problems: Vec<Value> = problems.into_iter().map(|(n, error)| json!({ "calendar": n, "error": error })).collect();
    integrations::emit(app, IntegrationUpdate {
        id: PILL_ID,
        data: json!({ "events": shown, "calendars": links.len(), "problems": problems }),
        error: None,
        event: None,
    });
}

/// What the island is told of a meeting: no one's name, no description.
fn event_json(o: &Occurrence) -> Value {
    json!({
        "id": key(o),
        "title": o.title,
        "start": o.start,
        "end": o.end,
        "allDay": o.all_day,
        "join": o.join,
        // Zoom and Teams put their address there: Join says it already.
        "where": o.location.as_deref().filter(|l| !l.contains("://")),
    })
}

// ── The card before a meeting ─────────────────────────────────────────────────

/// "Marie Dupont, Paul Martin +2".
fn with_whom(names: &[String]) -> Option<String> {
    let names: Vec<&str> = names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).collect();
    match names.len() {
        0 => None,
        1 | 2 => Some(names.join(", ")),
        n => Some(format!("{}, {} +{}", names[0], names[1], n - 2)),
    }
}

/// The meetings whose time has come show their card, once each.
fn remind(app: &AppHandle) {
    let lead = lead_ms(app);
    if lead == 0 {
        return;
    }
    let now = now_ms();
    let due: Vec<Occurrence> = {
        let mut known = KNOWN.lock().unwrap();
        let due: Vec<Occurrence> = known
            .events
            .iter()
            .filter(|o| !o.all_day && o.start - lead <= now && now <= o.start + LATE_MS && !known.told.contains(&key(o)))
            .cloned()
            .collect();
        for o in &due {
            known.told.insert(key(o));
        }
        due
    };
    for o in &due {
        card(app, o);
    }
}

fn card(app: &AppHandle, o: &Occurrence) {
    let names: Vec<String> = o.attendees.iter().map(|a| a.name.clone()).filter(|n| !n.is_empty()).collect();
    // The last button is the island's white one.
    let mut buttons = vec![Button { id: "notes".into(), label: t("Take notes") }];
    if o.join.is_some() {
        buttons.push(Button { id: "join".into(), label: t("Join") });
    }
    let n = Notification {
        id: notify::next_id("calendar"),
        source: "ciel".into(),
        // The pill's name, which stays as it is, as Finance's does.
        app: "Calendar".into(),
        title: if o.title.is_empty() { t("Meeting") } else { o.title.clone() },
        body: with_whom(&names).or_else(|| o.location.clone().filter(|l| !l.contains("://"))).unwrap_or_default(),
        icon: None,
        at: now_ms() as f64,
        tag: Some(format!("meeting-{}", key(o))),
        buttons,
        until: Some((o.start + CARD_STAYS_MS) as f64),
        time: Some(Span { start: o.start as f64, end: o.end as f64 }),
    };
    let meeting = crate::notes::Meeting {
        title: o.title.clone(),
        attendees: names,
        agenda: o.description.as_deref().and_then(ics::agenda),
    };
    notify::show(app, n, Target::Meeting { join: o.join.clone(), meeting });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webcal_links_are_read_over_https_and_nothing_else_is_read() {
        assert_eq!(address(" webcal://p01-caldav.icloud.com/published/2/abc ").as_deref(), Some("https://p01-caldav.icloud.com/published/2/abc"));
        assert_eq!(address("HTTPS://calendar.google.com/calendar/ical/x/basic.ics").as_deref(), Some("HTTPS://calendar.google.com/calendar/ical/x/basic.ics"));
        assert_eq!(address("http://example.com/cal.ics"), None);
        assert_eq!(address("file:///etc/passwd"), None);
        assert_eq!(address("https://"), None);
    }

    #[test]
    fn who_comes_is_said_in_a_few_words() {
        let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(with_whom(&[]), None);
        assert_eq!(with_whom(&names(&["Ana"])).as_deref(), Some("Ana"));
        assert_eq!(with_whom(&names(&["Ana", " Paul "])).as_deref(), Some("Ana, Paul"));
        assert_eq!(with_whom(&names(&["Ana", "Paul", "Marie", "Li"])).as_deref(), Some("Ana, Paul +2"));
    }

    #[test]
    fn the_island_is_told_no_ones_name_nor_the_description() {
        let o = Occurrence {
            uid: "u".into(),
            title: "Q4".into(),
            start: 1,
            end: 2,
            all_day: false,
            location: Some("https://zoom.us/j/1".into()),
            description: Some("secret plans".into()),
            join: Some("https://zoom.us/j/1".into()),
            attendees: vec![ics::Attendee { name: "Ana".into(), email: Some("ana@x.io".into()), declined: false, resource: false }],
            declined: false,
        };
        let v = event_json(&o);
        assert_eq!(v["where"], Value::Null);
        assert_eq!(v["join"], "https://zoom.us/j/1");
        let text = v.to_string();
        assert!(!text.contains("Ana") && !text.contains("secret"));
    }
}
