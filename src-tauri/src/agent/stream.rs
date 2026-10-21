// Claude Code's stream-json, as `claude -p --output-format stream-json`
// writes it, one JSON object a line, and the user's turn as it reads it on
// stdin. Only what « OK Ciel » needs is read: that it is ready, the tools it
// uses (for the pill), and the turn's result.

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `system` / `init`: the session is up.
    Ready,
    /// The assistant calls a tool.
    ToolUse(String),
    /// The turn is over: what to say, or that it failed.
    Done { text: String, failed: bool },
}

/// The events in one line of the stream (none for a line we don't need).
pub fn parse(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Vec::new() };
    match v["type"].as_str() {
        Some("system") if v["subtype"] == "init" => vec![Event::Ready],
        Some("assistant") => v["message"]["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "tool_use")
                    .filter_map(|b| b["name"].as_str().map(|n| Event::ToolUse(n.to_string())))
                    .collect()
            })
            .unwrap_or_default(),
        Some("result") => {
            let failed = v["is_error"].as_bool().unwrap_or(false) || v["subtype"].as_str().is_some_and(|s| s != "success");
            vec![Event::Done { text: v["result"].as_str().unwrap_or("").to_string(), failed }]
        }
        _ => Vec::new(),
    }
}

/// What the user said, as one line for Claude Code's stdin.
pub fn user_line(text: &str) -> String {
    json!({ "type": "user", "message": { "role": "user", "content": text }, "parent_tool_use_id": null }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stream_says_ready_the_tools_used_and_the_answer() {
        assert_eq!(parse(r#"{"type":"system","subtype":"init","session_id":"s","tools":["Read"],"mcp_servers":[{"name":"ciel","status":"connected"}]}"#), vec![Event::Ready]);
        assert_eq!(
            parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Let me look."},{"type":"tool_use","id":"t1","name":"mcp__ciel__calendar","input":{}}]},"session_id":"s"}"#),
            vec![Event::ToolUse("mcp__ciel__calendar".into())]
        );
        assert_eq!(
            parse(r#"{"type":"result","subtype":"success","is_error":false,"result":"Your next meeting is at 3.","total_cost_usd":0.01,"session_id":"s"}"#),
            vec![Event::Done { text: "Your next meeting is at 3.".into(), failed: false }]
        );
        assert_eq!(
            parse(r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["max turns"]}"#),
            vec![Event::Done { text: String::new(), failed: true }]
        );
        assert!(parse(r#"{"type":"stream_event","event":{}}"#).is_empty());
        assert!(parse(r#"{"type":"user","message":{"content":[{"type":"tool_result"}]}}"#).is_empty());
        assert!(parse("not json").is_empty());
    }

    #[test]
    fn the_users_turn_is_one_line() {
        let line = user_line("What's next?\nAnd after?");
        assert!(!line.contains('\n'));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v, json!({ "type": "user", "message": { "role": "user", "content": "What's next?\nAnd after?" }, "parent_tool_use_id": null }));
    }
}
