// Ciel's brain: the model that thinks for « OK Ciel », the dictation clean-up
// and the notes of a call. Claude with the user's key, or the local model the
// user picked for the chat — on Ollama, LM Studio or their own
// OpenAI-compatible server (local_chat.rs) — as Settings.ai_engine says:
// "auto" (Claude when a key is saved, else the local model), "claude" or
// "local". With neither, these features stay off, as they did without a key.
//
// A local model gets the same prompts and the same tools as Claude, less the
// web search (Anthropic runs it on its side). Its thinking is turned off where
// the server can (Ollama's own API, `think: false`) and dropped from the
// answer everywhere else, so it never reaches the user. Nothing goes anywhere
// but the address the user connected in Settings → Local models.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::i18n::{t, tf};
use crate::settings::Settings;
use crate::{local_chat, net, secrets};

pub const AUTO: &str = "auto";
pub const CLAUDE: &str = "claude";
pub const LOCAL: &str = "local";

/// The model servers, in the order Auto looks at them after the chat's own.
const SERVERS: [&str; 3] = ["ollama", "lmstudio", "custom"];
/// How long Ollama keeps the model loaded after a request: the next dictation
/// or question shouldn't wait for it to load again.
const KEEP_ALIVE: &str = "15m";
/// Ollama's context on most graphics cards. Asking for another one reloads the
/// model, so only a long call's summary does.
const DEFAULT_CONTEXT: u32 = 4096;
const MAX_CONTEXT: u32 = 20_480;

// ── Which engine ──────────────────────────────────────────────────────────────

/// A local model as the chat has it: its server and the model picked for it.
#[derive(Clone, PartialEq)]
pub struct Local {
    /// "ollama", "lmstudio" or "custom".
    pub id: &'static str,
    pub name: &'static str,
    /// The server's address, as stored.
    pub url: String,
    pub model: String,
    /// The custom server's key, bound to its address (local_chat.rs).
    key: Option<String>,
}

impl std::fmt::Debug for Local {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key.
        f.debug_struct("Local").field("id", &self.id).field("url", &self.url).field("model", &self.model).finish_non_exhaustive()
    }
}

impl Local {
    pub fn dialect(&self) -> Dialect {
        if self.id == "ollama" {
            Dialect::Ollama
        } else {
            Dialect::OpenAi
        }
    }

    /// The server runs on this computer: what it is sent stays here.
    pub fn loopback(&self) -> bool {
        net::normalise_server_url(&self.url).is_ok_and(|u| net::is_loopback_url(&u))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Engine {
    Claude,
    Local(Local),
}

/// The local server and model the chat would use: the chat's own provider
/// when it is a model server, else the first connected server with a model
/// picked. None when no server is connected.
pub fn local_target(settings: &Settings) -> Option<(&'static str, String)> {
    let chat = SERVERS.iter().find(|id| **id == settings.chat_provider);
    chat.into_iter().chain(SERVERS.iter()).find_map(|id| {
        let url = match *id {
            "ollama" => &settings.ollama_url,
            "lmstudio" => &settings.lmstudio_url,
            _ => &settings.custom_url,
        };
        let model = settings.chat_models.get(*id).map(|m| m.trim()).filter(|m| !m.is_empty())?;
        (!url.trim().is_empty()).then(|| (*id, model.to_string()))
    })
}

fn local_model(settings: &Settings) -> Option<Local> {
    let (id, model) = local_target(settings)?;
    let server = local_chat::server(settings, id)?;
    Some(Local { id: server.id, name: server.name, url: server.url, model, key: server.key })
}

/// The engine for a setting: Claude needs its key, a local model a connected
/// server; Auto takes Claude when it can, else the local model.
pub fn choose(pref: &str, has_claude_key: bool, local: impl FnOnce() -> Option<Local>) -> Option<Engine> {
    match pref {
        CLAUDE => has_claude_key.then_some(Engine::Claude),
        LOCAL => local().map(Engine::Local),
        _ if has_claude_key => Some(Engine::Claude),
        _ => local().map(Engine::Local),
    }
}

pub fn engine_for(settings: &Settings) -> Option<Engine> {
    choose(&settings.ai_engine, secrets::present(crate::claude::KEY), || local_model(settings))
}

/// What thinks now, or None: « OK Ciel », the clean-up and the summaries are off.
pub fn current(app: &AppHandle) -> Option<Engine> {
    let shared = app.try_state::<crate::Shared>()?;
    let settings = shared.settings.lock().unwrap().clone();
    engine_for(&settings)
}

/// The setting as one of its three values; anything else reads as Auto.
pub fn pref(settings: &Settings) -> &'static str {
    match settings.ai_engine.as_str() {
        CLAUDE => CLAUDE,
        LOCAL => LOCAL,
        _ => AUTO,
    }
}

/// What the Ciel window shows of it (src/core/brain.ts).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub pref: &'static str,
    /// "claude", "local", or "off".
    pub engine: &'static str,
    pub server: Option<String>,
    pub model: Option<String>,
    pub url: Option<String>,
    /// The local server is this computer.
    pub loopback: bool,
}

pub fn status_of(pref: &'static str, engine: Option<&Engine>) -> Status {
    let off = Status { pref, engine: "off", server: None, model: None, url: None, loopback: false };
    match engine {
        None => off,
        Some(Engine::Claude) => Status { engine: CLAUDE, ..off },
        Some(Engine::Local(l)) => Status {
            engine: LOCAL,
            server: Some(t(l.name)),
            model: Some(l.model.clone()),
            url: Some(l.url.clone()),
            loopback: l.loopback(),
            ..off
        },
    }
}

pub fn status(settings: &Settings) -> Status {
    status_of(pref(settings), engine_for(settings).as_ref())
}

// ── Talking to a local model ──────────────────────────────────────────────────

/// How a server is spoken to: Ollama's own API, where thinking can be turned
/// off, or the OpenAI one every other server has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialect {
    Ollama,
    OpenAi,
}

impl Dialect {
    fn path(self) -> &'static str {
        match self {
            Dialect::Ollama => "api/chat",
            Dialect::OpenAi => "v1/chat/completions",
        }
    }
}

/// Tokens a tool-choosing turn may spend thinking, on top of `max_tokens`.
pub const THINKING_BUDGET: u32 = 1024;

pub struct Request<'a> {
    pub system: &'a str,
    /// The conversation after the system prompt, in this dialect's format.
    pub messages: &'a [Value],
    /// In the function format (`function_tools`); empty for none.
    pub tools: &'a [Value],
    pub max_tokens: u32,
    /// The answer must be one JSON object (Ollama only).
    pub json: bool,
    /// A larger context than the server's usual one (Ollama only).
    pub context: Option<u32>,
}

/// The request's body. Never streamed: these answers are short, and read whole.
pub fn request_body(dialect: Dialect, model: &str, req: &Request) -> Value {
    let mut messages = vec![json!({ "role": "system", "content": req.system })];
    messages.extend(req.messages.iter().cloned());
    let mut body = match dialect {
        Dialect::Ollama => {
            // With tools to choose from, the model thinks first, at temperature 0:
            // without thinking, a small model like qwen3.5:9b tends to say it did
            // something instead of calling the tool (1 call in 12 tries, against 12
            // in 12 thinking). Its thinking needs room on top of the answer. Plain
            // answers (clean-up, summaries) stay quick, without thinking.
            let reason = !req.tools.is_empty();
            let budget = if reason { req.max_tokens.saturating_add(THINKING_BUDGET) } else { req.max_tokens };
            let mut options = json!({ "num_predict": budget });
            if reason {
                options["temperature"] = json!(0);
            }
            if let Some(n) = req.context {
                options["num_ctx"] = json!(n);
            }
            let mut body = json!({
                "model": model,
                "messages": messages,
                "stream": false,
                "think": reason,
                "keep_alive": KEEP_ALIVE,
                "options": options,
            });
            if req.json {
                body["format"] = json!("json");
            }
            body
        }
        Dialect::OpenAi => json!({ "model": model, "messages": messages, "stream": false, "max_tokens": req.max_tokens }),
    };
    if !req.tools.is_empty() {
        body["tools"] = json!(req.tools);
    }
    body
}

/// Claude's tool definitions in the OpenAI / Ollama function format. A tool
/// that runs on Anthropic's side (the web search) has no schema: left out.
pub fn function_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| {
            let name = tool.get("name")?.as_str()?;
            let schema = tool.get("input_schema")?;
            let mut function = json!({ "name": name, "parameters": schema });
            if let Some(description) = tool.get("description").and_then(Value::as_str) {
                function["description"] = json!(description);
            }
            Some(json!({ "type": "function", "function": function }))
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Always an object.
    pub arguments: Value,
}

/// One answer of the model: what it says, and the tools it calls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Turn {
    /// Without its thinking or calls written as text.
    pub text: String,
    pub calls: Vec<ToolCall>,
    /// Calls that could not be read (no name, arguments that are not an object).
    pub malformed: usize,
}

/// The answer in a response body. Small models get tool calls wrong: a call
/// that can't be read is counted, never run.
pub fn parse_reply(dialect: Dialect, body: &Value) -> Result<Turn, String> {
    let message = match dialect {
        Dialect::Ollama => body.get("message"),
        Dialect::OpenAi => body.pointer("/choices/0/message"),
    }
    .filter(|m| m.is_object())
    .ok_or_else(|| t("Unexpected API response."))?;
    let mut turn = Turn::default();
    for call in message.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
        match tool_call(call) {
            Some(c) => turn.calls.push(c),
            None => turn.malformed += 1,
        }
    }
    // Some chat templates leave the calls in the text, Qwen / Hermes style.
    let (text, embedded, bad) = embedded_calls(&content_text(message.get("content")));
    turn.calls.extend(embedded);
    turn.malformed += bad;
    turn.text = visible_text(&text);
    Ok(turn)
}

fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect(),
        _ => String::new(),
    }
}

fn new_call_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!("call_{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

fn tool_call(call: &Value) -> Option<ToolCall> {
    let function = call.get("function").unwrap_or(call);
    let name = function.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    let arguments = arguments(function.get("arguments").or_else(|| function.get("parameters")))?;
    let id = call.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(new_call_id);
    Some(ToolCall { id, name: name.to_string(), arguments })
}

/// Ollama sends the arguments as an object, the OpenAI API as the JSON text
/// of one. Anything else is a call that can't be read.
fn arguments(raw: Option<&Value>) -> Option<Value> {
    match raw {
        None | Some(Value::Null) => Some(json!({})),
        Some(Value::Object(_)) => raw.cloned(),
        Some(Value::String(s)) if s.trim().is_empty() => Some(json!({})),
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok().filter(Value::is_object),
        _ => None,
    }
}

/// `<tool_call>{"name": …, "arguments": …}</tool_call>` blocks written in the
/// text: the text without them, the calls, and how many could not be read.
fn embedded_calls(text: &str) -> (String, Vec<ToolCall>, usize) {
    const OPEN: &str = "<tool_call>";
    const CLOSE: &str = "</tool_call>";
    let (mut rest, mut calls, mut bad) = (String::new(), Vec::new(), 0);
    let mut tail = text;
    while let Some(start) = tail.find(OPEN) {
        rest.push_str(&tail[..start]);
        let after = &tail[start + OPEN.len()..];
        let (inner, next) = match after.find(CLOSE) {
            Some(end) => (&after[..end], &after[end + CLOSE.len()..]),
            None => (after, ""),
        };
        match serde_json::from_str::<Value>(inner.trim()).ok().as_ref().and_then(tool_call) {
            Some(call) => calls.push(call),
            None => bad += 1,
        }
        tail = next;
    }
    rest.push_str(tail);
    (rest, calls, bad)
}

/// What the model wrote for the user: its thinking left out — a `<think>`
/// block, the end of one the chat template opened, or one never closed.
pub fn visible_text(text: &str) -> String {
    let mut text = text;
    if let Some(end) = text.rfind("</think>") {
        if !text[..end].contains("<think>") {
            text = &text[end + "</think>".len()..];
        }
    }
    local_chat::progressive_filter(text)
}

/// The assistant's answer as the history keeps it, calls included.
pub fn assistant_message(dialect: Dialect, text: &str, calls: &[ToolCall]) -> Value {
    let mut message = json!({ "role": "assistant", "content": text });
    if !calls.is_empty() {
        let calls: Vec<Value> = calls
            .iter()
            .map(|c| match dialect {
                Dialect::Ollama => json!({ "id": c.id, "function": { "name": c.name, "arguments": c.arguments } }),
                Dialect::OpenAi => json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments.to_string() },
                }),
            })
            .collect();
        message["tool_calls"] = json!(calls);
    }
    message
}

/// What a tool answered, for the model.
pub fn tool_message(dialect: Dialect, call: &ToolCall, result: &str) -> Value {
    match dialect {
        Dialect::Ollama => json!({ "role": "tool", "tool_name": call.name, "tool_call_id": call.id, "content": result }),
        Dialect::OpenAi => json!({ "role": "tool", "tool_call_id": call.id, "content": result }),
    }
}

enum Failure {
    Status(u16, String),
    Other(String),
}

async fn post(base: &Url, key: Option<&str>, dialect: Dialect, model: &str, body: &Value, timeout: Duration) -> Result<Value, Failure> {
    let response = net::client(base, timeout)
        .map_err(Failure::Other)?
        .post(net::join(base, dialect.path()))
        .header("Authorization", local_chat::bearer(key))
        .json(body)
        .send()
        .await
        .map_err(|e| {
            Failure::Other(if e.is_timeout() {
                format!("no answer from the local model within {} s", timeout.as_secs())
            } else {
                local_chat::unreachable(base)
            })
        })?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Err(Failure::Other(tf("{model} isn't installed. Pick another model above the chat box.", &[("model", model)])));
    }
    if !status.is_success() {
        let body = net::read_capped(response, net::MAX_ERROR_BODY).await.unwrap_or_default();
        return Err(Failure::Status(status.as_u16(), net::error_detail(&body)));
    }
    let bytes = net::read_capped(response, net::MAX_BODY).await.map_err(Failure::Other)?;
    serde_json::from_slice(&bytes).map_err(|_| Failure::Other(t("Unexpected API response.")))
}

/// One request to the local model, and its answer.
pub async fn ask(local: &Local, req: &Request<'_>, timeout: Duration) -> Result<Turn, String> {
    if local.model.trim().is_empty() {
        return Err(t("Pick a model above the chat box first."));
    }
    let base = net::normalise_server_url(&local.url)?;
    let dialect = local.dialect();
    let mut body = request_body(dialect, &local.model, req);
    // What an older model refuses: not thinking, and tools. Asked again without.
    for _ in 0..3 {
        match post(&base, local.key.as_deref(), dialect, &local.model, &body, timeout).await {
            Ok(json) => return parse_reply(dialect, &json),
            Err(Failure::Status(400, detail)) if detail.contains("does not support thinking") && body.get("think").is_some() => {
                body.as_object_mut().map(|b| b.remove("think"));
            }
            Err(Failure::Status(400, detail)) if detail.contains("does not support tools") && body.get("tools").is_some() => {
                body.as_object_mut().map(|b| b.remove("tools"));
            }
            Err(Failure::Status(code, detail)) => {
                return Err(if detail.is_empty() { format!("HTTP {code}") } else { detail });
            }
            Err(Failure::Other(message)) => return Err(message),
        }
    }
    Err(t("Unexpected API response."))
}

/// How a one-question request is made.
pub struct Ask {
    pub max_tokens: u32,
    pub json: bool,
    pub context: Option<u32>,
    pub timeout: Duration,
}

/// One question, one answer, no history: the clean-up and the notes of a call.
pub async fn complete(local: &Local, system: &str, user: &str, how: Ask) -> Result<String, String> {
    let messages = [json!({ "role": "user", "content": user })];
    let req = Request { system, messages: &messages, tools: &[], max_tokens: how.max_tokens, json: how.json, context: how.context };
    let turn = ask(local, &req, how.timeout).await?;
    if turn.text.trim().is_empty() {
        return Err("empty answer".into());
    }
    Ok(turn.text)
}

/// The context a prompt of `chars` characters needs, with room for the
/// answer, when that is more than Ollama's usual one.
pub fn context_for(chars: usize, max_tokens: u32) -> Option<u32> {
    let needed = (chars / 3) as u32 + max_tokens + 256;
    (needed > DEFAULT_CONTEXT).then(|| (needed.div_ceil(4096) * 4096).min(MAX_CONTEXT))
}

/// Has Ollama load the model now (a dictation or a question is coming), so
/// the answer doesn't wait for it. At most once a minute; nothing for the
/// other servers, which have no way to ask for that alone.
pub fn warm_up(app: &AppHandle) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap();
        if last.is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(Engine::Local(local)) = current(&app) else { return };
        if local.dialect() != Dialect::Ollama {
            return;
        }
        let Ok(base) = net::normalise_server_url(&local.url) else { return };
        // No messages: Ollama only loads the model.
        let body = json!({ "model": local.model, "messages": [], "keep_alive": KEEP_ALIVE });
        let _ = tauri::async_runtime::block_on(async {
            net::client(&base, Duration::from_secs(90))?
                .post(net::join(&base, Dialect::Ollama.path()))
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())
        });
    });
}

#[cfg(test)]
pub fn test_local(id: &'static str, url: &str, model: &str) -> Local {
    Local { id, name: "Ollama", url: url.into(), model: model.into(), key: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::mpsc;

    fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    fn ollama() -> Local {
        test_local("ollama", "http://127.0.0.1:11434", "qwen3.5:9b")
    }

    // ── Which engine ──

    #[test]
    fn auto_takes_claude_with_a_key_else_the_local_model_else_nothing() {
        let some = || Some(ollama());
        assert_eq!(choose(AUTO, true, some), Some(Engine::Claude));
        assert_eq!(choose(AUTO, false, some), Some(Engine::Local(ollama())));
        assert_eq!(choose(AUTO, false, || None), None);
        // An unknown value (a newer build's) is Auto.
        assert_eq!(choose("", false, some), Some(Engine::Local(ollama())));
        assert_eq!(choose("cloud", true, some), Some(Engine::Claude));
    }

    #[test]
    fn claude_or_local_is_forced_and_off_without_what_it_needs() {
        let some = || Some(ollama());
        assert_eq!(choose(CLAUDE, true, some), Some(Engine::Claude));
        assert_eq!(choose(CLAUDE, false, some), None);
        assert_eq!(choose(LOCAL, true, some), Some(Engine::Local(ollama())));
        assert_eq!(choose(LOCAL, true, || None), None);
        assert_eq!(choose(LOCAL, false, || None), None);
        // With a key, Auto and Claude never look for a local server.
        assert_eq!(choose(AUTO, true, || panic!("looked")), Some(Engine::Claude));
    }

    fn settings(provider: &str, urls: [&str; 3], models: &[(&str, &str)]) -> Settings {
        Settings {
            chat_provider: provider.into(),
            ollama_url: urls[0].into(),
            lmstudio_url: urls[1].into(),
            custom_url: urls[2].into(),
            chat_models: models.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            ..Settings::default()
        }
    }

    #[test]
    fn the_local_model_is_the_one_the_chat_uses() {
        let both = [("ollama", "qwen3.5:9b"), ("lmstudio", "gemma-3")];
        // The chat's own server first.
        let s = settings("lmstudio", ["http://127.0.0.1:11434", "http://127.0.0.1:1234", ""], &both);
        assert_eq!(local_target(&s), Some(("lmstudio", "gemma-3".into())));
        // The chat on Claude: the first connected server with a model.
        let s = settings("anthropic", ["http://127.0.0.1:11434", "http://127.0.0.1:1234", ""], &both);
        assert_eq!(local_target(&s), Some(("ollama", "qwen3.5:9b".into())));
        // Not connected, or no model picked: skipped.
        let s = settings("ollama", ["", "http://127.0.0.1:1234", ""], &both);
        assert_eq!(local_target(&s), Some(("lmstudio", "gemma-3".into())));
        let s = settings("ollama", ["http://127.0.0.1:11434", "", ""], &[("ollama", " ")]);
        assert_eq!(local_target(&s), None);
        assert_eq!(local_target(&Settings::default()), None);
    }

    #[test]
    fn the_status_says_what_thinks_and_whether_it_stays_here() {
        let off = status_of(AUTO, None);
        assert_eq!((off.engine, off.model.as_deref()), ("off", None));
        assert_eq!(status_of(CLAUDE, Some(&Engine::Claude)).engine, "claude");
        let local = status_of(AUTO, Some(&Engine::Local(ollama())));
        assert_eq!(local.engine, "local");
        assert_eq!(local.model.as_deref(), Some("qwen3.5:9b"));
        assert_eq!(local.server.as_deref(), Some("Ollama"));
        assert!(local.loopback);
        let far = test_local("custom", "https://llm.example.com", "m");
        assert!(!status_of(LOCAL, Some(&Engine::Local(far))).loopback);
        assert_eq!(pref(&Settings::default()), AUTO);
        assert_eq!(pref(&Settings { ai_engine: "nonsense".into(), ..Settings::default() }), AUTO);
        assert_eq!(pref(&Settings { ai_engine: LOCAL.into(), ..Settings::default() }), LOCAL);
        // The key never shows in a log line.
        let keyed = Local { key: Some("sk-secret".into()), ..ollama() };
        assert!(!format!("{keyed:?}").contains("sk-secret"));
    }

    // ── Requests ──

    fn claude_tools() -> Vec<Value> {
        vec![
            json!({ "name": "pills", "description": "Add or remove pills.", "input_schema": { "type": "object", "properties": { "action": { "type": "string" } }, "required": ["action"] } }),
            json!({ "name": "service_info", "description": "Read data.", "input_schema": { "type": "object", "properties": {} } }),
            json!({ "type": "web_search_20250305", "name": "web_search", "max_uses": 3 }),
        ]
    }

    #[test]
    fn claude_tools_become_functions_and_the_web_search_stays_behind() {
        let tools = function_tools(&claude_tools());
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "pills");
        assert_eq!(tools[0]["function"]["description"], "Add or remove pills.");
        assert_eq!(tools[0]["function"]["parameters"]["required"][0], "action");
        assert_eq!(tools[1]["function"]["parameters"], json!({ "type": "object", "properties": {} }));
        assert!(tools.iter().all(|t| t["function"]["name"] != "web_search"));
    }

    #[test]
    fn ollama_thinks_only_when_it_has_tools_to_choose_from_and_never_streams() {
        let tools = function_tools(&claude_tools());
        let history = [json!({ "role": "user", "content": "add GitHub" })];
        let req = Request { system: "sys", messages: &history, tools: &tools, max_tokens: 1024, json: false, context: None };
        let body = request_body(Dialect::Ollama, "qwen3.5:9b", &req);
        assert_eq!(body["think"], true);
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "qwen3.5:9b");
        assert_eq!(body["keep_alive"], KEEP_ALIVE);
        assert_eq!(body["options"], json!({ "num_predict": 1024 + THINKING_BUDGET, "temperature": 0 }));
        assert_eq!(body["messages"][0], json!({ "role": "system", "content": "sys" }));
        assert_eq!(body["messages"][1]["content"], "add GitHub");
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
        assert!(body.get("format").is_none());

        // A summary: JSON, and a larger context when the call was long.
        let req = Request { system: "sys", messages: &history, tools: &[], max_tokens: 2048, json: true, context: Some(8192) };
        let body = request_body(Dialect::Ollama, "m", &req);
        assert_eq!(body["format"], "json");
        assert_eq!(body["think"], false);
        assert_eq!(body["options"], json!({ "num_predict": 2048, "num_ctx": 8192 }));
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn other_servers_get_the_plain_openai_request() {
        let tools = function_tools(&claude_tools());
        let req = Request { system: "sys", messages: &[], tools: &tools, max_tokens: 512, json: true, context: Some(8192) };
        let body = request_body(Dialect::OpenAi, "gemma-3", &req);
        assert_eq!(body["stream"], false);
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["tools"][0]["function"]["name"], "pills");
        for ollama_only in ["think", "keep_alive", "options", "format"] {
            assert!(body.get(ollama_only).is_none(), "{ollama_only}");
        }
    }

    #[test]
    fn a_long_call_asks_for_a_larger_context_up_to_a_ceiling() {
        assert_eq!(context_for(2_000, 2048), None);
        assert_eq!(context_for(12_000, 2048), Some(8192));
        assert_eq!(context_for(48_000, 2048), Some(20_480));
        assert_eq!(context_for(400_000, 2048), Some(MAX_CONTEXT));
    }

    // ── Answers ──

    #[test]
    fn a_tool_call_is_read_in_both_dialects() {
        let ollama = json!({ "message": { "role": "assistant", "content": "", "tool_calls": [
            { "function": { "name": "pills", "arguments": { "action": "add", "names": ["GitHub"] } } }
        ] }, "done": true });
        let turn = parse_reply(Dialect::Ollama, &ollama).unwrap();
        assert_eq!(turn.text, "");
        assert_eq!(turn.malformed, 0);
        assert_eq!(turn.calls.len(), 1);
        assert_eq!(turn.calls[0].name, "pills");
        assert_eq!(turn.calls[0].arguments, json!({ "action": "add", "names": ["GitHub"] }));
        assert!(turn.calls[0].id.starts_with("call_"));

        let openai = json!({ "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
            { "id": "abc", "type": "function", "function": { "name": "music", "arguments": "{\"action\":\"next\"}" } },
            { "id": "def", "type": "function", "function": { "name": "service_info", "arguments": "" } }
        ] }, "finish_reason": "tool_calls" }] });
        let turn = parse_reply(Dialect::OpenAi, &openai).unwrap();
        assert_eq!(turn.calls.len(), 2);
        assert_eq!(turn.calls[0], ToolCall { id: "abc".into(), name: "music".into(), arguments: json!({ "action": "next" }) });
        assert_eq!(turn.calls[1].arguments, json!({}));
    }

    #[test]
    fn a_malformed_tool_call_is_counted_and_never_run() {
        let body = json!({ "choices": [{ "message": { "content": "", "tool_calls": [
            { "id": "1", "function": { "name": "pills", "arguments": "{\"action\": \"add\", " } },
            { "id": "2", "function": { "name": "", "arguments": "{}" } },
            { "id": "3", "function": { "arguments": "{}" } },
            { "id": "4", "function": { "name": "pills", "arguments": "[\"add\"]" } },
            { "id": "5", "function": { "name": "pills", "arguments": 7 } },
            "nonsense"
        ] } }] });
        let turn = parse_reply(Dialect::OpenAi, &body).unwrap();
        assert!(turn.calls.is_empty());
        assert_eq!(turn.malformed, 6);
    }

    #[test]
    fn a_plain_answer_has_no_calls_and_no_thinking() {
        let body = json!({ "message": { "content": "<think>\nThe user greets me.\n</think>\n\nHello! Ready when you are." } });
        let turn = parse_reply(Dialect::Ollama, &body).unwrap();
        assert_eq!(turn, Turn { text: "Hello! Ready when you are.".into(), calls: vec![], malformed: 0 });
        // The template opened the block: only its end is in the answer.
        assert_eq!(visible_text("reasoning here</think>\n\nIt is 20 °C."), "It is 20 °C.");
        // Cut off while thinking: nothing of it is said.
        assert_eq!(visible_text("Sure. <think>let me see"), "Sure.");
        assert_eq!(visible_text("<think>only thinking"), "");
        assert_eq!(visible_text("  Plain.  "), "Plain.");
        // Content as parts, as some servers send it.
        let parts = json!({ "choices": [{ "message": { "content": [{ "type": "text", "text": "Hi " }, { "type": "text", "text": "there" }] } }] });
        assert_eq!(parse_reply(Dialect::OpenAi, &parts).unwrap().text, "Hi there");
    }

    #[test]
    fn calls_written_in_the_text_are_read_and_never_said() {
        let body = json!({ "message": { "content":
            "Adding it.\n<tool_call>\n{\"name\": \"pills\", \"arguments\": {\"action\": \"add\", \"names\": [\"GitHub\"]}}\n</tool_call>\n<tool_call>{not json}</tool_call>" } });
        let turn = parse_reply(Dialect::Ollama, &body).unwrap();
        assert_eq!(turn.text, "Adding it.");
        assert_eq!(turn.calls.len(), 1);
        assert_eq!(turn.calls[0].arguments["names"][0], "GitHub");
        assert_eq!(turn.malformed, 1);
        // One never closed is not read aloud either.
        let body = json!({ "message": { "content": "<tool_call>{\"name\": \"pills\"" } });
        let turn = parse_reply(Dialect::Ollama, &body).unwrap();
        assert_eq!((turn.text.as_str(), turn.calls.len(), turn.malformed), ("", 0, 1));
    }

    #[test]
    fn an_answer_without_a_message_is_an_error() {
        assert!(parse_reply(Dialect::Ollama, &json!({ "error": "x" })).is_err());
        assert!(parse_reply(Dialect::OpenAi, &json!({ "choices": [] })).is_err());
        assert!(parse_reply(Dialect::OpenAi, &json!({ "choices": [{ "message": "hi" }] })).is_err());
    }

    #[test]
    fn the_history_keeps_calls_and_results_in_each_dialect() {
        let call = ToolCall { id: "c1".into(), name: "pills".into(), arguments: json!({ "action": "list" }) };
        let m = assistant_message(Dialect::Ollama, "", std::slice::from_ref(&call));
        assert_eq!(m["tool_calls"][0]["function"]["arguments"], json!({ "action": "list" }));
        let m = assistant_message(Dialect::OpenAi, "", std::slice::from_ref(&call));
        assert_eq!(m["tool_calls"][0]["function"]["arguments"], "{\"action\":\"list\"}");
        assert_eq!(m["tool_calls"][0]["type"], "function");
        assert!(assistant_message(Dialect::OpenAi, "Hi", &[]).get("tool_calls").is_none());
        assert_eq!(tool_message(Dialect::Ollama, &call, "ok"), json!({ "role": "tool", "tool_name": "pills", "tool_call_id": "c1", "content": "ok" }));
        assert_eq!(tool_message(Dialect::OpenAi, &call, "ok"), json!({ "role": "tool", "tool_call_id": "c1", "content": "ok" }));
    }

    // ── Over HTTP ──

    /// A server on a free port answering each request with the next reply,
    /// and handing back what each request asked: (path, body).
    fn serve(replies: Vec<(&'static str, String)>) -> (String, mpsc::Receiver<(String, Value)>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (status, reply) in replies {
                let Ok((mut conn, _)) = listener.accept() else { return };
                let mut data = Vec::new();
                let mut buf = [0u8; 8192];
                let head_end = loop {
                    let n = conn.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&buf[..n]);
                    if let Some(at) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                };
                let head = String::from_utf8_lossy(&data[..head_end]).to_lowercase();
                let length = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                    .unwrap_or(0);
                while data.len() < head_end + length {
                    let n = conn.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..n]);
                }
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let _ = tx.send((path, serde_json::from_slice(&data[head_end..]).unwrap_or(Value::Null)));
                let answer = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = conn.write_all(answer.as_bytes());
            }
        });
        (url, rx)
    }

    fn quick() -> Ask {
        Ask { max_tokens: 64, json: false, context: None, timeout: Duration::from_secs(10) }
    }

    #[test]
    fn ollama_gets_its_own_api_without_thinking() {
        let (url, sent) = serve(vec![("200 OK", json!({ "message": { "content": "<think></think>Clean text." } }).to_string())]);
        let local = test_local("ollama", &url, "qwen3.5:9b");
        assert_eq!(block_on(complete(&local, "sys", "raw", quick())).unwrap(), "Clean text.");
        let (path, body) = sent.recv().unwrap();
        assert_eq!(path, "/api/chat");
        assert_eq!(body["think"], false);
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][1], json!({ "role": "user", "content": "raw" }));
    }

    #[test]
    fn other_servers_get_the_openai_api() {
        let (url, sent) = serve(vec![("200 OK", json!({ "choices": [{ "message": { "content": "Done." } }] }).to_string())]);
        let local = test_local("lmstudio", &url, "gemma-3");
        assert_eq!(block_on(complete(&local, "sys", "raw", quick())).unwrap(), "Done.");
        let (path, body) = sent.recv().unwrap();
        assert_eq!(path, "/v1/chat/completions");
        assert!(body.get("think").is_none());
    }

    #[test]
    fn a_model_that_cannot_turn_thinking_off_or_use_tools_is_asked_again_without() {
        let (url, sent) = serve(vec![
            ("400 Bad Request", json!({ "error": "\"llama3.2\" does not support thinking" }).to_string()),
            ("400 Bad Request", json!({ "error": "registry.ollama.ai/library/llama3.2 does not support tools" }).to_string()),
            ("200 OK", json!({ "message": { "content": "Hello." } }).to_string()),
        ]);
        let local = test_local("ollama", &url, "llama3.2");
        let tools = function_tools(&claude_tools());
        let messages = [json!({ "role": "user", "content": "hi" })];
        let req = Request { system: "s", messages: &messages, tools: &tools, max_tokens: 64, json: false, context: None };
        let turn = block_on(ask(&local, &req, Duration::from_secs(10))).unwrap();
        assert_eq!(turn.text, "Hello.");
        let bodies: Vec<Value> = sent.try_iter().map(|(_, b)| b).collect();
        assert_eq!(bodies.len(), 3);
        assert!(bodies[0].get("think").is_some() && bodies[0].get("tools").is_some());
        assert!(bodies[1].get("think").is_none() && bodies[1].get("tools").is_some());
        assert!(bodies[2].get("think").is_none() && bodies[2].get("tools").is_none());
    }

    #[test]
    fn errors_say_what_happened() {
        let (url, _sent) = serve(vec![("404 Not Found", json!({ "error": "model 'nope' not found" }).to_string())]);
        let err = block_on(complete(&test_local("ollama", &url, "nope"), "s", "u", quick())).unwrap_err();
        assert_eq!(err, "nope isn't installed. Pick another model above the chat box.");
        let (url, _sent) = serve(vec![("500 Internal Server Error", json!({ "error": "out of memory" }).to_string())]);
        assert_eq!(block_on(complete(&test_local("ollama", &url, "m"), "s", "u", quick())).unwrap_err(), "out of memory");
        let (url, _sent) = serve(vec![("200 OK", json!({ "message": { "content": "<think>only</think>" } }).to_string())]);
        assert_eq!(block_on(complete(&test_local("ollama", &url, "m"), "s", "u", quick())).unwrap_err(), "empty answer");
        let err = block_on(complete(&test_local("ollama", "http://127.0.0.1:1", "m"), "s", "u", quick())).unwrap_err();
        assert!(err.starts_with("Cannot reach"), "{err}");
        assert_eq!(block_on(complete(&test_local("ollama", "http://127.0.0.1:1", ""), "s", "u", quick())).unwrap_err(), "Pick a model above the chat box first.");
    }
}

/// With a real model server: `CIEL_TEST_OLLAMA=http://127.0.0.1:11434
/// CIEL_TEST_MODEL=qwen3.5:9b cargo test -- --ignored with_a_local_model`.
#[cfg(test)]
pub mod with_a_local_model {
    use super::*;

    pub fn local() -> Local {
        let url = std::env::var("CIEL_TEST_OLLAMA").unwrap_or_else(|_| "http://127.0.0.1:11434".into());
        let model = std::env::var("CIEL_TEST_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into());
        test_local("ollama", &url, &model)
    }

    pub fn block_on<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    #[test]
    #[ignore]
    fn writes_the_notes_of_a_call_with_a_local_model() {
        let transcript = "Me: So we ship the new onboarding on Friday?\nPerson 1: Yes, if Marie finishes the copy by Thursday.\n\
Me: OK. I'll check the analytics events tomorrow.\nPerson 1: And we drop the old welcome email, nobody opens it.\nMe: Agreed, let's remove it next week.";
        let user = format!("<transcript>\n{transcript}\n</transcript>");
        let started = Instant::now();
        let how = Ask { max_tokens: 2048, json: true, context: None, timeout: Duration::from_secs(300) };
        let reply = block_on(complete(&local(), crate::notes::builder::SUMMARY_INSTRUCTIONS, &user, how)).expect("an answer");
        println!("summary in {:?}:\n{reply}", started.elapsed());
        let parsed = crate::notes::builder::parse_summary(&reply).expect("JSON notes");
        assert!(!parsed.summary.is_empty());
        assert!(!parsed.todos.is_empty());
        assert!(!reply.contains("<think>"));
    }
}
