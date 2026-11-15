// Ciel writes to the user on Discord while they are away from their desk
// (Ciel window → Chat → Speak up → On Discord when you are away), and reads
// what they answer.
//
// The bot is the user's own: they make it in Discord's developer portal, add
// it to a server they are in, and give Ciel its token (kept in the keychain)
// and their own user ID. Ciel then writes to them, and to no one else, in the
// bot's direct messages: what it would have said aloud at their desk
// (speakup.rs). What they write back there goes to Ciel's brain like words
// spoken to it, and its answer is written back.
//
// A reply only talks to Ciel. It never allows a permission, which stays the
// user's click in the island, and it never starts a session: one the brain
// proposes waits for their click or their spoken yes at the desk
// (agent/launch.rs).
//
// No connection is kept open. Discord is asked for new messages over HTTPS:
// every few seconds while a conversation goes on, every few minutes otherwise
// while the user is away, and not at all while they are at their desk.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use crate::i18n::{t, tf};
use crate::settings::SpeakUp;
use crate::voice::brain::Brain;

/// The keychain key of the bot's token.
pub const TOKEN_KEY: &str = "discord-bot-token";

const API: &str = "https://discord.com/api/v10";
/// As Discord asks of a bot: its name, where it comes from, its version.
const USER_AGENT: &str = concat!("DiscordBot (https://github.com/jean-eudes-asg/ciel, ", env!("CARGO_PKG_VERSION"), ")");
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ANSWER: usize = 256 * 1024;
/// A message is 2000 characters at most there.
const MAX_MESSAGE: usize = 1900;
/// No more lines than this wait to be sent, and no more of the user's
/// messages than this are answered at once.
const MAX_WAITING: usize = 5;
const MAX_AT_ONCE: usize = 3;

/// A message came or went a moment ago: the next may follow at once.
const QUICK_FOR: Duration = Duration::from_secs(120);
const QUICK: Duration = Duration::from_secs(10);
/// The conversation may still go on.
const STEADY_FOR: Duration = Duration::from_secs(30 * 60);
const STEADY: Duration = Duration::from_secs(30);
/// Nothing said lately: while the user is away they may still write first.
const SLOW: Duration = Duration::from_secs(5 * 60);
/// Turned off, or refused by Discord: until something changes.
const OFF: Duration = Duration::from_secs(3600);
/// After a fault of the network.
const RETRY: Duration = Duration::from_secs(60);

static OUT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// The settings or the token changed: the channel is opened afresh.
static CHANGED: AtomicBool = AtomicBool::new(false);
/// The token as last read from the keychain; the outer none: not read yet.
static TOKEN: Mutex<Option<Option<String>>> = Mutex::new(None);

// ── What the user set up ──────────────────────────────────────────────────────

/// A Discord user ID: a number of 17 to 20 digits.
fn user_id(given: &str) -> Option<&str> {
    let id = given.trim();
    ((17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

fn token() -> Option<String> {
    let mut known = TOKEN.lock().unwrap();
    known.get_or_insert_with(|| crate::secrets::get(TOKEN_KEY)).clone()
}

/// Ciel can write to the user on Discord: turned on, with a token and their ID.
pub fn ready(prefs: &SpeakUp) -> bool {
    prefs.discord && user_id(&prefs.discord_user).is_some() && token().is_some()
}

/// The token was saved or removed (lib.rs).
pub fn token_changed() {
    *TOKEN.lock().unwrap() = None;
    settings_saved();
}

/// The settings were saved: looked at again with them.
pub fn settings_saved() {
    CHANGED.store(true, Ordering::Relaxed);
    WAKE.notify_one();
}

/// `text` is written to the user as soon as Discord can be reached (speakup.rs).
pub fn text(text: String) {
    {
        let mut out = OUT.lock().unwrap();
        if out.len() >= MAX_WAITING {
            return;
        }
        out.push_back(text);
    }
    WAKE.notify_one();
}

// ── Discord ───────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Fault {
    /// The token is not one Discord knows.
    Token,
    /// The bot may not write to that user (no server in common), or there is
    /// no such user.
    User,
    /// Too many requests: not before this long.
    Wait(Duration),
    /// The network, or anything else: said without the address.
    Other(String),
}

/// What an answer that isn't a success means.
fn fault(status: u16, body: &Value) -> Fault {
    match status {
        401 => Fault::Token,
        400 | 403 | 404 => Fault::User,
        429 => Fault::Wait(Duration::from_secs_f64(body["retry_after"].as_f64().unwrap_or(5.0).clamp(1.0, 600.0))),
        other => Fault::Other(format!("HTTP {other}")),
    }
}

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder().user_agent(USER_AGENT).connect_timeout(Duration::from_secs(10)).timeout(TIMEOUT).build().ok()
}

async fn call(client: &reqwest::Client, token: &str, post: bool, path: &str, body: Option<Value>) -> Result<Value, Fault> {
    let url = format!("{API}{path}");
    let request = if post { client.post(&url) } else { client.get(&url) };
    let request = request.header("Authorization", format!("Bot {token}"));
    let request = match body {
        Some(body) => request.json(&body),
        None => request,
    };
    // Never the error's own text: it can carry the address asked.
    let response = request.send().await.map_err(|e| Fault::Other(if e.is_timeout() { "timeout".into() } else { "network".into() }))?;
    let status = response.status();
    let bytes = crate::net::read_capped(response, MAX_ANSWER).await.map_err(Fault::Other)?;
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status.is_success() {
        Ok(value)
    } else {
        Err(fault(status.as_u16(), &value))
    }
}

fn id_of(value: &Value) -> Option<u64> {
    value["id"].as_str()?.parse().ok()
}

/// The direct messages between the bot and `user`: the channel's ID.
async fn open(client: &reqwest::Client, token: &str, user: &str) -> Result<String, Fault> {
    let channel = call(client, token, true, "/users/@me/channels", Some(json!({ "recipient_id": user }))).await?;
    channel["id"].as_str().filter(|id| id.bytes().all(|b| b.is_ascii_digit())).map(String::from).ok_or(Fault::User)
}

/// `text` as one message: cut at Discord's length, on a character.
fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_MESSAGE {
        return text.to_string();
    }
    text.chars().take(MAX_MESSAGE).chain("…".chars()).collect()
}

/// Writes `text` in the channel; the new message's ID. Nobody is pinged by
/// what it says.
async fn send(client: &reqwest::Client, token: &str, channel: &str, text: &str) -> Result<Option<u64>, Fault> {
    let body = json!({ "content": clip(text), "allowed_mentions": { "parse": [] } });
    call(client, token, true, &format!("/channels/{channel}/messages"), Some(body)).await.map(|m| id_of(&m))
}

/// What `user` wrote among `messages` (Discord's list, newest first) after the
/// message `after`, oldest first, and the newest ID seen. The bot's own lines
/// and anyone else's are passed over.
fn theirs(messages: &Value, user: &str, after: u64) -> (Vec<String>, u64) {
    let mut newest = after;
    let mut said: Vec<(u64, String)> = Vec::new();
    for message in messages.as_array().into_iter().flatten() {
        let Some(id) = id_of(message).filter(|id| *id > after) else { continue };
        newest = newest.max(id);
        let author = &message["author"];
        let text = message["content"].as_str().unwrap_or_default().trim();
        if author["id"].as_str() == Some(user) && author["bot"].as_bool() != Some(true) && !text.is_empty() {
            said.push((id, text.to_string()));
        }
    }
    said.sort_by_key(|(id, _)| *id);
    (said.into_iter().map(|(_, text)| text).collect(), newest)
}

/// How long until Discord is asked again; none: not until something happens.
/// `talked`: how long ago a message last came or went.
fn pace(talked: Option<Duration>, away: bool) -> Option<Duration> {
    match talked {
        Some(ago) if ago < QUICK_FOR => Some(QUICK),
        Some(ago) if ago < STEADY_FOR => Some(STEADY),
        _ if away => Some(SLOW),
        _ => None,
    }
}

/// What the brain is given: the user's words, and where they come from.
fn cue(text: &str) -> String {
    format!(
        "(The user wrote this to you on Discord, away from their computer: your answer is written back to them there, so keep it \
short. From here a coding session can't be started, and an agent's request is answered only by their reply with the code Ciel \
wrote them, never by you: say so if they ask.) {text}"
    )
}

// ── The loop ──────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Link {
    client: Option<reqwest::Client>,
    /// The user the channel was opened with, and the channel.
    channel: Option<(String, String)>,
    /// The newest message seen there.
    last: u64,
    /// When a message last came or went.
    talked: Option<Instant>,
    /// Discord refused the token or the user: not asked again until one changes.
    refused: bool,
    /// The conversation held there, apart from the spoken one.
    brain: Arc<Mutex<Brain>>,
}

fn prefs(app: &AppHandle) -> Option<SpeakUp> {
    app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().speak_up.clone())
}

impl Link {
    /// One look, and how long until the next.
    async fn tick(&mut self, app: &AppHandle) -> Duration {
        if CHANGED.swap(false, Ordering::Relaxed) {
            self.channel = None;
            self.refused = false;
        }
        // Off, or not set up: nothing is read, not even the keychain.
        let Some(prefs) = prefs(app).filter(|p| p.discord && user_id(&p.discord_user).is_some()) else {
            OUT.lock().unwrap().clear();
            return OFF;
        };
        // The keychain can take a moment: off the async threads.
        let Some(token) = tauri::async_runtime::spawn_blocking(token).await.ok().flatten() else {
            OUT.lock().unwrap().clear();
            return OFF;
        };
        if self.refused {
            OUT.lock().unwrap().clear();
            return OFF;
        }
        let away = !crate::desk::there(&prefs);
        let waiting = !OUT.lock().unwrap().is_empty();
        if !waiting && pace(self.talked.map(|t| t.elapsed()), away).is_none() {
            // At their desk with nothing going on: Discord isn't asked. Looked
            // at again in a while, in case they left.
            return SLOW;
        }
        match self.exchange(app, &prefs, &token).await {
            Ok(()) => pace(self.talked.map(|t| t.elapsed()), away).unwrap_or(SLOW),
            Err(Fault::Wait(wait)) => wait,
            Err(Fault::Other(why)) => {
                crate::log::line(format!("discord: not reached ({why})"));
                RETRY
            }
            Err(refused) => {
                crate::log::line(format!("discord: refused ({})", if refused == Fault::Token { "the token" } else { "the user" }));
                self.refused = true;
                OFF
            }
        }
    }

    /// Sends what waits, then answers what the user wrote.
    async fn exchange(&mut self, app: &AppHandle, prefs: &SpeakUp, token: &str) -> Result<(), Fault> {
        let user = user_id(&prefs.discord_user).ok_or(Fault::User)?.to_string();
        if self.client.is_none() {
            self.client = client();
        }
        let client = self.client.clone().ok_or_else(|| Fault::Other("no client".into()))?;
        if self.channel.as_ref().is_none_or(|(of, _)| *of != user) {
            let channel = open(&client, token, &user).await?;
            // What was written before now is not answered.
            let newest = call(&client, token, false, &format!("/channels/{channel}/messages?limit=1"), None).await?;
            self.last = newest.as_array().and_then(|list| list.first()).and_then(id_of).unwrap_or(0);
            self.channel = Some((user.clone(), channel));
        }
        let channel = self.channel.as_ref().map(|(_, channel)| channel.clone()).ok_or(Fault::User)?;
        loop {
            let Some(line) = OUT.lock().unwrap().pop_front() else { break };
            send(&client, token, &channel, &line).await?;
            self.talked = Some(Instant::now());
            // That one went: never what it says.
            crate::log::line("discord: a line sent");
        }
        let messages = call(&client, token, false, &format!("/channels/{channel}/messages?after={}&limit=20", self.last), None).await?;
        let (said, newest) = theirs(&messages, &user, self.last);
        self.last = newest;
        let skip = said.len().saturating_sub(MAX_AT_ONCE);
        for text in said.into_iter().skip(skip) {
            self.talked = Some(Instant::now());
            crate::log::line("discord: the user wrote");
            // « allow 4821 »: an agent's request, answered by Ciel's code (asked.rs).
            let answer = match crate::asked::by_code(app, &text) {
                Some(answer) => answer,
                None => self.answer(app, text).await,
            };
            send(&client, token, &channel, &answer).await?;
            self.talked = Some(Instant::now());
        }
        Ok(())
    }

    /// The brain's answer to what the user wrote, as for words spoken to it.
    async fn answer(&self, app: &AppHandle, text: String) -> String {
        let (brain, asker, question) = (self.brain.clone(), app.clone(), cue(&text));
        // Written, not spoken: nothing this turn may answer an agent's request.
        crate::asked::discord_turn(true);
        let reply = tauri::async_runtime::spawn_blocking(move || brain.lock().unwrap_or_else(|e| e.into_inner()).respond(&asker, &question))
            .await
            .ok()
            .flatten();
        crate::asked::discord_turn(false);
        let said = match reply {
            Some(reply) if !reply.text.trim().is_empty() => reply.text,
            Some(reply) if reply.acted => "✓".into(),
            _ => t("Ciel couldn't answer just now."),
        };
        // Noted for the next conversation, if the user asked Ciel to remember.
        crate::talks::note(app, &text, &said);
        said
    }
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(25)).await;
        let mut link = Link::default();
        loop {
            let wait = link.tick(&app).await;
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = WAKE.notified() => {}
            }
        }
    });
}

// ── Settings ──────────────────────────────────────────────────────────────────

/// Ciel window → Chat → Speak up → Send a test message: one line to the
/// user, on their click, to see that the bot can write to them.
#[tauri::command]
pub async fn discord_test(app: AppHandle) -> Result<(), String> {
    let prefs = prefs(&app).ok_or_else(|| "no settings".to_string())?;
    let user = user_id(&prefs.discord_user).ok_or_else(|| t("Enter your Discord user ID first: a number of 17 to 20 digits."))?.to_string();
    let token = tauri::async_runtime::spawn_blocking(token).await.ok().flatten().ok_or_else(|| t("Save the bot's token first."))?;
    let client = client().ok_or_else(|| "no client".to_string())?;
    let sent = async {
        let channel = open(&client, &token, &user).await?;
        send(&client, &token, &channel, &t("Ciel can write to you here.")).await
    };
    let sent = sent.await;
    // What became of it, never the token nor the user's ID: the test leaves a
    // line, so a message that didn't go can be told apart from one not looked for.
    crate::log::line(match &sent {
        Ok(_) => "discord: the test was sent".to_string(),
        Err(Fault::Token) => "discord: the test was refused (the token)".to_string(),
        Err(Fault::User) => "discord: the test was refused (the user)".to_string(),
        Err(Fault::Wait(_)) => "discord: the test waits (429)".to_string(),
        Err(Fault::Other(why)) => format!("discord: the test wasn't sent ({why})"),
    });
    sent.map(|_| ()).map_err(|fault| match fault {
        Fault::Token => t("Discord doesn't know this token."),
        Fault::User => t("The bot can't write to you: add it to a server you are in, check your user ID, then try again."),
        Fault::Wait(_) => tf("Discord didn't answer: {error}", &[("error", "429")]),
        Fault::Other(why) => tf("Discord didn't answer: {error}", &[("error", &why)]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: &str = "123456789012345678";

    #[test]
    fn only_a_user_id_is_written_to() {
        assert_eq!(user_id(" 123456789012345678 "), Some(ME));
        for bad in ["", "me", "1234", "12345678901234567890123", "12345678901234567x", "123456789012345678/../x"] {
            assert_eq!(user_id(bad), None, "{bad}");
        }
        // Off, or without an ID: nothing is asked of the keychain or of Discord.
        assert!(!ready(&SpeakUp::default()));
        assert!(!ready(&SpeakUp { discord: true, ..SpeakUp::default() }));
    }

    #[test]
    fn only_what_the_user_wrote_since_is_answered_oldest_first() {
        let messages = json!([
            { "id": "40", "content": "and then?", "author": { "id": ME } },
            { "id": "30", "content": "Codex has finished in site.", "author": { "id": "999", "bot": true } },
            { "id": "20", "content": " where do things stand? ", "author": { "id": ME } },
            { "id": "15", "content": "hello", "author": { "id": "555555555555555555" } },
            { "id": "12", "content": "", "author": { "id": ME } },
            { "id": "10", "content": "old news", "author": { "id": ME } },
        ]);
        let (said, newest) = theirs(&messages, ME, 10);
        assert_eq!(said, ["where do things stand?", "and then?"]);
        assert_eq!(newest, 40);
        // Nothing new, or nothing that reads as a list: as it was.
        assert_eq!(theirs(&messages, ME, 40), (Vec::new(), 40));
        assert_eq!(theirs(&json!({ "message": "401: Unauthorized" }), ME, 7), (Vec::new(), 7));
        assert_eq!(theirs(&json!([{ "id": "x", "content": "hi", "author": { "id": ME } }]), ME, 7), (Vec::new(), 7));
    }

    #[test]
    fn discord_is_asked_often_in_a_conversation_seldom_while_away_never_at_the_desk() {
        let ago = |secs: u64| Some(Duration::from_secs(secs));
        assert_eq!(pace(ago(5), false), Some(QUICK));
        assert_eq!(pace(ago(300), true), Some(STEADY));
        assert_eq!(pace(ago(3 * 3600), true), Some(SLOW));
        assert_eq!(pace(None, true), Some(SLOW));
        assert_eq!(pace(None, false), None);
        assert_eq!(pace(ago(3 * 3600), false), None);
    }

    #[test]
    fn a_refusal_says_which_and_too_many_requests_says_how_long() {
        assert_eq!(fault(401, &json!({ "message": "401: Unauthorized" })), Fault::Token);
        assert_eq!(fault(403, &json!({ "code": 50007 })), Fault::User);
        assert_eq!(fault(404, &Value::Null), Fault::User);
        assert_eq!(fault(429, &json!({ "retry_after": 2.5 })), Fault::Wait(Duration::from_millis(2500)));
        assert_eq!(fault(429, &json!({ "retry_after": 99999 })), Fault::Wait(Duration::from_secs(600)));
        assert_eq!(fault(502, &Value::Null), Fault::Other("HTTP 502".into()));
    }

    #[test]
    fn a_message_fits_discord_and_the_brain_knows_where_it_comes_from() {
        assert_eq!(clip("  Codex needs you in site.  "), "Codex needs you in site.");
        let long = "é".repeat(5000);
        assert_eq!(clip(&long).chars().count(), MAX_MESSAGE + 1);
        let cue = cue("where do things stand?");
        assert!(cue.ends_with("where do things stand?"));
        assert!(cue.contains("Discord") && cue.contains("can't be started"));
        // The bot says who it is, as Discord asks.
        assert!(USER_AGENT.starts_with("DiscordBot ("));
    }
}
