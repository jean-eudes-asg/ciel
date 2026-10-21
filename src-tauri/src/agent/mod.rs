// « OK Ciel » through the user's own Claude Code: the brain becomes an agent.
// Ciel starts the `claude` the user installed and signed in to themselves —
// it never reads their credentials and never signs them in — headless, for
// one conversation at a time:
//
// - what it may use: its web search and reading files, and, each time after
//   a click in the island, fetching a page, running a command or writing a
//   file, in a folder of Ciel's own (`Agent`);
// - Ciel's own tools (calendar, notes, pills, music…), served by
//   `ciel-hook --mcp` (hook/src/mcp.rs) and answered here, with only what the
//   user lets Claude read (voice/mod.rs `may_read`);
// - its permission requests reach the island through a PermissionRequest hook
//   of its own (`ciel-hook --agent ciel`), as Codex's do: no click, no.
//
// None of the user's settings, hooks or MCP servers are loaded, nothing of
// the conversation is kept, and the process ends 90 s after the last answer.
//
// The part that starts Claude Code (session.rs) is built only with the
// `claude-code` feature, and is off by default even then: whether an app may
// run the user's own Claude Code is Anthropic's to say. Without it, `wanted`
// is always false and nothing here runs but `mcp_reply`, which answers no one.

// Without the feature, the command line is built by the tests alone.
#![cfg_attr(not(feature = "claude-code"), allow(dead_code))]

#[cfg(feature = "claude-code")]
mod session;
pub mod stream;

use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::voice::brain::Reply;

/// The model, and how hard it thinks: a voice answer has to come quickly.
pub const MODEL: &str = "sonnet";
pub const EFFORT: &str = "low";
/// Tool rounds in one answer.
pub const MAX_TURNS: u32 = 8;
/// The PermissionRequest hook's timeout, as for Claude Code's own (hooks.rs).
const APPROVAL_TIMEOUT: u32 = 120;

/// What the voice page shows of the engine.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This build has the engine.
    pub built: bool,
    /// Where `claude` is, once found.
    pub path: Option<String>,
    /// `claude --version`.
    pub version: Option<String>,
    /// `claude auth status` says the user is signed in.
    pub signed_in: bool,
}

#[tauri::command]
pub async fn agent_status() -> Status {
    #[cfg(feature = "claude-code")]
    {
        tauri::async_runtime::spawn_blocking(session::status).await.unwrap_or_default()
    }
    #[cfg(not(feature = "claude-code"))]
    {
        Status::default()
    }
}

/// « OK Ciel » should go through Claude Code: the user turned it on, in a
/// build that has it, and their `claude` is there and signed in.
pub fn wanted(app: &AppHandle) -> bool {
    #[cfg(feature = "claude-code")]
    {
        session::wanted(app)
    }
    #[cfg(not(feature = "claude-code"))]
    {
        let _ = app;
        false
    }
}

/// One turn of the conversation; None when Claude Code couldn't answer (the
/// usual brain then does).
pub fn ask(app: &AppHandle, question: &str) -> Option<Reply> {
    #[cfg(feature = "claude-code")]
    {
        session::ask(app, question)
    }
    #[cfg(not(feature = "claude-code"))]
    {
        let _ = (app, question);
        None
    }
}

/// « OK Ciel » was heard: Claude Code starts while the question comes.
pub fn warm_up(app: &AppHandle) {
    #[cfg(feature = "claude-code")]
    session::warm_up(app);
    #[cfg(not(feature = "claude-code"))]
    let _ = app;
}

/// Ciel quits: so does Claude Code.
pub fn shutdown() {
    #[cfg(feature = "claude-code")]
    session::stop();
}

// ── The command line ──────────────────────────────────────────────────────────

/// Claude Code's own tools the brain gets: what reads, and, behind a click
/// each time, what acts.
fn builtin_tools(windows: bool) -> String {
    let mut tools = vec!["WebSearch", "WebFetch", "Read", "Glob", "Grep", "Bash", "Write", "Edit"];
    if windows {
        tools.push("PowerShell");
    }
    tools.join(",")
}

/// The tools that run without asking: reading, searching the web, and Ciel's
/// own (which read only what the user allowed). Everything else asks.
const ALLOWED: &str = "WebSearch,Read,Glob,Grep,mcp__ciel__*";

/// The arguments for `claude`, with Ciel's MCP configuration (a file), the
/// settings that add the approval hook, and Ciel's instructions.
pub fn arguments(mcp_config: &str, settings: &str, prompt: &str, windows: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        MODEL,
        "--effort",
        EFFORT,
        "--permission-mode",
        "default",
        "--strict-mcp-config",
        "--mcp-config",
        mcp_config,
        // Not the user's settings (their hooks, Ciel's relay among them, would
        // fire in here) and no project: Ciel's own folder has none.
        "--setting-sources",
        "project",
        "--settings",
        settings,
        "--allowedTools",
        ALLOWED,
        "--no-session-persistence",
        "--append-system-prompt",
        prompt,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(["--tools".to_string(), builtin_tools(windows)]);
    args.extend(["--max-turns".to_string(), MAX_TURNS.to_string()]);
    args
}

/// The settings the brain's Claude Code gets: the one hook that brings its
/// permission requests to the island. `relay` is the relay's command line.
pub fn approval_settings(relay: &str) -> String {
    json!({
        "hooks": {
            "PermissionRequest": [{
                "hooks": [{ "type": "command", "command": relay, "timeout": APPROVAL_TIMEOUT }]
            }]
        }
    })
    .to_string()
}

/// The MCP configuration: Ciel's tools, from the relay, with this session's token.
pub fn mcp_config(relay_exe: &str, token: &str) -> String {
    json!({
        "mcpServers": {
            "ciel": { "type": "stdio", "command": relay_exe, "args": ["--mcp"], "env": { "CIEL_MCP_TOKEN": token } }
        }
    })
    .to_string()
}

// ── Ciel's tools, for `ciel-hook --mcp` ───────────────────────────────────────

/// The token of the Claude Code Ciel runs now; None when none runs.
static TOKEN: Mutex<Option<String>> = Mutex::new(None);

fn set_token(token: Option<String>) {
    *TOKEN.lock().unwrap() = token;
}

/// Compared in constant time.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn token_ok(given: &str) -> bool {
    TOKEN.lock().unwrap().as_deref().is_some_and(|t| !t.is_empty() && same(t.as_bytes(), given.as_bytes()))
}

/// Ciel's tools as MCP lists them: the brain's own but the web search (Claude
/// Code has its own), with what the user lets Claude read.
pub fn mcp_tools(settings: &crate::settings::Settings) -> Vec<Value> {
    crate::voice::brain::agent_tools(settings)
        .into_iter()
        .filter_map(|t| {
            Some(json!({ "name": t.get("name")?, "description": t.get("description")?, "inputSchema": t.get("input_schema")? }))
        })
        .collect()
}

/// Ciel's answer to `ciel-hook --mcp` (pipe.rs): its tools, or what one did.
/// None for anyone but the Claude Code Ciel started; the relay then says the
/// tool didn't run.
pub fn mcp_reply(app: &AppHandle, payload: &Value) -> Option<String> {
    let given = payload.get("ciel_mcp_token").and_then(Value::as_str).unwrap_or("");
    if !token_ok(given) {
        crate::log::line("agent: a tool call without the session's token, refused");
        return None;
    }
    let request = payload.get("ciel_mcp")?;
    let settings = app.try_state::<crate::Shared>()?.settings.lock().unwrap().clone();
    let tools = mcp_tools(&settings);
    match request.get("method").and_then(Value::as_str)? {
        "tools/list" => Some(json!({ "tools": tools }).to_string()),
        "tools/call" => {
            let name = request.pointer("/params/name").and_then(Value::as_str)?;
            let args = request.pointer("/params/arguments").cloned().unwrap_or_else(|| json!({}));
            if !tools.iter().any(|t| t["name"] == name) {
                return Some(json!({ "content": [{ "type": "text", "text": format!("No tool {name}.") }], "isError": true }).to_string());
            }
            let (text, _) = crate::voice::run_tool(app, name, &args, true);
            Some(json!({ "content": [{ "type": "text", "text": text }], "isError": false }).to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_loads_nothing_of_the_users_and_asks_before_acting() {
        let args = arguments("C:/Ciel/Agent/mcp.json", "{\"hooks\":{}}", "Be brief.", true);
        let value = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str());
        assert_eq!(args[0], "-p");
        assert_eq!(value("--input-format"), Some("stream-json"));
        assert_eq!(value("--output-format"), Some("stream-json"));
        assert!(args.iter().any(|a| a == "--verbose"));
        assert!(args.iter().any(|a| a == "--strict-mcp-config"));
        assert_eq!(value("--mcp-config"), Some("C:/Ciel/Agent/mcp.json"));
        assert_eq!(value("--setting-sources"), Some("project"));
        assert_eq!(value("--permission-mode"), Some("default"));
        assert_eq!(value("--tools"), Some("WebSearch,WebFetch,Read,Glob,Grep,Bash,Write,Edit,PowerShell"));
        assert!(!arguments("m", "s", "p", false).iter().any(|a| a.contains("PowerShell")));
        // What acts is never allowed ahead: it asks, in the island.
        let allowed = value("--allowedTools").unwrap();
        for acts in ["Bash", "Write", "Edit", "WebFetch", "PowerShell"] {
            assert!(!allowed.split(',').any(|t| t == acts), "{acts}");
        }
        assert!(allowed.split(',').any(|t| t == "mcp__ciel__*"));
        for never in ["--bare", "--dangerously-skip-permissions", "bypassPermissions", "acceptEdits", "--resume", "--continue"] {
            assert!(!args.iter().any(|a| a == never), "{never}");
        }
        assert!(args.iter().any(|a| a == "--no-session-persistence"));
        assert_eq!(value("--append-system-prompt"), Some("Be brief."));
    }

    #[test]
    fn the_approval_hook_and_the_tools_server_are_ciels_relay() {
        let settings: Value = serde_json::from_str(&approval_settings("'/home/me/.local/share/ciel/bin/ciel-hook' --agent ciel PermissionRequest")).unwrap();
        let hook = &settings["hooks"]["PermissionRequest"][0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert!(hook["command"].as_str().unwrap().ends_with("--agent ciel PermissionRequest"));
        assert_eq!(hook["timeout"], 120);
        assert_eq!(settings["hooks"].as_object().unwrap().len(), 1, "no other hook");

        let mcp: Value = serde_json::from_str(&mcp_config("C:\\Ciel\\bin\\ciel-hook.exe", "abc")).unwrap();
        assert_eq!(mcp["mcpServers"]["ciel"]["command"], "C:\\Ciel\\bin\\ciel-hook.exe");
        assert_eq!(mcp["mcpServers"]["ciel"]["args"], json!(["--mcp"]));
        assert_eq!(mcp["mcpServers"]["ciel"]["env"]["CIEL_MCP_TOKEN"], "abc");
    }

    #[test]
    fn only_the_session_s_token_reaches_ciels_tools() {
        set_token(None);
        assert!(!token_ok(""));
        assert!(!token_ok("abc"));
        set_token(Some("abc".into()));
        assert!(token_ok("abc"));
        assert!(!token_ok("abd") && !token_ok("ab") && !token_ok(""));
        set_token(None);
        assert!(!token_ok("abc"));
    }

    #[test]
    fn mcp_lists_ciels_tools_but_the_web_search_and_only_what_claude_may_read() {
        let mut settings = crate::settings::Settings::default();
        let names = |s: &crate::settings::Settings| mcp_tools(s).iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        let base = names(&settings);
        assert!(base.iter().any(|n| n == "pills") && base.iter().any(|n| n == "notes"));
        assert!(!base.iter().any(|n| n == "web_search" || n == "calendar" || n == "finance"));
        assert!(mcp_tools(&settings).iter().all(|t| t["inputSchema"]["type"] == "object"));
        settings.active_integrations.push(crate::calendar::PILL_ID.into());
        settings.active_integrations.push(crate::finance::PILL_ID.into());
        assert!(names(&settings).iter().any(|n| n == "calendar"));
        assert!(!names(&settings).iter().any(|n| n == "finance"), "off until the user turns it on");
        settings.brain_reads.finance = true;
        assert!(names(&settings).iter().any(|n| n == "finance"));
    }
}
