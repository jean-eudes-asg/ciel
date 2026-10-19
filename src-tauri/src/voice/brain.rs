// « OK Ciel »'s brain: Claude with the user's key, or the chat's local model
// (ai.rs), with Ciel's tools — pills, music, the user's own data, opening an
// app, preparing an email (the mail app opens with it filled in; the user
// sends it) and, with Claude only, Anthropic's web search. The conversation
// lives in memory only and is forgotten 90 s after the last turn. Port of
// ClaudeVoiceBrain.swift.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::ai::{self, Dialect, Engine, Turn};

const FAST_MODEL: &str = "claude-haiku-4-5";
const FORGET_AFTER: Duration = Duration::from_secs(90);
/// Rounds of tool calls in one answer, with either engine.
const MAX_ROUNDS: usize = 5;
/// The messages Claude is given of the conversation.
const KEEP: usize = 24;
/// A local model's context is small: it gets fewer of them, and shorter
/// tool results.
const LOCAL_KEEP: usize = 12;
const LOCAL_RESULT_CHARS: usize = 4000;
/// A local model's turn may first wait for the model to load.
const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Brain {
    messages: Vec<Value>,
    last: Instant,
    /// Who the conversation is held with: its history is in that one's format.
    with: String,
}

impl Default for Brain {
    fn default() -> Self {
        Self { messages: Vec::new(), last: Instant::now(), with: String::new() }
    }
}

pub struct Reply {
    pub text: String,
    pub acted: bool,
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "pills",
            "description": "Add, remove, set as main or list the pills shown in the island at the top of the screen. At most 4 pills besides the main one: remove one first if needed.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["add", "remove", "set_main", "list"] },
                    "names": { "type": "array", "items": { "type": "string" }, "description": "Pill names as listed (e.g. Stripe, GitHub, Cursor). Empty for list." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "music",
            "description": "Control Spotify: play or pause, next, previous, set the volume, or search a song, an artist or a playlist in Spotify.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["play_pause", "next", "previous", "set_volume", "search"] },
                    "query": { "type": "string", "description": "What to search, for search." },
                    "volume": { "type": "integer", "description": "0-100 for set_volume." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "service_info",
            "description": "Read the user's real data that Ciel already has, as JSON: the active pills, the agent sessions (Claude Code, Codex…) and what they are doing, Claude or Codex plan usage, Spotify now playing, and the services' latest data (Stripe, GitHub, Vercel, Resend, n8n, Notion, Cal.com).",
            "input_schema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "open_app",
            "description": "Open an app on this computer by name (Firefox, VS Code, Notion, Spotify…).",
            "input_schema": { "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }
        }),
        json!({
            "name": "prepare_email",
            "description": "Open the user's mail app with an email filled in, for them to check and send (it is never sent automatically). The recipient must be an email address.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "recipient": { "type": "string", "description": "An email address." },
                    "subject": { "type": "string" },
                    "body": { "type": "string", "description": "The full email text, written for the user." }
                },
                "required": ["recipient", "subject", "body"]
            }
        }),
        json!({ "type": "web_search_20250305", "name": "web_search", "max_uses": 3 }),
    ]
}

/// The instructions, the same for both engines but for the web search, which
/// only Claude has.
fn system_prompt(pills: &str, web_search: bool) -> String {
    let t = crate::platform::local_time();
    let os = if cfg!(windows) { "Windows" } else { "Linux" };
    let act = if web_search {
        "Act with the tools instead of describing: pills, music, service_info for the user's own data, open_app, \
prepare_email, and web_search for anything current (weather, news, scores, prices, facts you are not sure of). \
Never invent the user's data."
    } else {
        "Act with the tools instead of describing: pills, music, service_info for the user's own data, open_app, \
prepare_email. Never say something is done unless a tool said so. Web search isn't available: for anything current \
(weather, news, scores, prices, facts you are not sure of), say you can't look it up from here. \
Never invent the user's data."
    };
    format!(
        "You are Ciel, a voice assistant living at the top of the screen of the user's {os} computer, a bit like Jarvis: \
quick, warm, a little playful. It is {:04}-{:02}-{:02} {:02}:{:02}.\n\
Everything you write is read aloud: answer in the language the user spoke, in one or two short sentences, \
no markdown, no lists, no emojis, no links.\n\
{act}\n\
Email: gather the recipient's address, the subject and what to say (write the text yourself from what the user \
wants to say, in the language they used), asking for what is missing one short question at a time, then call \
prepare_email. The email is never sent by you: the user checks it in their mail app and sends it.\n\
When you need an answer from the user, end with one short question. Otherwise never end with a question.\n\
Pills: {pills}",
        t.year, t.month, t.day, t.hour, t.minute
    )
}

impl Brain {
    pub fn respond(&mut self, app: &AppHandle, transcript: &str) -> Option<Reply> {
        let engine = ai::current(app)?;
        let with = match &engine {
            Engine::Claude => "claude".to_string(),
            Engine::Local(l) => format!("{}|{}|{}", l.id, l.url, l.model),
        };
        if self.last.elapsed() > FORGET_AFTER || self.with != with {
            self.messages.clear();
        }
        self.with = with;
        self.last = Instant::now();
        self.messages.push(json!({ "role": "user", "content": transcript }));
        self.trim(if matches!(engine, Engine::Claude) { KEEP } else { LOCAL_KEEP });
        let pills = super::ask_island(app, "pills", json!({ "action": "list" })).unwrap_or_default();
        match engine {
            Engine::Claude => self.respond_claude(app, &pills),
            Engine::Local(local) => self.respond_local(app, &local, &pills),
        }
    }

    fn respond_claude(&mut self, app: &AppHandle, pills: &str) -> Option<Reply> {
        let system = system_prompt(pills, true);
        let tools = tools();
        let chat_model = app
            .try_state::<crate::Shared>()
            .map(|s| s.settings.lock().unwrap().model.clone())
            .unwrap_or_default();
        let mut acted = false;
        let mut spoken = String::new();
        for _ in 0..MAX_ROUNDS {
            let turn = tauri::async_runtime::block_on(async {
                match crate::claude::voice_turn(FAST_MODEL, &system, &self.messages, &tools).await {
                    Ok(v) => Ok(v),
                    Err(err) if !chat_model.is_empty() => {
                        crate::log::line(format!("voice: {err}; the chat's model instead"));
                        crate::claude::voice_turn(&chat_model, &system, &self.messages, &tools).await
                    }
                    Err(err) => Err(err),
                }
            });
            let turn = match turn {
                Ok(t) => t,
                Err(err) => {
                    crate::log::line(format!("voice: Claude turn failed: {err}"));
                    if self.messages.last().is_some_and(|m| m["content"].is_string()) {
                        self.messages.pop();
                    }
                    return acted.then(|| Reply { text: String::new(), acted });
                }
            };
            let content = turn["content"].as_array().cloned().unwrap_or_default();
            self.messages.push(json!({ "role": "assistant", "content": content.clone() }));
            let text: String = content.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" ");
            if !text.trim().is_empty() {
                spoken = text;
            }
            let stop = turn["stop_reason"].as_str().unwrap_or("end_turn");
            if stop == "pause_turn" {
                continue;
            }
            let uses: Vec<&Value> = content.iter().filter(|b| b["type"] == "tool_use").collect();
            if stop != "tool_use" || uses.is_empty() {
                break;
            }
            let mut results = Vec::new();
            for u in uses {
                let (result, did) = super::run_tool(app, u["name"].as_str().unwrap_or(""), &u["input"]);
                acted |= did;
                results.push(json!({ "type": "tool_result", "tool_use_id": u["id"], "content": result }));
            }
            self.messages.push(json!({ "role": "user", "content": results }));
            spoken.clear();
        }
        self.repair();
        Some(Reply { text: spoken_text(&spoken), acted })
    }

    fn respond_local(&mut self, app: &AppHandle, local: &ai::Local, pills: &str) -> Option<Reply> {
        let system = system_prompt(pills, false);
        let tools = ai::function_tools(&tools());
        let names: Vec<String> = tools.iter().filter_map(|t| t["function"]["name"].as_str().map(String::from)).collect();
        let ask = |messages: &[Value], with_tools: bool| {
            let req = ai::Request {
                system: &system,
                messages,
                tools: if with_tools { tools.as_slice() } else { &[] },
                max_tokens: 1024,
                json: false,
                context: None,
            };
            tauri::async_runtime::block_on(ai::ask(local, &req, LOCAL_TIMEOUT))
        };
        local_loop(&mut self.messages, local.dialect(), &names, ask, |name, input| super::run_tool(app, name, input))
    }

    /// Keeps the last `keep` messages, never cutting between a tool call and its result.
    fn trim(&mut self, keep: usize) {
        if self.messages.len() <= keep {
            return;
        }
        let mut cut = self.messages.len() - keep;
        while cut < self.messages.len() && !(self.messages[cut]["role"] == "user" && self.messages[cut]["content"].is_string()) {
            cut += 1;
        }
        self.messages.drain(..cut.min(self.messages.len() - 1));
    }

    /// The API refuses a history ending on a tool call with no result after it.
    fn repair(&mut self) {
        let Some(last) = self.messages.last() else { return };
        if last["role"] != "assistant" {
            return;
        }
        let calls = last["content"].as_array().is_some_and(|c| c.iter().any(|b| b["type"] == "tool_use"));
        if calls {
            self.messages.pop();
        }
    }
}

/// « OK Ciel » with a local model: ask, run the tools it calls, give it their
/// results, and ask again — at most MAX_ROUNDS times. A small model may write
/// calls that can't be read, call a tool that isn't there, or call the same
/// thing again and again: it then gets one chance to answer in words, an
/// error it can correct, or the loop stops. Only the tools in `names` run,
/// through `run`, which is the same as with Claude.
fn local_loop(
    messages: &mut Vec<Value>,
    dialect: Dialect,
    names: &[String],
    mut ask: impl FnMut(&[Value], bool) -> Result<Turn, String>,
    mut run: impl FnMut(&str, &Value) -> (String, bool),
) -> Option<Reply> {
    let mut acted = false;
    let mut spoken = String::new();
    let mut with_tools = true;
    let mut previous: Option<Vec<(String, Value)>> = None;
    for round in 0..MAX_ROUNDS {
        let mut turn = match ask(messages, with_tools) {
            Ok(turn) => turn,
            Err(err) => {
                crate::log::line(format!("voice: local model turn failed: {err}"));
                if round == 0 && messages.last().is_some_and(|m| m["role"] == "user" && m["content"].is_string()) {
                    messages.pop();
                }
                return acted.then(|| Reply { text: String::new(), acted });
            }
        };
        if !with_tools {
            turn.calls.clear();
        }
        if !turn.text.is_empty() {
            spoken = turn.text.clone();
        }
        if turn.calls.is_empty() {
            if turn.text.is_empty() && turn.malformed > 0 && with_tools {
                // Only calls that can't be read: once more, for words this time.
                with_tools = false;
                continue;
            }
            if !turn.text.is_empty() {
                messages.push(ai::assistant_message(dialect, &turn.text, &[]));
            }
            break;
        }
        let asked: Vec<(String, Value)> = turn.calls.iter().map(|c| (c.name.clone(), c.arguments.clone())).collect();
        if previous.as_ref() == Some(&asked) {
            // The same calls as the round before: going round in circles.
            break;
        }
        previous = Some(asked);
        messages.push(ai::assistant_message(dialect, &turn.text, &turn.calls));
        for call in &turn.calls {
            let result = if names.iter().any(|n| *n == call.name) {
                let (result, did) = run(&call.name, &call.arguments);
                acted |= did;
                result
            } else {
                format!("There is no tool named {}. The tools are: {}.", call.name, names.join(", "))
            };
            let result: String = if result.chars().count() > LOCAL_RESULT_CHARS {
                result.chars().take(LOCAL_RESULT_CHARS).chain("…".chars()).collect()
            } else {
                result
            };
            messages.push(ai::tool_message(dialect, call, &result));
        }
        spoken.clear();
    }
    Some(Reply { text: spoken_text(&spoken), acted })
}

/// What the model wrote, ready to be said: citations and markdown left out.
pub fn spoken_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut skip = 0;
    for c in text.chars() {
        match c {
            '[' => skip += 1,
            ']' if skip > 0 => skip -= 1,
            '*' | '#' | '`' | '_' if skip == 0 => {}
            _ if skip == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::ToolCall;
    use std::collections::VecDeque;

    #[test]
    fn markdown_and_citations_are_not_read_aloud() {
        assert_eq!(spoken_text("**Done** — Stripe [1] is in the island."), "Done — Stripe is in the island.");
        assert_eq!(spoken_text("  Il fait  20 °C à Bordeaux. "), "Il fait 20 °C à Bordeaux.");
    }

    #[test]
    fn every_tool_has_a_schema() {
        for tool in tools() {
            assert!(tool["name"].is_string());
            assert!(tool.get("input_schema").is_some() || tool["type"] == "web_search_20250305");
        }
    }

    #[test]
    fn a_local_model_gets_every_tool_but_the_web_search_and_is_told_so() {
        let functions = ai::function_tools(&tools());
        let names: Vec<&str> = functions.iter().filter_map(|t| t["function"]["name"].as_str()).collect();
        assert_eq!(names, ["pills", "music", "service_info", "open_app", "prepare_email"]);
        let local = system_prompt("Stripe, GitHub", false);
        assert!(!local.contains("web_search"));
        assert!(local.contains("Web search isn't available"));
        assert!(local.contains("The email is never sent by you"));
        let claude = system_prompt("Stripe, GitHub", true);
        assert!(claude.contains("web_search for anything current"));
        assert!(!claude.contains("isn't available"));
        assert!(claude.contains("The email is never sent by you") && claude.ends_with("Pills: Stripe, GitHub"));
    }

    fn names() -> Vec<String> {
        ["pills", "music", "service_info", "open_app", "prepare_email"].map(String::from).to_vec()
    }

    fn call(name: &str, arguments: Value) -> ToolCall {
        ToolCall { id: format!("id-{name}"), name: name.into(), arguments }
    }

    fn says(text: &str) -> Turn {
        Turn { text: text.into(), ..Turn::default() }
    }

    fn calls(list: Vec<ToolCall>) -> Turn {
        Turn { calls: list, ..Turn::default() }
    }

    /// Runs the loop against a scripted model; returns the reply, how many
    /// times the model was asked (and whether with tools), and the tools run.
    fn script(turns: Vec<Result<Turn, String>>) -> (Option<Reply>, Vec<bool>, Vec<(String, Value)>, Vec<Value>) {
        let mut turns: VecDeque<_> = turns.into();
        let mut messages = vec![json!({ "role": "user", "content": "add GitHub to my pills" })];
        let mut asked = Vec::new();
        let mut ran = Vec::new();
        let reply = local_loop(
            &mut messages,
            Dialect::Ollama,
            &names(),
            |_, with_tools| {
                asked.push(with_tools);
                turns.pop_front().unwrap_or_else(|| Ok(says("(no more turns)")))
            },
            |name, input| {
                ran.push((name.to_string(), input.clone()));
                ("GitHub added".into(), name == "pills")
            },
        );
        (reply, asked, ran, messages)
    }

    #[test]
    fn a_tool_call_runs_and_the_model_then_says_it_is_done() {
        let (reply, asked, ran, messages) = script(vec![
            Ok(calls(vec![call("pills", json!({ "action": "add", "names": ["GitHub"] }))])),
            Ok(says("GitHub is in the island.")),
        ]);
        let reply = reply.unwrap();
        assert_eq!(reply.text, "GitHub is in the island.");
        assert!(reply.acted);
        assert_eq!(asked, [true, true]);
        assert_eq!(ran, [("pills".to_string(), json!({ "action": "add", "names": ["GitHub"] }))]);
        // user, assistant (call), tool (result), assistant (answer).
        let roles: Vec<&str> = messages.iter().filter_map(|m| m["role"].as_str()).collect();
        assert_eq!(roles, ["user", "assistant", "tool", "assistant"]);
        assert_eq!(messages[2]["content"], "GitHub added");
        assert_eq!(messages[2]["tool_name"], "pills");
    }

    #[test]
    fn a_plain_answer_needs_no_tool() {
        let (reply, asked, ran, _) = script(vec![Ok(says("Bonjour ! Je t'écoute."))]);
        assert_eq!(reply.unwrap().text, "Bonjour ! Je t'écoute.");
        assert_eq!(asked, [true]);
        assert!(ran.is_empty());
    }

    #[test]
    fn calls_that_cannot_be_read_get_one_more_try_in_words() {
        let garbled = Turn { malformed: 2, ..Turn::default() };
        let (reply, asked, ran, _) = script(vec![Ok(garbled), Ok(says("Sorry, say that again?"))]);
        assert_eq!(reply.unwrap().text, "Sorry, say that again?");
        // The second time without tools: only words can come back.
        assert_eq!(asked, [true, false]);
        assert!(ran.is_empty());
        // Even if the model writes a call then, nothing runs.
        let garbled = Turn { malformed: 1, ..Turn::default() };
        let sneaky = Turn { text: "Done.".into(), calls: vec![call("pills", json!({ "action": "add" }))], malformed: 0 };
        let (reply, asked, ran, _) = script(vec![Ok(garbled.clone()), Ok(sneaky)]);
        assert_eq!(reply.unwrap().text, "Done.");
        assert_eq!(asked, [true, false]);
        assert!(ran.is_empty());
        // And still nothing: no answer, no crash, no loop.
        let (reply, asked, _, _) = script(vec![Ok(garbled.clone()), Ok(garbled)]);
        let reply = reply.unwrap();
        assert_eq!((reply.text.as_str(), reply.acted), ("", false));
        assert_eq!(asked.len(), 2);
    }

    #[test]
    fn a_tool_that_does_not_exist_is_answered_not_run() {
        let (reply, _, ran, messages) = script(vec![
            Ok(calls(vec![call("web_search", json!({ "query": "weather" }))])),
            Ok(says("I can't look that up from here.")),
        ]);
        assert_eq!(reply.unwrap().text, "I can't look that up from here.");
        assert!(ran.is_empty());
        let told = messages[2]["content"].as_str().unwrap();
        assert!(told.starts_with("There is no tool named web_search."), "{told}");
        assert!(told.contains("pills, music"));
    }

    #[test]
    fn the_loop_stops_at_the_cap() {
        // A model that never stops calling tools, a different call each time.
        let endless: Vec<Result<Turn, String>> =
            (0..20).map(|i| Ok(calls(vec![call("music", json!({ "action": "set_volume", "volume": i }))]))).collect();
        let (reply, asked, ran, _) = script(endless);
        assert_eq!(asked.len(), MAX_ROUNDS);
        assert_eq!(ran.len(), MAX_ROUNDS);
        let reply = reply.unwrap();
        assert_eq!(reply.text, "");
        assert!(!reply.acted);
    }

    #[test]
    fn the_same_calls_again_stop_the_loop_at_once() {
        let again = || Ok(calls(vec![call("pills", json!({ "action": "add", "names": ["GitHub"] }))]));
        let (reply, asked, ran, _) = script(vec![again(), again(), again()]);
        assert_eq!(asked.len(), 2);
        assert_eq!(ran.len(), 1);
        // It was done once: the pill shows ✓.
        assert!(reply.unwrap().acted);
    }

    #[test]
    fn a_failed_turn_leaves_no_question_behind() {
        let (reply, _, _, messages) = script(vec![Err("Cannot reach http://127.0.0.1:11434. Is the server running?".into())]);
        assert!(reply.is_none());
        assert!(messages.is_empty());
        // After something was done, the pill still says it was.
        let (reply, _, _, _) = script(vec![Ok(calls(vec![call("pills", json!({ "action": "add" }))])), Err("timeout".into())]);
        assert!(reply.unwrap().acted);
    }

    #[test]
    fn a_long_tool_result_is_cut_for_a_small_context() {
        let mut messages = vec![json!({ "role": "user", "content": "what's up?" })];
        let mut turns: VecDeque<Turn> = vec![calls(vec![call("service_info", json!({}))]), says("All quiet.")].into();
        local_loop(&mut messages, Dialect::OpenAi, &names(), |_, _| Ok(turns.pop_front().unwrap()), |_, _| ("x".repeat(10_000), false));
        let result = messages[2]["content"].as_str().unwrap();
        assert_eq!(result.chars().count(), LOCAL_RESULT_CHARS + 1);
        assert_eq!(messages[2]["tool_call_id"], "id-service_info");
    }

    #[test]
    fn the_conversation_is_cut_at_a_question_never_between_a_call_and_its_result() {
        let mut brain = Brain::default();
        for i in 0..10 {
            brain.messages.push(json!({ "role": "user", "content": format!("q{i}") }));
            brain.messages.push(json!({ "role": "assistant", "content": "", "tool_calls": [] }));
            brain.messages.push(json!({ "role": "tool", "content": "r" }));
        }
        brain.trim(LOCAL_KEEP);
        assert!(brain.messages.len() <= LOCAL_KEEP);
        assert_eq!(brain.messages[0]["role"], "user");
        assert!(brain.messages[0]["content"].is_string());
    }
}

/// With a real model server: see crate::ai::with_a_local_model.
#[cfg(test)]
mod with_a_local_model {
    use super::*;

    #[test]
    #[ignore]
    fn adds_github_to_the_pills_with_a_local_model() {
        let local = crate::ai::with_a_local_model::local();
        let system = system_prompt("Main: VS Code. Active: Stripe, Vercel. Available: GitHub, Notion, Resend, n8n, Cal.com, Spotify.", false);
        let tools = ai::function_tools(&tools());
        let names: Vec<String> = tools.iter().filter_map(|t| t["function"]["name"].as_str().map(String::from)).collect();
        let mut messages = vec![json!({ "role": "user", "content": "add GitHub to my pills" })];
        let mut ran = Vec::new();
        let started = Instant::now();
        let reply = local_loop(
            &mut messages,
            local.dialect(),
            &names,
            |messages, with_tools| {
                let req = ai::Request { system: &system, messages, tools: if with_tools { tools.as_slice() } else { &[] }, max_tokens: 1024, json: false, context: None };
                let turn = crate::ai::with_a_local_model::block_on(ai::ask(&local, &req, LOCAL_TIMEOUT));
                println!("turn after {:?}: {turn:?}", started.elapsed());
                turn
            },
            |name, input| {
                ran.push((name.to_string(), input.clone()));
                ("GitHub added to the island.".into(), true)
            },
        )
        .expect("a reply");
        println!("said: {:?}, acted: {}, tools: {ran:?}", reply.text, reply.acted);
        assert!(ran.iter().any(|(name, input)| name == "pills"
            && input["action"] == "add"
            && input["names"].as_array().is_some_and(|n| n.iter().any(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("github"))))));
        assert!(!reply.text.contains("<think>"));
    }
}
