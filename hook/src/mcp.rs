//! `ciel-hook --mcp`: Ciel's own tools for the brain that runs on the user's
//! Claude Code (src-tauri/src/agent/). Claude Code starts it as a stdio MCP
//! server, from the configuration Ciel gives it; it reads JSON-RPC 2.0 one
//! line at a time and hands every tool question to Ciel over the pipe, like a
//! hook event, then gives Claude Ciel's answer.
//!
//! It knows nothing of the tools: `tools/list` and `tools/call` are Ciel's to
//! answer. Each request carries the token Ciel put in this process's
//! environment (CIEL_MCP_TOKEN), so only the Claude Code Ciel started reaches
//! its tools. When Ciel isn't there or doesn't answer in time, a call fails
//! at once: nothing is left waiting, and nothing runs without Ciel.

use std::io::{BufRead, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};

use super::connect;

/// The protocol version we speak when the client names none we know.
const PROTOCOL: &str = "2025-06-18";
/// How long Ciel has to list its tools, and to run one.
const LIST_BUDGET: Duration = Duration::from_secs(5);
const CALL_BUDGET: Duration = Duration::from_secs(30);
/// A line longer than this is not a request we take.
const MAX_LINE: usize = 1 << 20;

pub fn is_mcp(args: &[String]) -> bool {
    args.iter().any(|a| a == "--mcp")
}

pub fn run() -> ! {
    let token = std::env::var("CIEL_MCP_TOKEN").unwrap_or_default();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.len() > MAX_LINE || line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(reply) = answer(&request, &token, &ask_ciel) {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
    std::process::exit(0);
}

/// Asks Ciel over the pipe, within `budget`: its one-line answer, or None.
fn ask_ciel(payload: &str, budget: Duration) -> Option<String> {
    let payload = payload.to_string();
    let (tx, rx) = mpsc::channel();
    // As for a hook event: the worker owns the blocking calls, and is left
    // behind if it overruns.
    std::thread::spawn(move || {
        let _ = tx.send(talk(&payload));
    });
    rx.recv_timeout(budget).ok().flatten()
}

fn talk(payload: &str) -> Option<String> {
    let mut pipe = connect()?;
    pipe.write_all(payload.as_bytes()).ok()?;
    pipe.write_all(b"\n").ok()?;
    pipe.flush().ok()?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.contains(&b'\n') || buf.len() > MAX_LINE {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let answer = String::from_utf8_lossy(&buf).trim().to_string();
    (!answer.is_empty()).then_some(answer)
}

/// The reply to one JSON-RPC message (None for a notification). `ask` puts a
/// question to Ciel: the line for the pipe and how long to wait.
pub fn answer(request: &Value, token: &str, ask: &dyn Fn(&str, Duration) -> Option<String>) -> Option<String> {
    let id = request.get("id").cloned()?;
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let ok = |result: Value| Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string());
    match method {
        "initialize" => {
            let asked = request.pointer("/params/protocolVersion").and_then(Value::as_str);
            let version = match asked {
                Some(v @ ("2024-11-05" | "2025-03-26" | "2025-06-18")) => v,
                _ => PROTOCOL,
            };
            ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "ciel", "version": env!("CARGO_PKG_VERSION") },
            }))
        }
        "ping" => ok(json!({})),
        "tools/list" => {
            let tools = ask(&question("tools/list", Value::Null, token), LIST_BUDGET)
                .and_then(|a| serde_json::from_str::<Value>(&a).ok())
                .and_then(|v| v.get("tools").cloned())
                .filter(Value::is_array)
                .unwrap_or_else(|| json!([]));
            ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or(Value::Null);
            let result = ask(&question("tools/call", params, token), CALL_BUDGET)
                .and_then(|a| serde_json::from_str::<Value>(&a).ok())
                .filter(|v| v.get("content").is_some_and(Value::is_array))
                .unwrap_or_else(|| {
                    json!({ "content": [{ "type": "text", "text": "Ciel didn't answer: the tool didn't run." }], "isError": true })
                });
            ok(result)
        }
        _ => Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "unknown method" } }).to_string()),
    }
}

/// What goes to Ciel over the pipe.
fn question(method: &str, params: Value, token: &str) -> String {
    json!({ "ciel_mcp": { "method": method, "params": params }, "ciel_mcp_token": token }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn ask_with(reply: Option<&'static str>) -> (impl Fn(&str, Duration) -> Option<String>, std::rc::Rc<RefCell<Vec<String>>>) {
        let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
        let log = asked.clone();
        (move |payload: &str, _| {
            log.borrow_mut().push(payload.to_string());
            reply.map(String::from)
        }, asked)
    }

    fn reply(request: Value, ask: &dyn Fn(&str, Duration) -> Option<String>) -> Value {
        serde_json::from_str(&answer(&request, "t0k", ask).expect("a reply")).unwrap()
    }

    #[test]
    fn it_introduces_itself_without_asking_ciel() {
        let (ask, asked) = ask_with(None);
        let r = reply(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}), &ask);
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "ciel");
        let r = reply(json!({"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "2099-01-01"}}), &ask);
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL);
        assert_eq!(reply(json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}), &ask)["result"], json!({}));
        assert!(answer(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}), "t0k", &ask).is_none());
        assert!(asked.borrow().is_empty());
    }

    #[test]
    fn tools_are_ciels_to_list_and_to_run_with_the_token() {
        let (ask, asked) = ask_with(Some(r#"{"tools":[{"name":"calendar","inputSchema":{"type":"object"}}]}"#));
        let r = reply(json!({"jsonrpc": "2.0", "id": "a", "method": "tools/list"}), &ask);
        assert_eq!(r["id"], "a");
        assert_eq!(r["result"]["tools"][0]["name"], "calendar");
        let sent: Value = serde_json::from_str(&asked.borrow()[0]).unwrap();
        assert_eq!(sent, json!({"ciel_mcp": {"method": "tools/list", "params": null}, "ciel_mcp_token": "t0k"}));

        let (ask, asked) = ask_with(Some(r#"{"content":[{"type":"text","text":"No meetings."}]}"#));
        let r = reply(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "calendar", "arguments": {}}}), &ask);
        assert_eq!(r["result"]["content"][0]["text"], "No meetings.");
        let sent: Value = serde_json::from_str(&asked.borrow()[0]).unwrap();
        assert_eq!(sent["ciel_mcp"]["params"]["name"], "calendar");
    }

    #[test]
    fn without_ciel_nothing_runs_and_nothing_waits() {
        let (ask, _) = ask_with(None);
        assert_eq!(reply(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}), &ask)["result"]["tools"], json!([]));
        let r = reply(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "music"}}), &ask);
        assert_eq!(r["result"]["isError"], true);
        // An answer that isn't a tool result is no answer.
        let (ask, _) = ask_with(Some("not json"));
        assert_eq!(reply(json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {}}), &ask)["result"]["isError"], true);
        let (ask, _) = ask_with(None);
        assert_eq!(reply(json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}), &ask)["error"]["code"], -32601);
    }
}
