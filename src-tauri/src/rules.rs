// Remembered answers — "Don't ask again" on a permission card.
//
// A rule is made by one click on that button, and by nothing else. It says
// which requests of one agent, in one project folder, Ciel may answer "allow"
// to from then on without a card: the click that made the rule is the click
// behind each of those answers. Rules stay until the user removes them
// (Settings → Agents), and the island says so each time one answers.
//
// What a rule covers is narrow on purpose, and decided here, never by the
// webview. Ciel has no shell parser, so it does not try to be clever:
//
//   * a command, word for word (`Exact`);
//   * a command that starts with words the agent itself suggested as a rule —
//     Claude Code's `permission_suggestions`, `npm test:*` — and holds nothing
//     that could chain, substitute or redirect (`Prefix`, see `plain`);
//   * a page on one host, for WebFetch (`Domain`);
//   * any call of one MCP tool, or a web search (`Tool`);
//   * an edit to a file inside the project folder, hidden folders left out and
//     links followed (`Edits`, Claude Code only).
//
// Anything else, anything unsure, a question or a plan to approve: no rule is
// offered and none answers. The card comes up as before, and with no click the
// agent asks in its own terminal.
//
// rules.json lives under platform::local_dir(), next to recap.json, and never
// leaves the machine. Unlike the recap it does hold commands: the ones the user
// chose to remember, which the list in Settings has to show.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::{log, platform};

/// More rules than anyone makes by hand; past it, new ones are refused.
const MAX_RULES: usize = 500;
/// A command longer than this is never remembered.
const MAX_TEXT: usize = 2000;
/// Requests waiting for their card's answer, whatever happens to them.
const MAX_WAITING: usize = 64;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// The agents whose requests the island answers (hook/src/reply.rs), as the
/// relay tags them: Claude Code has no tag. Ciel's own brain is left out.
const AGENTS: &[&str] = &["", "codex", "copilot", "muse"];
/// Not permissions to grant: a question to answer, a plan to read.
const NEVER: &[&str] = &["AskUserQuestion", "ExitPlanMode"];
/// Claude Code's tools that write a file named in `file_path` / `notebook_path`.
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// What a rule lets through.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", content = "text", rename_all = "lowercase")]
pub enum What {
    /// This command, word for word.
    Exact(String),
    /// A command that is these words, or starts with them and a space.
    Prefix(String),
    /// A page on this host.
    Domain(String),
    /// Any call of the tool.
    Tool,
    /// An edit to a file in the project folder.
    Edits,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Rule {
    pub id: String,
    /// The relay's tag: empty for Claude Code.
    pub agent: String,
    /// The project folder: the rule holds there and in the folders under it.
    pub cwd: String,
    pub tool: String,
    #[serde(flatten)]
    pub what: What,
    pub created: i64,
}

#[derive(Serialize, Deserialize, Default)]
struct File {
    schema_version: u32,
    rules: Vec<Rule>,
}

/// A request whose card is up, or about to be answered by a rule.
struct Waiting {
    /// The rule that covers it, by id.
    rule: Option<String>,
    /// The rule "Don't ask again" would make.
    offer: Option<Rule>,
}

pub struct Rules {
    path: PathBuf,
    rules: Vec<Rule>,
    waiting: HashMap<String, Waiting>,
}

// ── Reading a request ─────────────────────────────────────────────────────────

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn agent_of(payload: &Value) -> Option<&str> {
    let agent = text(payload, "ciel_agent");
    AGENTS.contains(&agent).then_some(agent)
}

fn input_of(payload: &Value) -> &Value {
    payload.get("tool_input").unwrap_or(&Value::Null)
}

fn command(input: &Value) -> Option<&str> {
    let line = text(input, "command").trim();
    (!line.is_empty() && line.len() <= MAX_TEXT).then_some(line)
}

/// Nothing in the line can start a second command, run one inside it, or send
/// its output somewhere: no `;`, `&`, `|`, backtick, `$`, redirection, bracket
/// or line break. Stricter than a shell needs, and that is the point: a line
/// that fails only gets its card back.
fn plain(line: &str) -> bool {
    !line.chars().any(|c| {
        matches!(c, ';' | '&' | '|' | '`' | '$' | '<' | '>' | '(' | ')' | '{' | '}' | '\n' | '\r' | '\0')
    })
}

/// `npm test` starts `npm test` and `npm test -- one`, not `npm testing`.
fn starts(line: &str, prefix: &str) -> bool {
    line == prefix || line.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(' '))
}

/// The words the agent itself would make a rule of for this request: Claude
/// Code's `permission_suggestions`, `{ toolName: "Bash", ruleContent:
/// "npm test:*" }`. Only a single rule for this tool that ends in a wildcard.
fn suggested_prefix(payload: &Value, tool: &str) -> Option<String> {
    payload.get("permission_suggestions")?.as_array()?.iter().find_map(|entry| {
        if text(entry, "type") != "addRules" || text(entry, "behavior") != "allow" {
            return None;
        }
        let [rule] = entry.get("rules")?.as_array()?.as_slice() else { return None };
        if text(rule, "toolName") != tool {
            return None;
        }
        let content = text(rule, "ruleContent");
        let prefix = content.strip_suffix(":*").or_else(|| content.strip_suffix(" *"))?.trim();
        (!prefix.is_empty() && prefix.len() <= MAX_TEXT && !prefix.contains('*') && plain(prefix))
            .then(|| prefix.to_string())
    })
}

fn host(input: &Value) -> Option<String> {
    let url = reqwest::Url::parse(text(input, "url")).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    Some(url.host_str()?.to_ascii_lowercase())
}

fn whole_tool(tool: &str) -> bool {
    tool == "WebSearch" || tool.starts_with("mcp__")
}

// ── Folders ───────────────────────────────────────────────────────────────────

/// A path as the names along it, to compare two of them: either slash, no
/// trailing one, lower case on Windows. None unless it is absolute and has no
/// `.` or `..` in it.
fn parts(path: &str) -> Option<Vec<String>> {
    let path = path.trim().replace('\\', "/");
    let bytes = path.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    if !path.starts_with('/') && !drive {
        return None;
    }
    let mut names = Vec::new();
    for name in path.split('/') {
        match name {
            "" => {}
            "." | ".." => return None,
            name if cfg!(windows) => names.push(name.to_lowercase()),
            name => names.push(name.to_string()),
        }
    }
    (!names.is_empty()).then_some(names)
}

fn inside(path: &[String], root: &[String]) -> bool {
    path.len() >= root.len() && path[..root.len()] == *root
}

/// The file an edit writes is in the project: under `root` as written, with no
/// hidden folder or file on the way (`.git`, `.claude`, `.env`…), and still
/// there once links are followed.
fn edit_in(input: &Value, root: &str) -> bool {
    let file = ["file_path", "notebook_path"].into_iter().map(|key| text(input, key)).find(|p| !p.is_empty());
    let (Some(file), Some(root_names)) = (file, parts(root)) else { return false };
    let Some(names) = parts(file) else { return false };
    if names.len() <= root_names.len() || !inside(&names, &root_names) {
        return false;
    }
    if names[root_names.len()..].iter().any(|name| name.starts_with('.')) {
        return false;
    }
    resolves_inside(Path::new(file), Path::new(root))
}

/// Links followed, the file — or, when it does not exist yet, the nearest
/// folder that does — is under the project folder, and not in a hidden one.
fn resolves_inside(file: &Path, root: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(root) else { return false };
    let mut at = Some(file);
    while let Some(path) = at {
        match std::fs::canonicalize(path) {
            Ok(real) => {
                return real.strip_prefix(&root).is_ok_and(|rest| {
                    rest.components().all(|c| !c.as_os_str().to_string_lossy().starts_with('.'))
                });
            }
            // There, but leading nowhere (a link to a file not made yet): the
            // write would land wherever it points.
            Err(_) if path.symlink_metadata().is_ok() => return false,
            Err(_) => at = path.parent(),
        }
    }
    false
}

// ── What a request is, and what covers it ─────────────────────────────────────

/// The rule "Don't ask again" would make for this request: its tool and what
/// of it is remembered. None when there is nothing safe to remember.
pub fn offer(payload: &Value) -> Option<(String, What)> {
    let agent = agent_of(payload)?;
    let tool = text(payload, "tool_name");
    let cwd = text(payload, "cwd");
    if tool.is_empty() || NEVER.contains(&tool) || parts(cwd).is_none() {
        return None;
    }
    let input = input_of(payload);
    let what = if EDIT_TOOLS.contains(&tool) {
        (agent.is_empty() && edit_in(input, cwd)).then_some(What::Edits)?
    } else if let Some(line) = command(input) {
        match suggested_prefix(payload, tool).filter(|prefix| plain(line) && starts(line, prefix)) {
            Some(prefix) => What::Prefix(prefix),
            None => What::Exact(line.to_string()),
        }
    } else if tool == "WebFetch" {
        What::Domain(host(input)?)
    } else if whole_tool(tool) {
        What::Tool
    } else {
        return None;
    };
    Some((tool.to_string(), what))
}

/// True when `rule` answers this request: same agent, same tool, in the rule's
/// folder or under it, and the request is what the rule remembers.
pub fn covers(rule: &Rule, payload: &Value) -> bool {
    let Some(agent) = agent_of(payload) else { return false };
    let tool = text(payload, "tool_name");
    if agent != rule.agent || tool != rule.tool || NEVER.contains(&tool) {
        return false;
    }
    let (Some(at), Some(root)) = (parts(text(payload, "cwd")), parts(&rule.cwd)) else { return false };
    if !inside(&at, &root) {
        return false;
    }
    let input = input_of(payload);
    match &rule.what {
        What::Exact(line) => command(input) == Some(line.as_str()),
        What::Prefix(prefix) => command(input).is_some_and(|line| plain(line) && starts(line, prefix)),
        What::Domain(name) => tool == "WebFetch" && host(input).as_deref() == Some(name.as_str()),
        What::Tool => whole_tool(tool),
        What::Edits => agent.is_empty() && EDIT_TOOLS.contains(&tool) && edit_in(input, &rule.cwd),
    }
}

// ── The store ─────────────────────────────────────────────────────────────────

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_id(now: i64) -> String {
    format!("{now:x}-{:x}-{:x}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

impl Rules {
    /// Reads `path`. A file that cannot be used is renamed aside and no rule
    /// holds: every request gets its card.
    pub fn load(path: PathBuf) -> Self {
        Self { rules: read_rules(&path), path, waiting: HashMap::new() }
    }

    pub fn list(&self) -> &[Rule] {
        &self.rules
    }

    fn matching(&self, payload: &Value) -> Option<&Rule> {
        self.rules.iter().find(|rule| covers(rule, payload))
    }

    /// A permission request arrives. What the island needs to know of it:
    /// `ciel_rule`, the rule that answers it, or `ciel_remember`, the rule
    /// "Don't ask again" would make. Kept here under the request's id, so the
    /// island only ever names a request, never a rule of its own making.
    pub fn note(&mut self, request_id: &str, payload: &Value, now: i64) -> Option<(&'static str, Value)> {
        if self.waiting.len() >= MAX_WAITING {
            self.waiting.clear();
        }
        if let Some(rule) = self.matching(payload) {
            let (id, view) = (rule.id.clone(), json!(rule));
            self.waiting.insert(request_id.to_string(), Waiting { rule: Some(id), offer: None });
            return Some(("ciel_rule", view));
        }
        let (tool, what) = offer(payload)?;
        let rule = Rule {
            id: new_id(now),
            agent: text(payload, "ciel_agent").to_string(),
            cwd: text(payload, "cwd").to_string(),
            tool,
            what,
            created: now,
        };
        let view = json!(rule);
        self.waiting.insert(request_id.to_string(), Waiting { rule: None, offer: Some(rule) });
        Some(("ciel_remember", view))
    }

    /// The click on "Don't ask again": the rule offered for this request is
    /// kept. False when none was, and the click is then a plain Allow.
    pub fn remember(&mut self, request_id: &str) -> bool {
        let Some(rule) = self.waiting.remove(request_id).and_then(|w| w.offer) else { return false };
        if self.rules.len() >= MAX_RULES {
            log::line("rules: the list is full, nothing remembered".to_string());
            return false;
        }
        self.rules.push(rule);
        self.save();
        true
    }

    /// True when the rule found for this request as it came in is still on the
    /// list — it may have been removed in Settings since.
    pub fn answers(&mut self, request_id: &str) -> bool {
        let Some(id) = self.waiting.remove(request_id).and_then(|w| w.rule) else { return false };
        self.rules.iter().any(|rule| rule.id == id)
    }

    pub fn forget(&mut self, request_id: &str) {
        self.waiting.remove(request_id);
    }

    pub fn remove(&mut self, id: &str) {
        let before = self.rules.len();
        self.rules.retain(|rule| rule.id != id);
        if self.rules.len() != before {
            self.save();
        }
    }

    pub fn clear(&mut self) {
        if !self.rules.is_empty() {
            self.rules.clear();
            self.save();
        }
    }

    fn save(&self) {
        if let Err(err) = write_rules(&self.path, &self.rules) {
            log::line(format!("rules: could not save: {err}"));
        }
    }
}

fn read_rules(path: &Path) -> Vec<Rule> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
        set_aside(path);
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    match serde_json::from_slice::<File>(&bytes) {
        Ok(mut file) => {
            file.rules.truncate(MAX_RULES);
            file.rules
        }
        Err(_) => {
            set_aside(path);
            Vec::new()
        }
    }
}

/// A file Ciel cannot read is kept under another name, not written over.
fn set_aside(path: &Path) {
    let _ = std::fs::rename(path, path.with_extension("json.unreadable"));
}

/// Written whole to a temporary file, then renamed over the old one.
fn write_rules(path: &Path, rules: &[Rule]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(&File { schema_version: 1, rules: rules.to_vec() })?;
    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, path)
}

// ── The app's side ────────────────────────────────────────────────────────────

pub struct Store(Mutex<Rules>);

impl Store {
    fn lock(&self) -> MutexGuard<'_, Rules> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn load() -> Store {
    Store(Mutex::new(Rules::load(platform::local_dir().join("rules.json"))))
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Called by the relay server as a permission request comes in, before the
/// island hears of it: tags the payload for the island (see `Rules::note`).
pub fn note_request(app: &AppHandle, request_id: &str, payload: &mut Value) {
    let Some(store) = app.try_state::<Store>() else { return };
    let tag = store.lock().note(request_id, payload, now());
    if let Some((key, view)) = tag {
        payload[key] = view;
    }
}

/// "Don't ask again" was clicked on this request's card.
pub fn remember(app: &AppHandle, request_id: &str) {
    if app.try_state::<Store>().is_some_and(|store| store.lock().remember(request_id)) {
        log::line(format!("rules: one remembered from id={request_id}"));
    }
}

/// Whether a rule answers this request, asked once as the island passes it on.
pub fn answers(app: &AppHandle, request_id: &str) -> bool {
    app.try_state::<Store>().is_some_and(|store| store.lock().answers(request_id))
}

pub fn forget(app: &AppHandle, request_id: &str) {
    if let Some(store) = app.try_state::<Store>() {
        store.lock().forget(request_id);
    }
}

#[tauri::command]
pub fn rules_list(store: State<Store>) -> Vec<Rule> {
    store.lock().list().to_vec()
}

#[tauri::command]
pub fn rules_remove(store: State<Store>, id: String) {
    store.lock().remove(&id);
}

#[tauri::command]
pub fn rules_clear(store: State<Store>) {
    store.lock().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ciel-rules-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bash(cwd: &str, line: &str) -> Value {
        json!({ "hook_event_name": "PermissionRequest", "cwd": cwd, "tool_name": "Bash", "tool_input": { "command": line } })
    }

    /// The same, with the rule Claude Code suggests for it.
    fn suggested(cwd: &str, line: &str, content: &str) -> Value {
        let mut payload = bash(cwd, line);
        payload["permission_suggestions"] = json!([{
            "type": "addRules", "behavior": "allow", "destination": "localSettings",
            "rules": [{ "toolName": "Bash", "ruleContent": content }],
        }]);
        payload
    }

    fn rule(cwd: &str, tool: &str, what: What) -> Rule {
        Rule { id: "r".into(), agent: String::new(), cwd: cwd.into(), tool: tool.into(), what, created: T0 }
    }

    #[test]
    fn a_command_is_remembered_word_for_word_without_a_suggestion() {
        let (tool, what) = offer(&bash("/home/me/app", "  cargo build  ")).unwrap();
        assert_eq!((tool.as_str(), &what), ("Bash", &What::Exact("cargo build".into())));
        let rule = rule("/home/me/app", "Bash", what);
        assert!(covers(&rule, &bash("/home/me/app", "cargo build")));
        assert!(!covers(&rule, &bash("/home/me/app", "cargo build --release")));
        assert!(!covers(&rule, &bash("/home/me/app", "cargo build && rm -rf /")));
    }

    #[test]
    fn the_agents_own_suggestion_makes_a_rule_for_what_follows() {
        for content in ["npm test:*", "npm test *"] {
            let (_, what) = offer(&suggested("/home/me/app", "npm test -- one", content)).unwrap();
            assert_eq!(what, What::Prefix("npm test".into()), "{content}");
        }
        let rule = rule("/home/me/app", "Bash", What::Prefix("npm test".into()));
        assert!(covers(&rule, &bash("/home/me/app", "npm test")));
        assert!(covers(&rule, &bash("/home/me/app", "npm test -- two --watch=false")));
        assert!(!covers(&rule, &bash("/home/me/app", "npm testing")));
        assert!(!covers(&rule, &bash("/home/me/app", "FOO=1 npm test")));
    }

    #[test]
    fn nothing_that_chains_substitutes_or_redirects_passes_a_prefix() {
        let rule = rule("/home/me/app", "Bash", What::Prefix("npm test".into()));
        for line in [
            "npm test; rm -rf ~", "npm test && curl x", "npm test || true", "npm test | sh",
            "npm test & disown", "npm test `id`", "npm test $(id)", "npm test $HOME",
            "npm test > ~/.bashrc", "npm test < /etc/passwd", "npm test\nrm -rf ~",
            "npm test (x)", "npm test {a,b}",
        ] {
            assert!(!covers(&rule, &bash("/home/me/app", line)), "{line:?}");
        }
    }

    #[test]
    fn a_suggestion_that_does_not_fit_the_command_falls_back_to_the_command_itself() {
        // The command chains: the words suggested would not cover it again.
        let (_, what) = offer(&suggested("/home/me/app", "npm test && npm run lint", "npm test:*")).unwrap();
        assert_eq!(what, What::Exact("npm test && npm run lint".into()));
        // The suggestion is for other words, another tool, several rules, or no wildcard.
        let (_, what) = offer(&suggested("/home/me/app", "cargo test", "npm test:*")).unwrap();
        assert_eq!(what, What::Exact("cargo test".into()));
        let mut other = suggested("/home/me/app", "npm test", "npm test:*");
        other["permission_suggestions"][0]["rules"][0]["toolName"] = json!("PowerShell");
        assert_eq!(offer(&other).unwrap().1, What::Exact("npm test".into()));
        let mut two = suggested("/home/me/app", "npm test", "npm test:*");
        two["permission_suggestions"][0]["rules"] = json!([
            { "toolName": "Bash", "ruleContent": "npm test:*" }, { "toolName": "Bash", "ruleContent": "rm:*" },
        ]);
        assert_eq!(offer(&two).unwrap().1, What::Exact("npm test".into()));
        for content in ["npm test", "npm * test:*", ":*", "npm test; x:*"] {
            let (_, what) = offer(&suggested("/home/me/app", "npm test", content)).unwrap();
            assert_eq!(what, What::Exact("npm test".into()), "{content}");
        }
        // A deny or ask suggestion is never an allow rule.
        let mut deny = suggested("/home/me/app", "npm test", "npm test:*");
        deny["permission_suggestions"][0]["behavior"] = json!("deny");
        assert_eq!(offer(&deny).unwrap().1, What::Exact("npm test".into()));
    }

    #[test]
    fn a_rule_holds_for_its_agent_its_tool_and_its_folder_only() {
        let rule = rule("/home/me/app", "Bash", What::Exact("cargo build".into()));
        assert!(covers(&rule, &bash("/home/me/app/", "cargo build")));
        assert!(covers(&rule, &bash("/home/me/app/crates/core", "cargo build")));
        assert!(!covers(&rule, &bash("/home/me/application", "cargo build")));
        assert!(!covers(&rule, &bash("/home/me", "cargo build")));
        assert!(!covers(&rule, &bash("/home/me/app/../other", "cargo build")));
        assert!(!covers(&rule, &bash("", "cargo build")));
        assert!(!covers(&rule, &bash("app", "cargo build")));

        let mut codex = bash("/home/me/app", "cargo build");
        codex["ciel_agent"] = json!("codex");
        assert!(!covers(&rule, &codex));
        let mut shell = bash("/home/me/app", "cargo build");
        shell["tool_name"] = json!("PowerShell");
        assert!(!covers(&rule, &shell));
    }

    #[test]
    fn only_the_agents_the_island_answers_for_get_a_rule() {
        for agent in ["codex", "copilot", "muse"] {
            let mut payload = bash("/home/me/app", "ls");
            payload["ciel_agent"] = json!(agent);
            assert!(offer(&payload).is_some(), "{agent}");
        }
        for agent in ["ciel", "gemini", "cursor", "claude", "Codex"] {
            let mut payload = bash("/home/me/app", "ls");
            payload["ciel_agent"] = json!(agent);
            assert!(offer(&payload).is_none(), "{agent}");
            let mut made = rule("/home/me/app", "Bash", What::Exact("ls".into()));
            made.agent = agent.into();
            assert!(!covers(&made, &payload), "{agent}");
        }
    }

    #[test]
    fn a_question_and_a_plan_are_never_remembered() {
        for tool in NEVER {
            let payload = json!({ "cwd": "/home/me/app", "tool_name": tool, "tool_input": { "command": "x" } });
            assert!(offer(&payload).is_none(), "{tool}");
            for what in [What::Tool, What::Exact("x".into()), What::Edits] {
                assert!(!covers(&rule("/home/me/app", tool, what), &payload), "{tool}");
            }
        }
        // Nor a tool Ciel knows nothing about, or a command too long to show.
        let unknown = json!({ "cwd": "/home/me/app", "tool_name": "Agent", "tool_input": { "prompt": "x" } });
        assert!(offer(&unknown).is_none());
        assert!(offer(&bash("/home/me/app", &"x".repeat(MAX_TEXT + 1))).is_none());
        assert!(offer(&bash("/home/me/app", "   ")).is_none());
    }

    #[test]
    fn a_fetch_is_remembered_by_its_host() {
        let fetch = |url: &str| json!({ "cwd": "/home/me/app", "tool_name": "WebFetch", "tool_input": { "url": url } });
        let (tool, what) = offer(&fetch("https://Docs.Rs/serde/latest")).unwrap();
        assert_eq!((tool.as_str(), &what), ("WebFetch", &What::Domain("docs.rs".into())));
        let rule = rule("/home/me/app", "WebFetch", what);
        assert!(covers(&rule, &fetch("https://docs.rs/tokio")));
        assert!(covers(&rule, &fetch("http://docs.rs:8080/x")));
        assert!(!covers(&rule, &fetch("https://docs.rs.evil.example/x")));
        assert!(!covers(&rule, &fetch("https://evil.example/?docs.rs")));
        assert!(!covers(&rule, &fetch("https://user@evil.example/docs.rs")));
        assert!(!covers(&rule, &fetch("file:///docs.rs/x")));
        assert!(offer(&fetch("not a url")).is_none());
        assert!(offer(&fetch("ftp://docs.rs/x")).is_none());
    }

    #[test]
    fn an_mcp_tool_and_a_web_search_are_remembered_whole() {
        let call = |tool: &str| json!({ "cwd": "/home/me/app", "tool_name": tool, "tool_input": { "query": "x" } });
        assert_eq!(offer(&call("mcp__github__list_issues")).unwrap().1, What::Tool);
        assert_eq!(offer(&call("WebSearch")).unwrap().1, What::Tool);
        let rule = rule("/home/me/app", "mcp__github__list_issues", What::Tool);
        assert!(covers(&rule, &call("mcp__github__list_issues")));
        assert!(!covers(&rule, &call("mcp__github__create_issue")));
        // A rules.json written by hand cannot make a whole-tool rule of a shell.
        let whole = Rule { tool: "Bash".into(), ..rule };
        assert!(!covers(&whole, &bash("/home/me/app", "rm -rf ~")));
    }

    fn edit(cwd: &Path, file: &Path) -> Value {
        json!({
            "cwd": cwd.to_string_lossy(), "tool_name": "Edit",
            "tool_input": { "file_path": file.to_string_lossy(), "old_string": "a", "new_string": "b" },
        })
    }

    #[test]
    fn edits_are_remembered_for_files_in_the_project_only() {
        let root = scratch("edits");
        let outside = scratch("edits-outside");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join("src/a.rs"), "a").unwrap();

        let (tool, what) = offer(&edit(&root, &root.join("src/a.rs"))).unwrap();
        assert_eq!((tool.as_str(), &what), ("Edit", &What::Edits));
        let rule = rule(&root.to_string_lossy(), "Edit", What::Edits);
        assert!(covers(&rule, &edit(&root, &root.join("src/a.rs"))));
        // A file not made yet, in a folder not made yet.
        assert!(covers(&rule, &edit(&root, &root.join("src/new/b.rs"))));
        // The session moved into a folder of the project: still the project.
        assert!(covers(&rule, &edit(&root.join("src"), &root.join("src/a.rs"))));

        assert!(!covers(&rule, &edit(&root, &outside.join("a.rs"))));
        assert!(!covers(&rule, &edit(&root, &root.join(".git/hooks/pre-commit"))));
        assert!(!covers(&rule, &edit(&root, &root.join(".env"))));
        assert!(!covers(&rule, &edit(&root, &root.join("src/../../x"))));
        assert!(!covers(&rule, &edit(&root, Path::new("src/a.rs"))));
        assert!(!covers(&rule, &edit(&root, &root)));
        assert!(offer(&edit(&root, &outside.join("a.rs"))).is_none());
        assert!(offer(&edit(&root, &root.join(".claude/settings.json"))).is_none());

        // Another tool of the same rule kind is another rule; Write is not Edit.
        let mut write = edit(&root, &root.join("src/a.rs"));
        write["tool_name"] = json!("Write");
        assert!(!covers(&rule, &write));
        // Only Claude Code's edits: another agent's tool of that name gets no rule.
        let mut codex = edit(&root, &root.join("src/a.rs"));
        codex["ciel_agent"] = json!("codex");
        assert!(offer(&codex).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn an_edit_through_a_link_out_of_the_project_is_not_covered() {
        let root = scratch("links");
        let outside = scratch("links-outside");
        std::fs::write(outside.join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("file")).unwrap();
        std::os::unix::fs::symlink(outside.join("not-yet"), root.join("dangling")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::os::unix::fs::symlink(root.join(".git"), root.join("plain")).unwrap();

        let rule = rule(&root.to_string_lossy(), "Edit", What::Edits);
        for file in ["out/a.rs", "file", "dangling", "plain/config"] {
            assert!(!covers(&rule, &edit(&root, &root.join(file))), "{file}");
        }
    }

    #[test]
    fn a_request_is_tagged_with_its_rule_or_with_the_one_it_could_make() {
        let dir = scratch("note");
        let mut rules = Rules::load(dir.join("rules.json"));

        let (key, view) = rules.note("r1", &suggested("/home/me/app", "npm test", "npm test:*"), T0).unwrap();
        assert_eq!(key, "ciel_remember");
        assert_eq!((view["kind"].as_str(), view["text"].as_str(), view["tool"].as_str()),
            (Some("prefix"), Some("npm test"), Some("Bash")));
        // Nothing answers before the click, and only the request that was offered a rule makes one.
        assert!(!rules.answers("r1"));
        assert!(!rules.remember("r1"));
        rules.note("r2", &suggested("/home/me/app", "npm test", "npm test:*"), T0);
        assert!(!rules.remember("unknown"));
        assert!(rules.remember("r2"));
        assert!(!rules.remember("r2"));
        assert_eq!(rules.list().len(), 1);

        let (key, view) = rules.note("r3", &bash("/home/me/app", "npm test -- one"), T0 + 5).unwrap();
        assert_eq!(key, "ciel_rule");
        assert_eq!(view["id"], json!(rules.list()[0].id));
        assert!(rules.answers("r3"));
        // Once: the same id cannot be answered twice.
        assert!(!rules.answers("r3"));
        // A request a rule covers is not offered another.
        assert!(!rules.remember("r3"));

        // Nothing to remember, nothing to tag.
        assert!(rules.note("r4", &json!({ "cwd": "/home/me/app", "tool_name": "AskUserQuestion" }), T0).is_none());
    }

    #[test]
    fn a_rule_removed_while_its_request_waits_no_longer_answers() {
        let dir = scratch("removed");
        let mut rules = Rules::load(dir.join("rules.json"));
        rules.note("r1", &bash("/home/me/app", "ls"), T0);
        assert!(rules.remember("r1"));
        let id = rules.list()[0].id.clone();

        rules.note("r2", &bash("/home/me/app", "ls"), T0);
        rules.remove(&id);
        assert!(!rules.answers("r2"));
        assert!(rules.note("r3", &bash("/home/me/app", "ls"), T0).is_some_and(|(key, _)| key == "ciel_remember"));

        rules.note("r4", &bash("/home/me/app", "ls"), T0);
        rules.forget("r4");
        assert!(!rules.remember("r4"));
    }

    #[test]
    fn rules_survive_a_restart_and_a_broken_file_holds_none() {
        let dir = scratch("disk");
        let path = dir.join("rules.json");
        let mut rules = Rules::load(path.clone());
        rules.note("r1", &bash("/home/me/app", "ls"), T0);
        rules.note("r2", &json!({ "cwd": "/home/me/app", "tool_name": "WebSearch", "tool_input": {} }), T0);
        assert!(rules.remember("r1") && rules.remember("r2"));

        let again = Rules::load(path.clone());
        assert_eq!(again.list(), rules.list());
        assert!(again.matching(&bash("/home/me/app", "ls")).is_some());

        let mut again = again;
        again.clear();
        assert!(Rules::load(path.clone()).list().is_empty());

        std::fs::write(&path, b"{ not json").unwrap();
        assert!(Rules::load(path.clone()).list().is_empty());
        assert!(!path.exists());
        assert!(path.with_extension("json.unreadable").exists());
    }

    #[test]
    fn the_list_stops_growing_at_its_cap() {
        let dir = scratch("cap");
        let mut rules = Rules::load(dir.join("rules.json"));
        for n in 0..MAX_RULES {
            rules.rules.push(rule("/home/me/app", "Bash", What::Exact(format!("echo {n}"))));
        }
        rules.note("r", &bash("/home/me/app", "ls"), T0);
        assert!(!rules.remember("r"));
        assert_eq!(rules.list().len(), MAX_RULES);
    }
}
