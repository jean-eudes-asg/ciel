// Coding sessions started by voice: « OK Ciel, start a session in brollyhub
// to fix the login test », or « continue the one in ciel ».
//
// The brain only proposes (the `session` tool). Ciel shows the proposal on a
// card in the island, and the session starts on the user's click on Start, or
// on their yes in their next words — never in the turn that proposed it, so
// nothing the brain read or misheard can start work on its own.
//
// It starts in a terminal tab the user sees: their own `claude`, with their
// own settings and hooks, so it shows in the island and asks its permissions
// there as any session does, in a project of the folders they named for
// their work (work.rs), or in such a folder itself when it holds no project,
// and nowhere else. Ciel gives it the task as its first
// prompt, `--continue` to go on with the folder's last conversation, and
// nothing that loosens anything.
//
// The terminal is given the relay and a token, never the user's words:
// `ciel-hook --session <token>` asks Ciel what to start (hook/src/session.rs),
// once, within a minute.
//
// Part of « OK Ciel » on the user's own Claude Code: without the `claude-code`
// feature, or while that is off, there is no such tool.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::i18n::{t, tf};
use crate::notify::{self, Button, Notification, Target};

/// A proposal waits this long for the user's yes; its card stays as long.
const PROPOSAL_KEPT: Duration = Duration::from_secs(120);
/// What the relay may collect, once, within this of the yes.
const LAUNCH_KEPT: Duration = Duration::from_secs(60);
/// The task, at most.
const TASK_CHARS: usize = 2000;
/// A card that was answered by voice shows what happened this long.
const FOLDS_AFTER_MS: f64 = 2500.0;

/// What the brain is told of the tool, after its other instructions.
pub const PROMPT: &str = "
Sessions: when the user asks to start work in one of their projects or work folders, or to go on with it, call session with \
action start or continue, its name as their work lists it and the task in their own words. That only proposes it: a card in the island asks them. Then ask \
them, in one short question naming the project and the task, whether to start. When they say yes in their next words, call \
session with action confirm; when they say no, action cancel. Never call confirm before they have answered.";

pub fn tool_def() -> Value {
    json!({
        "name": "session",
        "description": "Start a coding session of the user's own agent in one of their projects or work folders, in a terminal tab they see, or go on with its last conversation. start and continue only propose it, on a card in the island; confirm starts it once the user said yes in their next words (or they click Start themselves); cancel drops it.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["start", "continue", "confirm", "cancel"] },
                "project": { "type": "string", "description": "start, continue: the project's or the work folder's name, as the user's work lists it." },
                "task": { "type": "string", "description": "start: what the session should do, in the user's words. continue: what to do next, or leave out to just open it." }
            },
            "required": ["action"]
        }
    })
}

/// What the user was asked to say yes to.
#[derive(Debug, Clone, PartialEq)]
struct Proposal {
    /// Its card in the island.
    card: String,
    project: String,
    path: PathBuf,
    task: String,
    /// Go on with the folder's last conversation.
    resume: bool,
    /// The turn of « OK Ciel » it was made in (voice/mod.rs `turn`).
    turn: u64,
    at: Instant,
}

/// What the relay is told to start.
#[derive(Debug, Clone, PartialEq)]
struct Launch {
    exe: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

/// One proposal at a time: a new one takes the place of the last.
static PROPOSED: Mutex<Option<Proposal>> = Mutex::new(None);
/// The sessions said yes to, until the relay in their terminal asks.
static READY: Mutex<Vec<(String, Launch, Instant)>> = Mutex::new(Vec::new());

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

/// The task as one line, never read as an option by `claude`: no dash first.
fn task(said: &str) -> String {
    let line = said.split_whitespace().collect::<Vec<_>>().join(" ");
    line.trim_start_matches(|c: char| c == '-' || c.is_whitespace()).chars().take(TASK_CHARS).collect()
}

/// What `claude` is given: `--continue` for the folder's last conversation,
/// the task as its first prompt, and nothing else. No permission mode and no
/// setting: the session is the user's own, as if they had typed it.
fn arguments(task: &str, resume: bool) -> Vec<String> {
    let mut args = Vec::new();
    if resume {
        args.push("--continue".to_string());
    }
    if !task.is_empty() {
        args.push(task.to_string());
    }
    args
}

/// A yes by voice counts only once the user has spoken since the proposal.
fn spoken_since(proposed: u64, now: u64) -> bool {
    now > proposed
}

fn title(p: &Proposal) -> String {
    let project = [("project", p.project.as_str())];
    if p.resume {
        tf("Continue the session in {project}?", &project)
    } else {
        tf("Start a session in {project}?", &project)
    }
}

/// The card says what became of the proposal, then folds away.
fn fold(app: &AppHandle, card: &str, title: String) {
    notify::update(app, notify::Update { id: card.to_string(), title, body: String::new(), buttons: None, until: Some(now_ms() + FOLDS_AFTER_MS) });
}

/// The tool is there: this build has the engine, the user turned it on, and
/// their `claude` is found and signed in.
fn available(app: &AppHandle) -> bool {
    cfg!(feature = "claude-code") && super::wanted(app)
}

/// The `session` tool: what goes back to the brain, and whether a session started.
pub fn tool(app: &AppHandle, input: &Value) -> (String, bool) {
    if !available(app) {
        return ("unknown tool session".into(), false);
    }
    match input["action"].as_str().unwrap_or("") {
        action @ ("start" | "continue") => propose(app, input, action == "continue"),
        "confirm" => confirm(app),
        "cancel" => cancel(app),
        _ => ("The actions are start, continue, confirm and cancel.".into(), false),
    }
}

fn propose(app: &AppHandle, input: &Value, resume: bool) -> (String, bool) {
    let settings = app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().clone()).unwrap_or_default();
    let places = crate::work::places(&crate::work::folders(&settings));
    let Some((name, path)) = crate::work::startable(&places, input["project"].as_str().unwrap_or("")) else {
        let names = crate::work::startable_names(&places);
        return (format!("No project or work folder of that name in the folders the user named for their work. They are: {}. Ask which one.", names.join(", ")), false);
    };
    let task = task(input["task"].as_str().unwrap_or(""));
    if !resume && task.is_empty() {
        return ("Ask the user what the session should do.".into(), false);
    }
    let proposal = Proposal {
        card: notify::next_id("session"),
        project: name.to_string(),
        path: path.to_path_buf(),
        task,
        resume,
        turn: crate::voice::turn(),
        at: Instant::now(),
    };
    if let Some(old) = PROPOSED.lock().unwrap().replace(proposal.clone()) {
        fold(app, &old.card, title(&old));
    }
    notify::show(
        app,
        Notification {
            id: proposal.card.clone(),
            source: "ciel".into(),
            app: "Ciel".into(),
            title: title(&proposal),
            body: proposal.task.clone(),
            icon: None,
            at: now_ms(),
            tag: None,
            buttons: vec![Button { id: "start".into(), label: t("Start") }],
            until: Some(now_ms() + PROPOSAL_KEPT.as_millis() as f64),
            time: None,
        },
        Target::Session,
    );
    (
        format!(
            "Proposed, not started: a card in the island asks the user about {}. Now ask them, in one short question naming the project and the task, whether to start it. When they say yes in their next words, call session with action confirm; when they say no, action cancel. Never call confirm before they have answered.",
            proposal.project
        ),
        false,
    )
}

/// The user's yes, as the brain heard it: only after they spoke.
fn confirm(app: &AppHandle) -> (String, bool) {
    let proposal = {
        let mut slot = PROPOSED.lock().unwrap();
        let Some(p) = slot.as_ref() else {
            return ("Nothing is waiting: the user clicked Start or dismissed the card already, or nothing was proposed.".into(), false);
        };
        if p.at.elapsed() > PROPOSAL_KEPT {
            *slot = None;
            return ("That proposal is too old. Propose it again if the user still wants it.".into(), false);
        }
        if !spoken_since(p.turn, crate::voice::turn()) {
            return ("The user has not answered yet. Ask them, and wait for their words.".into(), false);
        }
        slot.take()
    };
    let Some(proposal) = proposal else { return ("Nothing is waiting.".into(), false) };
    match launch(&proposal) {
        Ok(()) => {
            fold(app, &proposal.card, tf("The session in {project} is starting.", &[("project", &proposal.project)]));
            (format!("The session in {} is starting in a new terminal tab. It shows in the island, and asks its permissions there.", proposal.project), true)
        }
        Err(why) => {
            fold(app, &proposal.card, title(&proposal));
            (why, false)
        }
    }
}

fn cancel(app: &AppHandle) -> (String, bool) {
    match PROPOSED.lock().unwrap().take() {
        Some(p) => {
            fold(app, &p.card, title(&p));
            ("Dropped: nothing starts.".into(), false)
        }
        None => ("Nothing was waiting.".into(), false),
    }
}

/// Start, or Dismiss, on the proposal's card (notify.rs).
pub fn clicked(card: &str, start: bool) {
    let proposal = {
        let mut slot = PROPOSED.lock().unwrap();
        if slot.as_ref().is_some_and(|p| p.card == card) {
            slot.take()
        } else {
            None
        }
    };
    let Some(proposal) = proposal else { return };
    if !start || proposal.at.elapsed() > PROPOSAL_KEPT {
        return;
    }
    // A terminal to open: not on the window's thread.
    std::thread::spawn(move || {
        if let Err(why) = launch(&proposal) {
            crate::log::line(format!("agent: the session didn't start: {why}"));
        }
    });
}

/// Opens the terminal on the relay, which then asks what to start (`collect`).
fn launch(p: &Proposal) -> Result<(), String> {
    let exe = super::claude_path().ok_or("The user's claude was not found.")?;
    if !p.path.is_dir() {
        return Err("That project's folder is no longer there.".into());
    }
    let token = new_token()?;
    {
        let mut ready = READY.lock().unwrap();
        ready.retain(|(_, _, at)| at.elapsed() <= LAUNCH_KEPT);
        ready.push((token.clone(), Launch { exe, args: arguments(&p.task, p.resume), cwd: p.path.clone() }, Instant::now()));
    }
    let relay = crate::settings::hook_exe_path();
    if !crate::platform::run_in_terminal(&relay, &["--session".to_string(), token.clone()], &p.project) {
        READY.lock().unwrap().retain(|(t, ..)| *t != token);
        return Err("No terminal could be opened for the session.".into());
    }
    // Never the task: only that one started, and where.
    crate::log::line(format!("agent: a session {} in {}", if p.resume { "goes on" } else { "starts" }, p.project));
    Ok(())
}

fn new_token() -> Result<String, String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).map_err(|_| "no randomness".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The relay in the new terminal asks what to start (pipe.rs): it is told
/// once, within a minute of the yes. None for any other token.
pub fn collect(payload: &Value) -> Option<String> {
    let token = payload.get("ciel_session")?.as_str()?;
    let mut ready = READY.lock().unwrap();
    ready.retain(|(_, _, at)| at.elapsed() <= LAUNCH_KEPT);
    let at = ready.iter().position(|(t, ..)| super::same(t.as_bytes(), token.as_bytes()))?;
    let (_, launch, _) = ready.remove(at);
    Some(json!({ "exe": launch.exe.to_string_lossy(), "args": launch.args, "cwd": launch.cwd.to_string_lossy() }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_gets_the_task_and_nothing_that_loosens_anything() {
        assert_eq!(arguments("fix the login test", false), ["fix the login test"]);
        assert_eq!(arguments("then run the tests", true), ["--continue", "then run the tests"]);
        assert_eq!(arguments("", true), ["--continue"]);
        for (said, resume) in [("fix it", false), ("fix it", true), ("", true)] {
            let args = arguments(said, resume);
            for never in ["--dangerously-skip-permissions", "--permission-mode", "--allowedTools", "--settings", "-p", "--print", "--bare"] {
                assert!(!args.iter().any(|a| a == never), "{never}");
            }
        }
    }

    #[test]
    fn the_task_is_one_line_and_never_an_option() {
        assert_eq!(task("  fix the\nlogin   test "), "fix the login test");
        assert_eq!(task("--dangerously-skip-permissions"), "dangerously-skip-permissions");
        assert_eq!(task(" - -p do it; rm -rf x"), "p do it; rm -rf x");
        assert_eq!(task("---"), "");
        assert_eq!(task(&"x".repeat(5000)).chars().count(), TASK_CHARS);
        // Whatever is in it, it stays one argument.
        assert_eq!(arguments(&task("say \"hi\"; echo & del *"), false).len(), 1);
    }

    #[test]
    fn a_spoken_yes_needs_the_user_to_have_spoken_since() {
        assert!(!spoken_since(7, 7), "the turn that proposed it can't confirm it");
        assert!(spoken_since(7, 8));
        assert!(!spoken_since(7, 6));
    }

    #[test]
    fn the_relay_is_told_what_to_start_once_and_only_with_its_token() {
        let launch = Launch { exe: PathBuf::from("/home/me/.local/bin/claude"), args: arguments("fix it", true), cwd: PathBuf::from("/work/ciel") };
        READY.lock().unwrap().push(("00112233445566778899aabbccddeeff".into(), launch, Instant::now()));
        assert_eq!(collect(&json!({ "ciel_session": "00112233445566778899aabbccddeef0" })), None);
        assert_eq!(collect(&json!({ "ciel_session": "" })), None);
        assert_eq!(collect(&json!({ "ciel_mcp": {} })), None);
        let told: Value = serde_json::from_str(&collect(&json!({ "ciel_session": "00112233445566778899aabbccddeeff" })).unwrap()).unwrap();
        assert_eq!(told, json!({ "exe": "/home/me/.local/bin/claude", "args": ["--continue", "fix it"], "cwd": "/work/ciel" }));
        assert_eq!(collect(&json!({ "ciel_session": "00112233445566778899aabbccddeeff" })), None, "a token is good once");
    }

    #[test]
    fn the_tool_proposes_before_it_starts() {
        let tool = tool_def();
        assert_eq!(tool["name"], "session");
        assert_eq!(tool["inputSchema"]["properties"]["action"]["enum"], json!(["start", "continue", "confirm", "cancel"]));
        assert!(PROMPT.contains("Never call confirm before they have answered."));
    }
}

/// With a real terminal, and the relay built: `cargo test -p ciel -- --ignored in_a_terminal`.
#[cfg(test)]
mod with_a_terminal {
    #[test]
    #[ignore]
    fn the_relay_runs_in_a_terminal_and_starts_nothing_without_a_token_of_ciels() {
        let relay = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/release").join(crate::platform::HOOK_EXE);
        assert!(relay.is_file(), "build the relay first: cargo build --release -p ciel-hook");
        // A token Ciel never made: the relay asks no one, says nothing is there to start, and the tab closes.
        assert!(crate::platform::run_in_terminal(&relay, &["--session".to_string(), "not-a-token".to_string()], "Ciel; test"));
    }
}
