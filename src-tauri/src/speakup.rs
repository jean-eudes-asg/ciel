// What Ciel says aloud without being asked (Ciel window → Chat → Speak up).
//
// Off until the user turns it on. Then four things are said, each with its
// own switch, and only while the user is at their desk (desk.rs):
//   * an agent waits for an answer (pipe.rs), unless a rule of theirs answers;
//   * an agent finished a turn that took a while (pipe.rs);
//   * a meeting of the Calendar pill is about to start (calendar/mod.rs);
//   * the day's brief, once, the first time they are there in the first hours
//     of their day: the brain is asked for it, as if they had asked.
//
// « OK Ciel » says them (voice/mod.rs), between two conversations. Without it
// listening, or with the user in a meeting, recording a call, dictating or
// presenting, nothing is said: the card in the island is all there is, as
// before. Away from their desk, the same lines but the brief are written to
// them on Discord instead, if they set that up (discord.rs). Ciel says that
// something waits; it answers nothing for the user, unless they turned on
// answering agents by voice and on Discord: then a permission request is a
// question, and their spoken yes or their reply with its code answers it
// (asked.rs).
//
// Nothing is watched for any of it. Each line is made when its event comes,
// and the brief is looked at once a minute during those first hours only.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::desk::{self, Hours};
use crate::i18n::{t, tf};
use crate::settings::SpeakUp;

/// What « OK Ciel » says next on its own.
#[derive(Debug, Clone, PartialEq)]
pub enum Say {
    /// A line, as it is.
    Line(String),
    /// The day's brief: the brain is asked for it (`brief_cue`).
    Brief,
    /// An agent's request, read as a question the user's next words answer
    /// (asked.rs).
    Ask { id: String, text: String },
    /// Something the user always allows, shown to the brain, which may ask
    /// them whether Ciel can allow it on its own (learn.rs).
    Learn { key: String },
}

struct Waiting {
    say: Say,
    until: Instant,
}

/// A line waits this long for a conversation to end; past it, it is old news.
const KEPT: Duration = Duration::from_secs(90);
/// The same line is not said twice within this.
const AGAIN: Duration = Duration::from_secs(120);
/// No more lines than this wait at once.
const MAX_WAITING: usize = 4;
/// A turn at least this long is worth saying it ended: the user has
/// probably looked away by then.
const WORTH_SAYING: Duration = Duration::from_secs(60);
/// A turn begun this long ago and never ended is forgotten.
const TURN_FORGOTTEN: Duration = Duration::from_secs(12 * 3600);
const MAX_TURNS: usize = 256;
/// The brief is for the first hours at the desk, in minutes.
const BRIEF_WITHIN: u32 = 4 * 60;
/// A brief the brain couldn't give is asked again this many times that day.
const BRIEF_TRIES: u8 = 3;
/// How often the brief is looked at: during the first hours, and otherwise.
const MINUTE: Duration = Duration::from_secs(60);
const REST: Duration = Duration::from_secs(600);

static QUEUE: Mutex<VecDeque<Waiting>> = Mutex::new(VecDeque::new());
static SAID: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());
static TURNS: LazyLock<Mutex<Turns>> = LazyLock::new(|| Mutex::new(Turns::default()));
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
static BRIEF: Mutex<Brief> = Mutex::new(Brief { said: None, due: None, tries: 0, asking: false });

fn prefs(app: &AppHandle) -> Option<SpeakUp> {
    app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().speak_up.clone())
}

/// How Ciel may tell the user something right now.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Way {
    /// Aloud: they are at their desk.
    Aloud,
    /// Written on Discord: they are away.
    Text,
    /// Not at all: the card in the island is enough.
    Quiet,
}

/// Away, a line is written if Discord is set up. At the desk it is said
/// aloud if that is turned on and nothing stands in the way: « OK Ciel »
/// listening to say it, no call recorded, no meeting under way, nothing
/// presented. A paused island tells nothing either way.
fn choose(paused: bool, there: bool, aloud: bool, in_the_way: bool, discord: bool) -> Way {
    match (paused, there) {
        (true, _) => Way::Quiet,
        (false, true) if aloud && !in_the_way => Way::Aloud,
        (false, false) if discord => Way::Text,
        _ => Way::Quiet,
    }
}

fn way(app: &AppHandle, prefs: &SpeakUp) -> Way {
    let in_the_way = || {
        !crate::voice::is_listening() || crate::notes::is_recording(app) || crate::calendar::under_way() || crate::platform::presenting()
    };
    let there = desk::there(prefs);
    choose(
        crate::integrations::PAUSED.load(Ordering::Relaxed),
        there,
        prefs.on,
        there && prefs.on && in_the_way(),
        !there && crate::discord::ready(prefs),
    )
}

/// Ciel may speak now.
fn may(app: &AppHandle, prefs: &SpeakUp) -> bool {
    prefs.on && way(app, prefs) == Way::Aloud
}

/// One of the four, told the way that fits: said, written, or left to its card.
fn tell(app: &AppHandle, prefs: &SpeakUp, what: &str, text: String) {
    match way(app, prefs) {
        Way::Aloud => line(what, text),
        Way::Text => {
            if fresh(&mut SAID.lock().unwrap(), &text, Instant::now()) {
                // What it is about, never what it says.
                crate::log::line(format!("speak up, on Discord: {what}"));
                crate::discord::text(text);
            }
        }
        Way::Quiet => {}
    }
}

// ── The lines ─────────────────────────────────────────────────────────────────

fn text<'a>(payload: &'a Value, key: &str) -> &'a str {
    payload.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The agent's name as its pill says it; none for Ciel's own brain, whose
/// requests the pill already speaks of.
fn agent(payload: &Value) -> Option<String> {
    Some(match text(payload, "ciel_agent") {
        "ciel" => return None,
        "" | "claude" => "Claude Code".into(),
        "claude-desktop" => "Claude Desktop".into(),
        id => crate::agents::Agent::from_id(id).map_or_else(|| t("Agent"), |a| a.name().to_string()),
    })
}

/// The project's name: the folder the session runs in.
fn project(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default().trim().to_string()
}

fn needs_line(agent: &str, project: &str) -> String {
    if project.is_empty() {
        tf("{agent} needs you.", &[("agent", agent)])
    } else {
        tf("{agent} needs you in {project}.", &[("agent", agent), ("project", project)])
    }
}

fn done_line(agent: &str, project: &str) -> String {
    if project.is_empty() {
        tf("{agent} has finished.", &[("agent", agent)])
    } else {
        tf("{agent} has finished in {project}.", &[("agent", agent), ("project", project)])
    }
}

/// `text` was not told a moment ago: it is, now.
fn fresh(said: &mut Vec<(String, Instant)>, text: &str, now: Instant) -> bool {
    said.retain(|(_, at)| now.duration_since(*at) < AGAIN);
    if said.iter().any(|(s, _)| s == text) {
        return false;
    }
    said.push((text.to_string(), now));
    true
}

/// `text` waits to be said, unless the same was said a moment ago or enough
/// wait already.
fn wait(queue: &mut VecDeque<Waiting>, said: &mut Vec<(String, Instant)>, text: String, now: Instant) -> bool {
    if queue.len() >= MAX_WAITING || !fresh(said, &text, now) {
        return false;
    }
    queue.push_back(Waiting { say: Say::Line(text), until: now + KEPT });
    true
}

/// The first thing waiting that is not old news.
fn take(queue: &mut VecDeque<Waiting>, now: Instant) -> Option<Say> {
    while let Some(waiting) = queue.pop_front() {
        if now <= waiting.until {
            return Some(waiting.say);
        }
    }
    None
}

fn line(what: &str, text: String) {
    if wait(&mut QUEUE.lock().unwrap(), &mut SAID.lock().unwrap(), text, Instant::now()) {
        // What it is about, never what it says.
        crate::log::line(format!("speak up: {what}"));
    }
}

// ── Agents ────────────────────────────────────────────────────────────────────

/// When each session's turn began.
#[derive(Default)]
struct Turns(HashMap<String, Instant>);

impl Turns {
    /// One hook event of the session `key`; true when it ends a turn long
    /// enough to be worth saying.
    fn event(&mut self, event: &str, key: &str, now: Instant) -> bool {
        match event {
            "UserPromptSubmit" => {
                if self.0.len() >= MAX_TURNS {
                    self.0.retain(|_, began| now.duration_since(*began) < TURN_FORGOTTEN);
                }
                if self.0.len() < MAX_TURNS {
                    self.0.insert(key.to_string(), now);
                }
                false
            }
            // An agent that doesn't say when it was asked: its first tool.
            "PreToolUse" | "PostToolUse" => {
                if self.0.len() < MAX_TURNS {
                    self.0.entry(key.to_string()).or_insert(now);
                }
                false
            }
            "Stop" => self.0.remove(key).is_some_and(|began| now.duration_since(began) >= WORTH_SAYING),
            "StopFailure" | "SessionEnd" | "Interrupt" => {
                self.0.remove(key);
                false
            }
            _ => false,
        }
    }
}

fn session_key(payload: &Value) -> String {
    match text(payload, "session_id") {
        "" | "unknown" => format!("{}+{}", text(payload, "ciel_agent"), text(payload, "cwd")),
        id => id.to_string(),
    }
}

/// Every hook event but a permission request comes by here (pipe.rs): a long
/// turn that ends is said.
pub fn agent_event(app: &AppHandle, event: &str, payload: &Value) {
    if !matches!(event, "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "Stop" | "StopFailure" | "SessionEnd" | "Interrupt") {
        return;
    }
    let long = TURNS.lock().unwrap().event(event, &session_key(payload), Instant::now());
    if !long {
        return;
    }
    let Some(prefs) = prefs(app).filter(|p| (p.on || p.discord) && p.done) else { return };
    let Some(agent) = agent(payload) else { return };
    tell(app, &prefs, "an agent finished", done_line(&agent, &project(text(payload, "cwd"))));
}

/// An agent asks for a permission or an answer (pipe.rs), once the rules had
/// their say: one that a rule of the user's answers needs no one.
pub fn needs(app: &AppHandle, payload: &Value) {
    if payload.get("ciel_rule").is_some() {
        return;
    }
    let Some(prefs) = prefs(app).filter(|p| (p.on || p.discord) && p.needs) else { return };
    let Some(agent) = agent(payload) else { return };
    let project = project(text(payload, "cwd"));
    // Asked, if the user turned that on and this request may be answered so.
    if let Some(what) = crate::asked::askable(payload).filter(|_| prefs.answer) {
        let id = text(payload, "request_id");
        let question = crate::asked::line(&agent, &project, &what);
        match way(app, &prefs) {
            Way::Aloud => {
                if crate::asked::open_aloud(id, &what) {
                    ask(id, question);
                }
            }
            Way::Text => {
                if let Some(code) = crate::asked::open_written(id, &what) {
                    crate::log::line("speak up, on Discord: an agent asks");
                    crate::discord::text(format!("{question} {}", crate::asked::reply_hint(&code)));
                }
            }
            Way::Quiet => {}
        }
        return;
    }
    tell(app, &prefs, "an agent needs the user", needs_line(&agent, &project));
}

/// Something the user always allows, to be asked about between two
/// conversations (learn.rs): only at the desk, with Ciel free to speak, and
/// one at a time.
pub fn propose(app: &AppHandle, key: String) {
    let Some(prefs) = prefs(app).filter(|p| p.on) else { return };
    if !may(app, &prefs) {
        return;
    }
    let mut queue = QUEUE.lock().unwrap();
    if queue.len() >= MAX_WAITING || queue.iter().any(|w| matches!(w.say, Say::Learn { .. })) {
        return;
    }
    queue.push_back(Waiting { say: Say::Learn { key }, until: Instant::now() + MINUTE });
}

/// A request read as a question, before the lines that wait: it waits on
/// the user, and lives only as long as the relay waits.
fn ask(id: &str, text: String) {
    let mut queue = QUEUE.lock().unwrap();
    if queue.len() >= MAX_WAITING {
        queue.pop_back();
    }
    queue.push_front(Waiting { say: Say::Ask { id: id.to_string(), text }, until: Instant::now() + KEPT });
    crate::log::line("speak up: an agent asks");
}

// ── Meetings ──────────────────────────────────────────────────────────────────

/// A meeting's card just showed (calendar/mod.rs): its title and when it starts.
pub fn meeting(app: &AppHandle, title: &str, start_ms: i64) {
    let Some(prefs) = prefs(app).filter(|p| (p.on || p.discord) && p.meeting) else { return };
    let time = DateTime::from_timestamp_millis(start_ms).map(|d| d.with_timezone(&Local).format("%H:%M").to_string()).unwrap_or_default();
    let title = if title.trim().is_empty() { t("Meeting") } else { title.trim().to_string() };
    tell(app, &prefs, "a meeting is about to start", tf("{meeting} starts at {time}.", &[("meeting", &title), ("time", &time)]));
}

// ── The day's brief ───────────────────────────────────────────────────────────

struct Brief {
    /// The day whose brief was said, or given up on.
    said: Option<NaiveDate>,
    /// The day of the brief that waits to be said.
    due: Option<NaiveDate>,
    /// How many times the brain couldn't give that one.
    tries: u8,
    /// The brain is being asked for it right now (voice/mod.rs).
    asking: bool,
}

/// A brief may be put in the queue: none waits there, and none is being asked
/// of the brain. The brief is looked at every minute and at each save of the
/// settings, and either can fall while the brain still answers: the day is
/// not said yet and the queue is empty, and a second one would follow the first.
fn queues(brief: &Brief, waits: bool) -> bool {
    !waits && !brief.asking
}

/// The brief that waited is asked of the brain now: not one whose day was
/// said meanwhile, nor a second while the first is asked.
fn begin(brief: &mut Brief) -> bool {
    if brief.asking || brief.due.is_none() || brief.due == brief.said {
        return false;
    }
    brief.asking = true;
    true
}

/// The day a brief is due for at `now`: in the first hours of a span at the
/// desk, the first of the day the user is there for, and not said for that
/// day. Hours past midnight are still the day they began.
fn brief_day(hours: &Hours, now: NaiveDateTime, said: Option<NaiveDate>) -> Option<NaiveDate> {
    let since = hours.since_start(now).filter(|m| *m < BRIEF_WITHIN)?;
    let day = (now - chrono::Duration::minutes(since as i64)).date();
    (said != Some(day)).then_some(day)
}

/// The brief of `due` was asked of the brain: said, or not, and whether to
/// stop asking for that day.
fn after_brief(brief: &mut Brief, spoken: bool) -> Option<NaiveDate> {
    brief.asking = false;
    let day = brief.due.take()?;
    brief.tries = if spoken { 0 } else { brief.tries + 1 };
    if spoken || brief.tries >= BRIEF_TRIES {
        brief.tries = 0;
        brief.said = Some(day);
        return Some(day);
    }
    None
}

/// Where the day of the last brief is kept, so that Ciel started again the
/// same morning doesn't say it twice.
fn said_file() -> PathBuf {
    crate::settings::local_dir().join("brief-said.txt")
}

/// What the brain is asked for the brief, in place of the user's words.
pub fn brief_cue() -> String {
    format!(
        "(This is Ciel's own cue, not the user's words: they just sat down at their desk for the day.) Give them their brief for \
today, aloud, in three or four short sentences: what is most pressing on their list, where the projects they last worked in stand, \
and today's meetings if you can read them. If there is nothing to tell, a short good morning is enough. Do not end with a question. \
Speak in the language whose code is {}.",
        crate::i18n::current()
    )
}

/// « OK Ciel » asked the brain for the brief (voice/mod.rs): `spoken` when it
/// gave one.
pub fn brief_done(spoken: bool) {
    let day = after_brief(&mut BRIEF.lock().unwrap(), spoken);
    if let Some(day) = day {
        let _ = std::fs::write(said_file(), day.format("%Y-%m-%d").to_string());
    }
}

/// One look at the brief, and how long until the next.
fn look(app: &AppHandle) -> Duration {
    // Off: until the settings are saved again.
    let Some(prefs) = prefs(app).filter(|p| p.on && p.brief) else { return Duration::from_secs(3600) };
    let hours = Hours::of(&prefs);
    let now = Local::now().naive_local();
    let said = BRIEF.lock().unwrap().said;
    let Some(day) = brief_day(&hours, now, said) else {
        // Until today's hours begin, if they haven't, ten minutes at most: a
        // computer that slept, or a clock that changed, is caught up with.
        return hours.until_start(now).map_or(REST, |m| Duration::from_secs(m as u64 * 60 + 2).min(REST));
    };
    if may(app, &prefs) {
        let mut queue = QUEUE.lock().unwrap();
        let mut brief = BRIEF.lock().unwrap();
        if queues(&brief, queue.iter().any(|w| w.say == Say::Brief)) {
            brief.due = Some(day);
            queue.push_back(Waiting { say: Say::Brief, until: Instant::now() + MINUTE });
        }
    }
    MINUTE
}

pub fn start(app: AppHandle) {
    let said = std::fs::read_to_string(said_file()).ok().and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok());
    BRIEF.lock().unwrap().said = said;
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            let wait = look(&app);
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = WAKE.notified() => {}
            }
        }
    });
}

/// The settings were saved: the brief is looked at again with them.
pub fn settings_saved() {
    WAKE.notify_one();
}

// ── For « OK Ciel » ───────────────────────────────────────────────────────────

/// Between two conversations (voice/mod.rs): what to say now, if it still
/// stands. Asked several times a second, so nothing is read while nothing
/// waits.
pub fn next(app: &AppHandle) -> Option<Say> {
    let say = {
        let mut queue = QUEUE.lock().unwrap();
        loop {
            if queue.is_empty() {
                return None;
            }
            match take(&mut queue, Instant::now())? {
                // Answered in the island, or gone back to the terminal, meanwhile.
                Say::Ask { id, .. } if !crate::asked::waits(&id) => continue,
                say => break say,
            }
        }
    };
    // The user may have left, or a call begun, while it waited.
    let prefs = prefs(app)?;
    if !may(app, &prefs) {
        if say == Say::Brief {
            BRIEF.lock().unwrap().due = None;
        }
        return None;
    }
    if say == Say::Brief && !begin(&mut BRIEF.lock().unwrap()) {
        return None;
    }
    Some(say)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hours(days: &[u8], from: &str, to: &str) -> Hours {
        Hours::of(&SpeakUp { days: days.to_vec(), from: from.into(), to: to.into(), ..SpeakUp::default() })
    }

    fn at(day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        // November 2026: the 9th is a Monday.
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap().and_hms_opt(hour, minute, 0).unwrap()
    }

    #[test]
    fn nothing_is_said_until_the_user_turns_it_on() {
        let prefs = SpeakUp::default();
        assert!(!prefs.on);
        // Turned on, the four are said, on weekdays from nine to six.
        assert!(prefs.needs && prefs.done && prefs.meeting && prefs.brief);
        assert_eq!((prefs.days.as_slice(), prefs.from.as_str(), prefs.to.as_str()), (&[0, 1, 2, 3, 4][..], "09:00", "18:00"));
    }

    #[test]
    fn a_line_is_said_at_the_desk_written_when_away_and_otherwise_left_to_its_card() {
        // At the desk: aloud, unless something stands in the way; never written.
        assert_eq!(choose(false, true, true, false, false), Way::Aloud);
        assert_eq!(choose(false, true, true, true, false), Way::Quiet, "a meeting, a call, « OK Ciel » off");
        assert_eq!(choose(false, true, false, false, true), Way::Quiet, "speaking up is off, and they are there");
        // Away: written if Discord is set up; never aloud to an empty room.
        assert_eq!(choose(false, false, true, false, true), Way::Text);
        assert_eq!(choose(false, false, false, false, true), Way::Text, "Discord alone");
        assert_eq!(choose(false, false, true, false, false), Way::Quiet);
        // A paused island tells nothing.
        assert_eq!(choose(true, true, true, false, true), Way::Quiet);
        assert_eq!(choose(true, false, true, false, true), Way::Quiet);
    }

    #[test]
    fn a_line_names_the_agent_and_the_project_never_what_was_asked() {
        let claude = json!({ "cwd": "D:\\work\\ciel\\", "tool_input": { "command": "rm -rf build" } });
        assert_eq!(needs_line(&agent(&claude).unwrap(), &project(text(&claude, "cwd"))), "Claude Code needs you in ciel.");
        let codex = json!({ "ciel_agent": "codex", "cwd": "/home/me/site" });
        assert_eq!(done_line(&agent(&codex).unwrap(), &project(text(&codex, "cwd"))), "Codex has finished in site.");
        assert_eq!(agent(&json!({ "ciel_agent": "claude-desktop" })).as_deref(), Some("Claude Desktop"));
        assert_eq!(agent(&json!({ "ciel_agent": "someone-new" })).as_deref(), Some("Agent"));
        // No folder said: the agent alone.
        assert_eq!(needs_line("Codex", &project("")), "Codex needs you.");
        assert_eq!(done_line("Codex", &project("/")), "Codex has finished.");
        // Ciel's own brain: its pill already says an OK is needed.
        assert_eq!(agent(&json!({ "ciel_agent": "ciel" })), None);
    }

    #[test]
    fn only_a_turn_that_took_a_while_is_said_to_have_ended() {
        let start = Instant::now();
        let secs = |s: u64| start + Duration::from_secs(s);
        let mut turns = Turns::default();
        // A quick answer: the user is still looking at it.
        assert!(!turns.event("UserPromptSubmit", "a", secs(0)));
        assert!(!turns.event("Stop", "a", secs(20)));
        // A long one.
        turns.event("UserPromptSubmit", "a", secs(100));
        turns.event("PreToolUse", "a", secs(150));
        assert!(turns.event("Stop", "a", secs(400)));
        // Said once: a second Stop finds no turn.
        assert!(!turns.event("Stop", "a", secs(401)));
        // An agent that only says its tools: from the first one.
        turns.event("PreToolUse", "b", secs(0));
        turns.event("PostToolUse", "b", secs(50));
        assert!(turns.event("Stop", "b", secs(61)));
        // Stopped by the user, or failed: nothing to say.
        turns.event("UserPromptSubmit", "c", secs(0));
        turns.event("Interrupt", "c", secs(300));
        assert!(!turns.event("Stop", "c", secs(301)));
        // A session with no id is told apart by its agent and folder.
        assert_eq!(session_key(&json!({ "session_id": "s1", "cwd": "/w" })), "s1");
        assert_eq!(session_key(&json!({ "ciel_agent": "codex", "cwd": "/w" })), "codex+/w");
    }

    #[test]
    fn a_line_is_said_once_and_not_as_old_news() {
        let start = Instant::now();
        let (mut queue, mut said) = (VecDeque::new(), Vec::new());
        assert!(wait(&mut queue, &mut said, "Codex needs you in site.".into(), start));
        // The same again a moment later: once is enough.
        assert!(!wait(&mut queue, &mut said, "Codex needs you in site.".into(), start + Duration::from_secs(30)));
        assert_eq!(take(&mut queue, start + Duration::from_secs(5)), Some(Say::Line("Codex needs you in site.".into())));
        assert_eq!(take(&mut queue, start + Duration::from_secs(5)), None);
        // Later it may be said again; but one that waited through a long
        // conversation is dropped, and the next one is said.
        let later = start + AGAIN + Duration::from_secs(1);
        assert!(wait(&mut queue, &mut said, "Codex needs you in site.".into(), later));
        assert!(wait(&mut queue, &mut said, "Codex has finished in site.".into(), later + KEPT));
        assert_eq!(take(&mut queue, later + KEPT + Duration::from_secs(1)), Some(Say::Line("Codex has finished in site.".into())));
        // No more than a few wait at once.
        for i in 0..10 {
            wait(&mut queue, &mut said, format!("line {i}"), later);
        }
        assert_eq!(queue.len(), MAX_WAITING);
    }

    #[test]
    fn the_brief_is_for_the_first_hours_at_the_desk_once_a_day() {
        let week = hours(&[0, 1, 2, 3, 4], "09:00", "18:00");
        let monday = at(9, 0, 0).date();
        assert_eq!(brief_day(&week, at(9, 8, 59), None), None);
        assert_eq!(brief_day(&week, at(9, 9, 0), None), Some(monday));
        assert_eq!(brief_day(&week, at(9, 12, 59), None), Some(monday));
        // Sitting down in the afternoon: no morning brief.
        assert_eq!(brief_day(&week, at(9, 13, 0), None), None);
        // Said today: not again; yesterday's doesn't count.
        assert_eq!(brief_day(&week, at(9, 9, 30), Some(monday)), None);
        assert_eq!(brief_day(&week, at(10, 9, 30), Some(monday)), Some(at(10, 0, 0).date()));
        assert_eq!(brief_day(&week, at(14, 9, 30), None), None, "Saturday");
        // Hours that begin late and run past midnight: one brief, for the day they began.
        let night = hours(&[0], "22:00", "06:00");
        assert_eq!(brief_day(&night, at(9, 23, 0), None), Some(monday));
        assert_eq!(brief_day(&night, at(10, 1, 0), None), Some(monday));
        assert_eq!(brief_day(&night, at(10, 1, 0), Some(monday)), None);
    }

    #[test]
    fn a_brief_the_brain_couldnt_give_is_asked_again_a_few_times_only() {
        let monday = at(9, 0, 0).date();
        let mut brief = Brief { said: None, due: Some(monday), tries: 0, asking: false };
        assert_eq!(after_brief(&mut brief, true), Some(monday));
        assert_eq!((brief.said, brief.due), (Some(monday), None));
        // Nothing was due: nothing changes.
        assert_eq!(after_brief(&mut brief, true), None);
        let tuesday = at(10, 0, 0).date();
        for tried in 1..BRIEF_TRIES {
            brief.due = Some(tuesday);
            assert_eq!(after_brief(&mut brief, false), None, "try {tried}");
            assert_eq!(brief.said, Some(monday));
        }
        brief.due = Some(tuesday);
        assert_eq!(after_brief(&mut brief, false), Some(tuesday), "given up for the day");
        assert!(brief_cue().contains("code is en"));
    }

    #[test]
    fn a_brief_is_said_once_even_when_it_is_looked_at_while_the_brain_answers() {
        let monday = at(9, 0, 0).date();
        let mut brief = Brief { said: None, due: Some(monday), tries: 0, asking: false };
        // It waits in the queue: another look adds nothing.
        assert!(!queues(&brief, true));
        // The voice takes it and asks the brain.
        assert!(begin(&mut brief));
        // The settings are saved, or the minute comes, while the brain answers:
        // the queue is empty and the day not said yet, and still none is added.
        assert!(!queues(&brief, false));
        assert!(!begin(&mut brief), "nor a second one asked");
        assert_eq!(after_brief(&mut brief, true), Some(monday));
        assert!(!brief.asking);
        // One that was in the queue all the same is dropped once its day is said.
        brief.due = Some(monday);
        assert!(!begin(&mut brief));
        // The brain couldn't give it: it may be asked again.
        let tuesday = at(10, 0, 0).date();
        brief.due = Some(tuesday);
        assert!(begin(&mut brief));
        assert_eq!(after_brief(&mut brief, false), None);
        assert!(queues(&brief, false));
        // Nothing waits: nothing to begin.
        assert!(!begin(&mut brief));
    }
}
