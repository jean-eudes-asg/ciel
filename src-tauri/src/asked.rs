// An agent's permission request answered without a click on its card, once
// the user turned that on (Ciel window → Chat → Speak up → Answer agents by
// voice and on Discord, off by default):
//
//   * At the desk, Ciel reads the request aloud and asks (speakup.rs). The
//     user's next words go to the brain as usual, told what was asked, and
//     its `permission` tool allows or denies it. Only that request, only once
//     Ciel read it to them, only in a turn they spoke aloud: never from a
//     Discord reply, never in Ciel's own brief.
//   * Away, the request is written to them on Discord with a code of four
//     digits (discord.rs), and only their reply « allow 4821 » or
//     « deny 4821 » answers it. That reply is read here, by Ciel's code; the
//     brain never sees the code and can't answer for them.
//
// Never « Don't ask again » (that stays a click, rules.rs), never a request
// of Ciel's own brain, never a question an agent asks: those wait for the
// card. A request lives as long as the relay waits for it (pipe.rs); an
// answer that comes later answers nothing. Each answer leaves a line on the
// session in the island, and the log says by what, never what was asked.

use std::hash::{BuildHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::i18n::{t, tf};

/// No more requests than this are held at once.
const MAX_OPEN: usize = 8;
/// A little longer than the relay waits (pipe.rs): past it, nothing waits.
const LIFE: Duration = Duration::from_secs(120);
/// What is asked, at most, when it is said or written.
const WHAT_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq)]
struct Open {
    id: String,
    /// What is asked, in a few words: « run npm test ».
    what: String,
    /// The turn of « OK Ciel » it was read aloud in (voice/mod.rs `turn`).
    read: Option<u64>,
    /// The code written with it on Discord.
    code: Option<String>,
    at: Instant,
}

static OPEN: Mutex<Vec<Open>> = Mutex::new(Vec::new());
/// The brain answers words the user spoke aloud right now.
static BY_VOICE: AtomicBool = AtomicBool::new(false);
/// The brain answers what the user wrote on Discord right now.
static BY_DISCORD: AtomicBool = AtomicBool::new(false);

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).chain("…".chars()).collect()
}

fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?.trim();
    (!host.is_empty()).then(|| host.to_string())
}

/// What the request asks for, in a few words; none for one that is never
/// answered this way, whose card waits for a click.
pub fn askable(payload: &Value) -> Option<String> {
    let text = |key: &str| payload.get(key).and_then(Value::as_str).unwrap_or_default();
    if text("ciel_agent") == "ciel" {
        return None;
    }
    let tool = text("tool_name");
    if tool.is_empty() || matches!(tool, "AskUserQuestion" | "ExitPlanMode") {
        return None;
    }
    let input = &payload["tool_input"];
    let field = |key: &str| input.get(key).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    Some(match tool {
        "Bash" | "PowerShell" => {
            let command = field("command")?.lines().next().unwrap_or_default();
            tf("run {command}", &[("command", &clip(command, WHAT_CHARS))])
        }
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            let path = field("file_path").or_else(|| field("notebook_path"))?;
            let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
            tf("edit {file}", &[("file", &clip(file, WHAT_CHARS))])
        }
        "WebFetch" => tf("open {site}", &[("site", &clip(&host(field("url")?)?, WHAT_CHARS))]),
        other => tf("use {tool}", &[("tool", &clip(other.rsplit("__").next().unwrap_or(other), WHAT_CHARS))]),
    })
}

/// The question, as it is said or written.
pub fn line(agent: &str, project: &str, what: &str) -> String {
    if project.is_empty() {
        tf("{agent} asks: {what}. Allow it?", &[("agent", agent), ("what", what)])
    } else {
        tf("{agent} asks, in {project}: {what}. Allow it?", &[("agent", agent), ("project", project), ("what", what)])
    }
}

fn words() -> (String, String) {
    (t("Allow").to_lowercase(), t("Deny").to_lowercase())
}

/// How to answer from Discord, after the question.
pub fn reply_hint(code: &str) -> String {
    let (allow, deny) = words();
    tf("Reply « {allow} {code} » or « {deny} {code} » within a minute and a half.", &[("allow", &allow), ("deny", &deny), ("code", code)])
}

fn hold(open: Open) -> bool {
    let mut list = OPEN.lock().unwrap();
    list.retain(|o| o.at.elapsed() < LIFE);
    if list.len() >= MAX_OPEN || list.iter().any(|o| o.id == open.id) {
        return false;
    }
    list.push(open);
    true
}

/// A request that will be read aloud: held until answered or gone.
pub fn open_aloud(id: &str, what: &str) -> bool {
    !id.is_empty() && hold(Open { id: id.into(), what: what.into(), read: None, code: None, at: Instant::now() })
}

/// Four digits no other request holds.
fn new_code(id: &str, taken: &[Open]) -> String {
    let seed = std::collections::hash_map::RandomState::new();
    for attempt in 0u32.. {
        let mut hasher = seed.build_hasher();
        (id, attempt, std::time::SystemTime::now()).hash(&mut hasher);
        let code = format!("{}", 1000 + hasher.finish() % 9000);
        if !taken.iter().any(|o| o.code.as_deref() == Some(code.as_str())) {
            return code;
        }
    }
    unreachable!()
}

/// A request that will be written on Discord: held, and its code.
pub fn open_written(id: &str, what: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    let code = new_code(id, &OPEN.lock().unwrap());
    hold(Open { id: id.into(), what: what.into(), read: None, code: Some(code.clone()), at: Instant::now() }).then_some(code)
}

/// « OK Ciel » has just read it to the user, in turn `turn`.
pub fn read_aloud(id: &str, turn: u64) {
    if let Some(open) = OPEN.lock().unwrap().iter_mut().find(|o| o.id == id) {
        open.read = Some(turn);
    }
}

/// The request still waits for an answer.
pub fn waits(id: &str) -> bool {
    OPEN.lock().unwrap().iter().any(|o| o.id == id && o.at.elapsed() < LIFE)
}

/// The request the user's words of turn `turn` may answer: the latest one
/// read to them before it that still waits.
fn answerable(list: &[Open], turn: u64) -> Option<&Open> {
    list.iter().rev().find(|o| o.at.elapsed() < LIFE && o.read.is_some_and(|read| turn > read))
}

/// Told to the brain before the user's words, while a request read to them waits.
pub fn context(turn: u64) -> Option<String> {
    let list = OPEN.lock().unwrap();
    let open = answerable(&list, turn)?;
    Some(format!(
        "(Ciel's own cue, not the user's words: Ciel has just read them an agent's request and asked whether to allow it: « {} ». \
If their words below agree, call permission with action allow; if they refuse, action deny; if it is unclear, ask again in a few \
words. If they speak of something else, answer that and leave the request alone.) ",
        open.what
    ))
}

/// The brain answers words the user spoke (voice/mod.rs), or not.
pub fn voice_turn(on: bool) {
    BY_VOICE.store(on, Ordering::Relaxed);
}

/// The brain answers words the user spoke aloud, and nothing written.
pub fn spoken() -> bool {
    BY_VOICE.load(Ordering::Relaxed) && !BY_DISCORD.load(Ordering::Relaxed)
}

/// The brain answers what the user wrote on Discord (discord.rs), or not.
pub fn discord_turn(on: bool) {
    BY_DISCORD.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy)]
enum By {
    Voice,
    Discord,
}

fn decide(app: &AppHandle, id: &str, word: &str, by: By) {
    OPEN.lock().unwrap().retain(|o| o.id != id);
    let by_word = match by {
        By::Voice => "voice",
        By::Discord => "discord",
    };
    crate::learn::answered(app, id, word, by_word);
    crate::recap::record_decision(app, id, word);
    crate::pipe::answer(app, id, word);
    let by = match by {
        By::Voice => "voice",
        By::Discord => "discord",
    };
    // By what, never what was asked.
    crate::log::line(format!("{by}: {word} id={id}"));
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "approval-answered", json!({ "requestId": id, "decision": word, "by": by }));
}

/// The brain's `permission` tool: what goes back to it, and whether it acted.
pub fn tool(app: &AppHandle, input: &Value) -> (String, bool) {
    let word = match input["action"].as_str() {
        Some("allow") => "allow",
        Some("deny") => "deny",
        _ => return ("The actions are allow and deny.".into(), false),
    };
    if !spoken() {
        return (
            "Only the user's own spoken answer, right after Ciel read them the request, answers it. On Discord they reply with the code Ciel wrote them: you can't answer it for them.".into(),
            false,
        );
    }
    let found = answerable(&OPEN.lock().unwrap(), crate::voice::turn()).map(|o| (o.id.clone(), o.what.clone()));
    let Some((id, what)) = found else {
        return ("No request read aloud to the user still waits: there is nothing to answer.".into(), false);
    };
    decide(app, &id, word, By::Voice);
    (format!("{} « {what} ».", if word == "allow" { "Allowed:" } else { "Denied:" }), true)
}

/// A reply on Discord made of an answer word and a code: « allow 4821 ».
fn parse(text: &str, allow: &str, deny: &str) -> Option<(&'static str, String)> {
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '.' | '!' | ':' | '«' | '»' | '"' | '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let [a, b] = words.as_slice() else { return None };
    let is_code = |w: &str| w.len() == 4 && w.bytes().all(|b| b.is_ascii_digit());
    let (word, code) = if is_code(b) {
        (a, b)
    } else if is_code(a) {
        (b, a)
    } else {
        return None;
    };
    let word = word.as_str();
    if word == "allow" || word == "yes" || word == "ok" || word == allow {
        Some(("allow", code.clone()))
    } else if word == "deny" || word == "no" || word == deny {
        Some(("deny", code.clone()))
    } else {
        None
    }
}

/// What the user wrote on Discord answers a request with its code: what to
/// write back. None for anything else, which goes to the brain.
pub fn by_code(app: &AppHandle, text: &str) -> Option<String> {
    let (allow, deny) = words();
    let (word, code) = parse(text, &allow, &deny)?;
    let found = OPEN.lock().unwrap().iter().find(|o| o.code.as_deref() == Some(code.as_str()) && o.at.elapsed() < LIFE).map(|o| o.id.clone());
    let Some(id) = found else { return Some(t("That request is no longer waiting.")) };
    decide(app, &id, word, By::Discord);
    Some(if word == "allow" { t("Allowed.") } else { t("Denied.") })
}

/// The relay is done with the request (pipe.rs): `decided` when an answer
/// went back. One written on Discord that got none is said to have gone back
/// to the terminal.
pub fn closed(id: &str, decided: bool) {
    let gone = {
        let mut list = OPEN.lock().unwrap();
        list.iter().position(|o| o.id == id).map(|at| list.remove(at))
    };
    if let Some(open) = gone.filter(|o| !decided && o.code.is_some()) {
        crate::discord::text(tf("That request went back to the terminal: {what}.", &[("what", &open.what)]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(id: &str, read: Option<u64>) -> Open {
        Open { id: id.into(), what: format!("run {id}"), read, code: None, at: Instant::now() }
    }

    #[test]
    fn only_a_request_read_aloud_before_the_users_words_is_answered() {
        let list = [open("a", Some(3)), open("b", None), open("c", Some(5))];
        // Words of the turn it was read in, or before: nothing.
        assert_eq!(answerable(&list, 3), None);
        // After the first was read, before the last: the first.
        assert_eq!(answerable(&list, 4).map(|o| o.id.as_str()), Some("a"));
        // The latest read wins; one never read is never answered.
        assert_eq!(answerable(&list, 6).map(|o| o.id.as_str()), Some("c"));
        assert_eq!(answerable(&[open("b", None)], 9), None);
        // Past its life, nothing waits.
        let old = Open { at: Instant::now() - LIFE - Duration::from_secs(1), ..open("d", Some(1)) };
        assert_eq!(answerable(&[old], 9), None);
    }

    #[test]
    fn a_discord_reply_answers_only_with_an_answer_word_and_a_code() {
        let p = |text: &str| parse(text, "autoriser", "refuser");
        assert_eq!(p("allow 4821"), Some(("allow", "4821".into())));
        assert_eq!(p(" Deny 4821. "), Some(("deny", "4821".into())));
        assert_eq!(p("4821 yes"), Some(("allow", "4821".into())));
        assert_eq!(p("« autoriser 4821 »"), Some(("allow", "4821".into())));
        assert_eq!(p("Refuser 0042"), Some(("deny", "0042".into())));
        for not in ["allow", "yes", "4821", "allow 482", "allow 48210", "allow it 4821", "please allow 4821", "maybe 4821", "allow abcd"] {
            assert_eq!(p(not), None, "{not}");
        }
    }

    #[test]
    fn what_is_asked_is_told_in_a_few_words_and_some_requests_never() {
        let ask = |tool: &str, input: Value| askable(&json!({ "tool_name": tool, "tool_input": input }));
        assert_eq!(ask("Bash", json!({ "command": "npm test\necho done" })).as_deref(), Some("run npm test"));
        assert_eq!(ask("Edit", json!({ "file_path": "D:\\ciel\\src\\main.rs" })).as_deref(), Some("edit main.rs"));
        assert_eq!(ask("WebFetch", json!({ "url": "https://user@docs.rs:443/serde?x=1" })).as_deref(), Some("open docs.rs"));
        assert_eq!(ask("mcp__github__create_issue", json!({})).as_deref(), Some("use create_issue"));
        let long = ask("Bash", json!({ "command": "x".repeat(200) })).unwrap();
        assert!(long.chars().count() <= "run ".len() + WHAT_CHARS + 1 && long.ends_with('…'));
        // A question, a plan, Ciel's own brain, or a request that says nothing: the card.
        assert_eq!(ask("AskUserQuestion", json!({})), None);
        assert_eq!(ask("ExitPlanMode", json!({})), None);
        assert_eq!(ask("Bash", json!({ "command": "  " })), None);
        assert_eq!(askable(&json!({ "tool_name": "Bash", "tool_input": { "command": "ls" }, "ciel_agent": "ciel" })), None);
        assert_eq!(line("Codex", "site", "run ls"), "Codex asks, in site: run ls. Allow it?");
        assert_eq!(line("Codex", "", "run ls"), "Codex asks: run ls. Allow it?");
    }

    #[test]
    fn each_request_written_has_its_own_code() {
        let taken = [Open { code: Some("1234".into()), ..open("a", None) }];
        for n in 0..50 {
            let code = new_code(&format!("r{n}"), &taken);
            assert!(code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()) && code != "1234", "{code}");
        }
        assert!(reply_hint("4821").contains("allow 4821") && reply_hint("4821").contains("deny 4821"));
    }
}
