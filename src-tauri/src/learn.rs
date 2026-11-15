// What Ciel learns from the user's answers to their agents (Ciel window →
// Chat → Speak up → Learn from my answers, off by default).
//
// The journal: each answer the user gives to an agent's permission request —
// a click on the card, a spoken answer, a reply on Discord with its code — is
// one line of approvals.jsonl in Ciel's own folder: when, which agent, which
// project, which tool, what was asked (as a rule would remember it, rules.rs),
// allow or deny, and how. A plain file the user can read or delete, kept to a
// few thousand lines, never sent anywhere. Answers given by a rule are not
// written: they are not the user's.
//
// Learning: from the journal, Ciel's code finds what the user always allows —
// the same request of the same agent in the same project, allowed at least
// three times in thirty days and never denied, that no rule covers yet and
// that is nothing destructive (`dangerous`). At the desk, between
// conversations, at most once every two hours and three times a day, the brain
// is shown one of them: it judges whether it is safe for Ciel to allow on its
// own, and asks the user in one question if so, or lets it go (`learn` skip).
// The user's spoken yes, in their next words, makes it a rule like one of
// « Don't ask again », marked learned: Ciel allows those requests from then
// on, and each answer leaves a line on the session; the user removes it in
// Settings → Agents like any other. A no, or the brain's skip, is remembered
// for thirty days. Nothing becomes a rule without the user's yes, and what a
// rule covers is matched by Ciel's code (rules.rs), never by the brain.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::rules::What;

/// The journal, at most; past it the older half goes.
const MAX_JOURNAL_BYTES: u64 = 1024 * 1024;
/// Requests waiting for an answer, whatever happens to them.
const MAX_WAITING: usize = 64;
/// Allowed this many times, never denied, within `WINDOW`: worth asking.
const ENOUGH: usize = 3;
const WINDOW: i64 = 30 * 24 * 3600;
/// A no, or the brain's skip, holds this long.
const DECLINED_FOR: i64 = 30 * 24 * 3600;
/// Two questions are this far apart at least, and no more than `PER_DAY` a day.
const APART: i64 = 2 * 3600;
const PER_DAY: u32 = 3;
/// The user's answer to a question comes within this.
const ANSWER_WITHIN: Duration = Duration::from_secs(180);
/// How often the journal is looked at, while the user wants it.
const LOOK: Duration = Duration::from_secs(600);

/// One answer of the user's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    at: i64,
    agent: String,
    cwd: String,
    tool: String,
    /// What a rule would remember of the request; none when nothing could be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    what: Option<What>,
    /// The request in a few words, for the user reading the file.
    asked: String,
    /// "allow" or "deny".
    answer: String,
    /// "click", "voice" or "discord".
    by: String,
}

/// What the user always allows: one candidate for a rule.
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    agent: String,
    cwd: String,
    tool: String,
    what: What,
    allowed: usize,
}

impl Candidate {
    fn key(&self) -> String {
        key(&self.agent, &self.cwd, &self.tool, &self.what)
    }
}

fn key(agent: &str, cwd: &str, tool: &str, what: &What) -> String {
    format!("{agent}\u{1f}{}\u{1f}{tool}\u{1f}{}", cwd.to_lowercase(), serde_json::to_string(what).unwrap_or_default())
}

/// What is kept between launches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Kept {
    /// Keys not to ask about again, and when that was settled.
    declined: Vec<(String, i64)>,
    /// When the last question was asked.
    last: i64,
    /// The day of `today`'s count, as days since 1970 on the local clock.
    day: i64,
    today: u32,
}

/// A question asked, waiting for the user's answer.
struct Asked {
    candidate: Candidate,
    turn: u64,
    at: Instant,
}

struct Waiting {
    agent: String,
    cwd: String,
    tool: String,
    what: Option<What>,
    asked: String,
}

static WAITING: Mutex<Option<HashMap<String, Waiting>>> = Mutex::new(None);
static ASKED: Mutex<Option<Asked>> = Mutex::new(None);
/// The brain is being shown a candidate, in a turn of Ciel's own.
static SHOWING: AtomicBool = AtomicBool::new(false);
/// One look at a time.
static FILES: Mutex<()> = Mutex::new(());

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn today() -> i64 {
    chrono::Local::now().date_naive().signed_duration_since(chrono::NaiveDate::default()).num_days()
}

pub fn journal_file() -> PathBuf {
    crate::platform::local_dir().join("approvals.jsonl")
}

fn kept_file() -> PathBuf {
    crate::platform::local_dir().join("learned.json")
}

fn on(app: &AppHandle) -> bool {
    app.try_state::<crate::Shared>().is_some_and(|s| {
        let s = s.settings.lock().unwrap();
        s.speak_up.learn
    })
}

// ── The journal ───────────────────────────────────────────────────────────────

/// A permission request arrives (pipe.rs): what it is, kept until answered.
pub fn note_request(app: &AppHandle, id: &str, payload: &Value) {
    if !on(app) || payload.get("ciel_rule").is_some() {
        return;
    }
    let text = |key: &str| payload.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    if text("ciel_agent") == "ciel" {
        return;
    }
    let Some(asked) = crate::asked::askable(payload) else { return };
    let what = crate::rules::offer(payload).map(|(_, what)| what);
    let mut waiting = WAITING.lock().unwrap();
    let map = waiting.get_or_insert_with(HashMap::new);
    if map.len() >= MAX_WAITING {
        map.clear();
    }
    map.insert(id.to_string(), Waiting { agent: text("ciel_agent"), cwd: text("cwd"), tool: text("tool_name"), what, asked });
}

/// The relay is done with the request (pipe.rs).
pub fn forget(id: &str) {
    if let Some(map) = WAITING.lock().unwrap().as_mut() {
        map.remove(id);
    }
}

/// The user answered it: a line in the journal. `decision` as the card says
/// it ("always" is an allow); `by` "click", "voice" or "discord".
pub fn answered(app: &AppHandle, id: &str, decision: &str, by: &str) {
    if !on(app) {
        return;
    }
    let Some(w) = WAITING.lock().unwrap().as_mut().and_then(|map| map.remove(id)) else { return };
    let answer = if matches!(decision, "allow" | "always") { "allow" } else { "deny" };
    let entry = Entry { at: now(), agent: w.agent, cwd: w.cwd, tool: w.tool, what: w.what, asked: w.asked, answer: answer.into(), by: by.into() };
    let _one = FILES.lock().unwrap();
    if let Err(err) = append(&journal_file(), &entry) {
        crate::log::line(format!("learn: the journal couldn't be written: {err}"));
    }
}

fn append(path: &Path, entry: &Entry) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::platform::ensure_private_dir(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_JOURNAL_BYTES) {
        // The newer half stays.
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let kept = lines[lines.len() / 2..].join("\n");
        crate::wallpaper::write_atomic(path, format!("{kept}\n").as_bytes())?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(entry)?)
}

fn read_journal(path: &Path) -> Vec<Entry> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 2 * MAX_JOURNAL_BYTES) {
        return Vec::new();
    }
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

// ── What is worth asking ──────────────────────────────────────────────────────

/// The first word of a command, as a shell would find it: no folder, no
/// `.exe`, lower case.
fn program(line: &str) -> String {
    let first = line.split_whitespace().next().unwrap_or_default().trim_matches(|c| c == '"' || c == '\'');
    let name = first.rsplit(['/', '\\']).next().unwrap_or(first).to_lowercase();
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

/// Never learned, however often allowed: what deletes, publishes, sends
/// anything out, changes the system or rewrites history, a whole MCP tool
/// (which can do anything), and any command that isn't one plain command.
fn dangerous(tool: &str, what: &What) -> bool {
    const PROGRAMS: &[&str] = &[
        "rm", "rmdir", "del", "erase", "rd", "remove-item", "ri", "format", "mkfs", "dd", "shred", "shutdown", "reboot",
        "sudo", "su", "runas", "chmod", "chown", "takeown", "icacls", "curl", "wget", "invoke-webrequest", "iwr",
        "invoke-restmethod", "irm", "ssh", "scp", "sftp", "ftp", "nc", "ncat", "netcat", "reg", "regedit", "setx",
        "kill", "taskkill", "stop-process", "powershell", "pwsh", "cmd", "bash", "sh", "zsh", "eval", "iex",
        "invoke-expression", "start-process", "mv", "move", "move-item",
    ];
    const SUBCOMMANDS: &[(&str, &str)] = &[
        ("git", "push"), ("git", "reset"), ("git", "clean"), ("git", "rebase"), ("git", "checkout"), ("git", "restore"),
        ("git", "branch"), ("git", "tag"), ("git", "rm"), ("git", "filter-branch"), ("git", "config"), ("npm", "publish"),
        ("npm", "unpublish"), ("npm", "install"), ("npm", "i"), ("npm", "uninstall"), ("pnpm", "publish"), ("yarn", "publish"),
        ("cargo", "publish"), ("cargo", "install"), ("pip", "install"), ("pip", "uninstall"), ("docker", "rm"),
        ("docker", "rmi"), ("docker", "system"), ("docker", "push"), ("kubectl", "delete"), ("gh", "release"),
        ("gh", "repo"), ("gh", "pr"), ("winget", "install"), ("winget", "uninstall"), ("choco", "install"),
    ];
    match what {
        What::Exact(line) | What::Prefix(line) => {
            let plain = !line.chars().any(|c| matches!(c, ';' | '&' | '|' | '`' | '$' | '<' | '>' | '(' | ')' | '{' | '}' | '\n' | '\r' | '\0'));
            let program = program(line);
            let second = line.split_whitespace().nth(1).unwrap_or_default().to_lowercase();
            !plain
                || program.is_empty()
                || PROGRAMS.contains(&program.as_str())
                || SUBCOMMANDS.iter().any(|(p, s)| *p == program && *s == second)
                || line.split_whitespace().any(|w| matches!(w.to_lowercase().as_str(), "--force" | "-f" | "--hard" | "-rf" | "-fr"))
        }
        What::Tool => tool != "WebSearch",
        What::Domain(_) | What::Edits => false,
    }
}

/// What the user always allows, most allowed first: allowed `ENOUGH` times
/// within the window and never denied there, not covered by a rule, not
/// declined lately, and nothing dangerous.
fn candidates(journal: &[Entry], rules: &[crate::rules::Rule], declined: &[(String, i64)], now: i64) -> Vec<Candidate> {
    let mut seen: HashMap<String, (Candidate, bool)> = HashMap::new();
    for entry in journal.iter().filter(|e| now - e.at <= WINDOW) {
        let Some(what) = entry.what.clone() else { continue };
        let key = key(&entry.agent, &entry.cwd, &entry.tool, &what);
        let slot = seen.entry(key).or_insert_with(|| {
            (Candidate { agent: entry.agent.clone(), cwd: entry.cwd.clone(), tool: entry.tool.clone(), what, allowed: 0 }, false)
        });
        if entry.answer == "allow" {
            slot.0.allowed += 1;
        } else {
            slot.1 = true;
        }
    }
    let mut out: Vec<Candidate> = seen
        .into_iter()
        .filter(|(key, (c, denied))| {
            !denied
                && c.allowed >= ENOUGH
                && !dangerous(&c.tool, &c.what)
                && !declined.iter().any(|(k, at)| k == key && now - at < DECLINED_FOR)
                && !rules.iter().any(|r| r.agent == c.agent && r.cwd.eq_ignore_ascii_case(&c.cwd) && r.tool == c.tool && r.what == c.what)
        })
        .map(|(_, (c, _))| c)
        .collect();
    out.sort_by(|a, b| b.allowed.cmp(&a.allowed).then_with(|| a.key().cmp(&b.key())));
    out
}

/// A question may be asked now: far enough from the last, and not too many today.
fn due(kept: &Kept, now: i64, day: i64) -> bool {
    now - kept.last >= APART && (kept.day != day || kept.today < PER_DAY)
}

fn read_kept() -> Kept {
    std::fs::read(kept_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_kept(kept: &Kept) {
    let path = kept_file();
    if let Some(dir) = path.parent() {
        let _ = crate::platform::ensure_private_dir(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(kept) {
        let _ = crate::wallpaper::write_atomic(&path, &bytes);
    }
}

fn rules_now(app: &AppHandle) -> Vec<crate::rules::Rule> {
    crate::rules::list(app)
}

/// The candidate behind `key`, if it still is one.
fn find(app: &AppHandle, key: &str) -> Option<Candidate> {
    let _one = FILES.lock().unwrap();
    let kept = read_kept();
    candidates(&read_journal(&journal_file()), &rules_now(app), &kept.declined, now()).into_iter().find(|c| c.key() == key)
}

// ── Asking ────────────────────────────────────────────────────────────────────

fn project(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or_default().to_string()
}

fn agent_name(agent: &str) -> String {
    match agent {
        "" => "Claude Code".into(),
        id => crate::agents::Agent::from_id(id).map_or_else(|| id.to_string(), |a| a.name().to_string()),
    }
}

/// The candidate in words, for the brain.
fn told(c: &Candidate) -> String {
    match &c.what {
        What::Exact(line) => format!("run « {line} »"),
        What::Prefix(start) => format!("run commands that start with « {start} »"),
        What::Domain(host) => format!("open pages on {host}"),
        What::Edits => "edit files inside the project".into(),
        What::Tool => format!("use the tool {}", c.tool),
    }
}

/// Once in a while, at the desk: one candidate is put forward to be asked
/// about (speakup.rs queues it, « OK Ciel » asks the brain).
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(300)).await;
        loop {
            look(&app);
            tokio::time::sleep(LOOK).await;
        }
    });
}

fn look(app: &AppHandle) {
    expire();
    if !on(app) || ASKED.lock().unwrap().is_some() {
        return;
    }
    let first = {
        let _one = FILES.lock().unwrap();
        let kept = read_kept();
        if !due(&kept, now(), today()) {
            return;
        }
        candidates(&read_journal(&journal_file()), &rules_now(app), &kept.declined, now()).into_iter().next()
    };
    if let Some(c) = first {
        crate::speakup::propose(app, c.key());
    }
}

/// « OK Ciel » shows the candidate to the brain now (voice/mod.rs): what it
/// is asked. None if it is no longer one.
pub fn cue(app: &AppHandle, key: &str) -> Option<String> {
    let c = find(app, key)?;
    {
        let _one = FILES.lock().unwrap();
        let mut kept = read_kept();
        let day = today();
        if kept.day != day {
            kept.day = day;
            kept.today = 0;
        }
        kept.today += 1;
        kept.last = now();
        write_kept(&kept);
    }
    let text = format!(
        "(This is Ciel's own cue, not the user's words. From the user's answers to their agents: in the project {}, {} asked {} times \
to {}, and the user always allowed it, never denied. Judge whether it is safe for Ciel to allow that on its own from now on, at the \
desk or away: nothing that deletes, publishes, installs, sends data out, touches secrets or works outside the project. If it is, ask \
the user in one short question whether you may allow it on your own from now, naming the project and what it is. If it isn't, call \
learn with action skip and say nothing. Speak in the language whose code is {}.)",
        project(&c.cwd),
        agent_name(&c.agent),
        c.allowed,
        told(&c),
        crate::i18n::current()
    );
    *ASKED.lock().unwrap() = Some(Asked { candidate: c, turn: crate::voice::turn(), at: Instant::now() });
    Some(text)
}

/// The brain is shown the candidate (true), or that turn is over (false).
pub fn showing(on: bool) {
    SHOWING.store(on, Ordering::Relaxed);
}

/// Told to the brain before the user's words, while a question waits.
pub fn context(turn: u64) -> Option<String> {
    let asked = ASKED.lock().unwrap();
    let a = asked.as_ref().filter(|a| turn > a.turn && a.at.elapsed() < ANSWER_WITHIN)?;
    Some(format!(
        "(Ciel's own cue, not the user's words: Ciel has just asked them whether it may, from now on, allow on its own {} in the \
project {}. If their words below agree, call learn with action accept; if they refuse, action decline; if it is unclear, ask again in \
a few words. If they speak of something else, answer that and leave the question alone.) ",
        told(&a.candidate),
        project(&a.candidate.cwd)
    ))
}

fn settle(key: String) {
    let _one = FILES.lock().unwrap();
    let mut kept = read_kept();
    let now = now();
    kept.declined.retain(|(k, at)| *k != key && now - at < DECLINED_FOR);
    kept.declined.push((key, now));
    write_kept(&kept);
}

/// The brain's `learn` tool: what goes back to it, and whether it acted.
pub fn tool(app: &AppHandle, input: &Value) -> (String, bool) {
    let action = input["action"].as_str().unwrap_or_default();
    match action {
        // Only while it is shown the candidate, in Ciel's own turn.
        "skip" => {
            if !SHOWING.load(Ordering::Relaxed) {
                return ("Nothing is being shown to you to skip.".into(), false);
            }
            let Some(a) = ASKED.lock().unwrap().take() else { return ("Nothing to skip.".into(), false) };
            settle(a.candidate.key());
            crate::log::line("learn: the brain let one go");
            ("Let go: the user won't be asked about it.".into(), true)
        }
        "accept" | "decline" => {
            if !crate::asked::spoken() {
                return ("Only the user's own spoken answer, right after Ciel asked them, answers this.".into(), false);
            }
            let turn = crate::voice::turn();
            let taken = {
                let mut asked = ASKED.lock().unwrap();
                match asked.as_ref() {
                    Some(a) if turn > a.turn && a.at.elapsed() < ANSWER_WITHIN => asked.take(),
                    _ => None,
                }
            };
            let Some(a) = taken else { return ("No question waits for the user's answer.".into(), false) };
            let c = a.candidate;
            if action == "decline" {
                settle(c.key());
                crate::log::line("learn: the user said no");
                return ("Understood: Ciel will keep asking for these.".into(), true);
            }
            if crate::rules::learn(app, &c.agent, &c.cwd, &c.tool, c.what.clone()) {
                crate::log::line("learn: the user said yes, a rule is learned");
                (format!("Learned: from now on Ciel allows on its own {} in {}.", told(&c), project(&c.cwd)), true)
            } else {
                ("It couldn't be kept (the list of rules may be full).".into(), false)
            }
        }
        _ => ("The actions are accept, decline and skip.".into(), false),
    }
}

/// A question that got no answer is dropped once its time is up, so the
/// next one can come in two hours.
pub fn expire() {
    let mut asked = ASKED.lock().unwrap();
    if asked.as_ref().is_some_and(|a| a.at.elapsed() >= ANSWER_WITHIN) {
        *asked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 24 * 3600;

    fn entry(what: What, answer: &str, days_ago: i64) -> Entry {
        Entry {
            at: NOW - days_ago * DAY,
            agent: String::new(),
            cwd: "D:\\ciel".into(),
            tool: "Bash".into(),
            what: Some(what),
            asked: "run it".into(),
            answer: answer.into(),
            by: "click".into(),
        }
    }

    fn exact(line: &str) -> What {
        What::Exact(line.into())
    }

    #[test]
    fn what_is_always_allowed_is_found_and_what_was_ever_denied_is_not() {
        let mut journal = vec![
            entry(exact("npm test"), "allow", 1),
            entry(exact("npm test"), "allow", 2),
            entry(exact("npm test"), "allow", 3),
            entry(exact("cargo check"), "allow", 1),
            entry(exact("cargo check"), "allow", 1),
            entry(exact("cargo build"), "allow", 1),
            entry(exact("cargo build"), "allow", 1),
            entry(exact("cargo build"), "allow", 1),
            entry(exact("cargo build"), "deny", 5),
        ];
        let found = candidates(&journal, &[], &[], NOW);
        assert_eq!(found.len(), 1, "twice is not enough, and a deny settles it");
        assert_eq!((found[0].what.clone(), found[0].allowed), (exact("npm test"), 3));
        // Older than the window: forgotten.
        journal.iter_mut().for_each(|e| e.at -= 40 * DAY);
        assert!(candidates(&journal, &[], &[], NOW).is_empty());
    }

    #[test]
    fn a_rule_or_a_no_ends_the_asking_and_the_no_wears_off() {
        let journal: Vec<Entry> = (0..4).map(|d| entry(exact("npm test"), "allow", d)).collect();
        let key = key("", "D:\\ciel", "Bash", &exact("npm test"));
        assert!(candidates(&journal, &[], &[(key.clone(), NOW - DAY)], NOW).is_empty());
        assert_eq!(candidates(&journal, &[], &[(key, NOW - 31 * DAY)], NOW).len(), 1);
        let rule = crate::rules::Rule {
            id: "r".into(),
            agent: String::new(),
            cwd: "d:\\CIEL".into(),
            tool: "Bash".into(),
            what: exact("npm test"),
            created: NOW,
            learned: false,
        };
        assert!(candidates(&journal, &[rule], &[], NOW).is_empty());
    }

    #[test]
    fn what_deletes_publishes_or_reaches_out_is_never_learned() {
        for line in [
            "rm -rf build", "del /q x", "Remove-Item x", "C:\\tools\\curl.exe https://x", "git push", "git reset --hard",
            "git checkout main", "npm publish", "npm install left-pad", "cargo test && rm x", "echo $HOME", "cat a > b",
            "git commit --force", "powershell -c x", "sudo ls",
        ] {
            assert!(dangerous("Bash", &exact(line)), "{line}");
            assert!(dangerous("Bash", &What::Prefix(line.into())), "{line}");
        }
        for line in ["npm test", "cargo test --workspace", "git status", "git diff", "npx tsc --noEmit", "pytest -q"] {
            assert!(!dangerous("Bash", &exact(line)), "{line}");
        }
        assert!(dangerous("mcp__gmail__send", &What::Tool));
        assert!(!dangerous("WebSearch", &What::Tool));
        assert!(!dangerous("Edit", &What::Edits) && !dangerous("WebFetch", &What::Domain("docs.rs".into())));
    }

    #[test]
    fn questions_are_spaced_and_few() {
        let kept = Kept { declined: Vec::new(), last: NOW - 3600, day: 10, today: 1 };
        assert!(!due(&kept, NOW, 10), "within two hours of the last");
        assert!(due(&kept, NOW + 3600, 10));
        let full = Kept { last: NOW - 3 * 3600, today: PER_DAY, ..kept.clone() };
        assert!(!due(&full, NOW, 10), "three today already");
        assert!(due(&full, NOW, 11), "a new day");
    }

    #[test]
    fn the_journal_keeps_its_lines_and_halves_when_too_long() {
        let dir = std::env::temp_dir().join(format!("ciel-learn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("approvals.jsonl");
        let one = entry(exact("npm test"), "allow", 0);
        append(&path, &one).unwrap();
        append(&path, &Entry { answer: "deny".into(), what: None, ..one.clone() }).unwrap();
        let read = read_journal(&path);
        assert_eq!(read.len(), 2);
        assert_eq!(read[0], one);
        assert_eq!(read[1].what, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
