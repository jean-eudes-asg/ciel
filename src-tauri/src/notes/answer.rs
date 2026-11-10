// The answer key (Ctrl+Alt+K, or the button on the Notetaker's strip): during
// a call the Notetaker records, Ciel finds the last question the others put to
// the user and suggests an answer on a card in the island, kept in the call's
// note. Pressed with no call noted, a card asks first, and only its Take notes
// starts the Notetaker.
//
// Quick, because the slow parts are done before the press or while it shows:
// - the card shows at the press, saying Ciel is thinking, and the answer is
//   written into it as the brain writes it, its first line a sentence to say
//   as it is; once whole, it has Copy;
// - the words just said are transcribed at the press, in order with the live
//   transcript (the gate in mod.rs), so the question is never left out;
// - the company folder is read when the call starts (folder.rs); at a press
//   only what changed since is read again;
// - with Claude, the call goes in blocks of two minutes, and the API keeps the
//   finished ones cached from one question to the next;
// - with « Prepare answers ahead » on (off by default), a question the others
//   ask is answered at once, before the press: the key's button lights up, and
//   the press shows the answer.
// While an answer is on its card, Ciel is in screen shares (capture.rs), unless
// the user keeps the answers to themselves (`answers_in_shares`). The brain is
// the user's own Claude when they chose it for Ciel's (agent/, in a build that
// has it), else Claude with its key or a local model. The log says how long
// each step took.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::{builder, folder, Answer, Line, Note, NoteTaker, Status};
use crate::ai::{self, Engine};
use crate::flow::pill;
use crate::i18n::{t, tf};
use crate::notify::{self, Button, Target};

const ANSWER_MODEL: &str = "claude-haiku-5-5";
/// What a local model, or the user's own Claude, is given of the call: a long
/// prompt is read slowly.
const LOCAL_WINDOW_S: f64 = 180.0;
/// What Claude is given of the call: this many blocks of `BLOCK_S` seconds.
const CLAUDE_BLOCKS: u64 = 15;
const BLOCK_S: f64 = 120.0;
/// The company folder's passages sent with the question, at most (characters).
const FOLDER_BUDGET: usize = 6000;
/// The folder is looked at again at most this often.
const FOLDER_CHECK: Duration = Duration::from_secs(5);
const MAX_TOKENS: u32 = 500;
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(30);
const LOCAL_TIMEOUT: Duration = Duration::from_secs(90);
/// An answer's card stays this long, or until dismissed, and is in screen
/// shares meanwhile. So do the card that waits for it, until the answer or a
/// word is on it, and the one that asks before recording.
const SHOWN_MS: u64 = 3 * 60_000;
/// A prepared answer is offered this long after its question was heard.
const PREPARED_FOR: Duration = Duration::from_secs(120);
/// At most this many answers prepared in one call.
const MAX_PREPARED: u32 = 200;
/// The card is written into at most this often while the answer comes.
const CARD_INTERVAL: Duration = Duration::from_millis(1000 / 12);
/// The answer key's cards share a tag: a new one takes the place of the last.
const TAG: &str = "meeting-answer";

/// What the brain is asked. The answer is read at a glance, while speaking:
/// its first sentence has to do alone.
const SYSTEM: &str = "You help the user during a live call, which they are recording with the other people's knowledge. \
In the transcript, \"Me\" is the user and \"Them\" the others. \
Answer the question named at the end; if none is named, the most recent question or request put to the user. \
The user reads your answer at a glance while speaking.\n\
Reply in plain text, without Markdown, in exactly this form:\n\
Q: the question, in a few words\n\
A: one sentence the user can say as it is, in the first person, in the language of the call, \
with the figure, the date or the yes or no that was asked for.\n\
Then, only if it helps, one to three short sentences more: a detail, or what to check.\n\
Use the company's files and earlier calls when they hold the answer. Never invent a figure, a date or a promise: \
if the answer needs facts you don't have, say in a few words what to check, and give a sensible answer meanwhile.";

/// The answer key's side of the Notetaker: the answer being written or ready,
/// and the company folder as read for this call.
#[derive(Default)]
pub struct Desk {
    job: Mutex<Option<Job>>,
    /// The folder as read, and when it was last looked at.
    index: Mutex<Option<(folder::Index, Instant)>>,
    seq: AtomicU64,
    prepared: AtomicU32,
    /// The last card dismissed: nothing more is written for it.
    gone: Mutex<Option<String>>,
}

/// One answer, being written or written: for a press, or prepared ahead.
struct Job {
    seq: u64,
    note: String,
    /// The question it answers, when prepared: the line Ciel heard.
    line: Option<String>,
    started: Instant,
    /// Seconds into the call.
    at: f64,
    text: String,
    done: bool,
    /// Why there is no answer, for the card.
    failed: Option<String>,
    /// The card showing it, once the key was pressed for it.
    card: Option<String>,
    last_card: Option<Instant>,
    /// Its first words are on the card, and Ciel in screen shares for them.
    shared: bool,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for Job {
    /// A job replaced stops: its request goes with it.
    fn drop(&mut self) {
        if !self.done {
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }
    }
}

/// One press at a time, from the key to the answer being written: another
/// meanwhile changes nothing.
static PRESSING: AtomicBool = AtomicBool::new(false);

/// Held while a press is looked into.
struct Pressing;

impl Pressing {
    /// None while an earlier press is still looked into.
    fn begin() -> Option<Self> {
        (!PRESSING.swap(true, Ordering::AcqRel)).then_some(Self)
    }
}

impl Drop for Pressing {
    fn drop(&mut self) {
        PRESSING.store(false, Ordering::Release);
    }
}

// ── What is said ──────────────────────────────────────────────────────────────

/// Words a question starts with, when the recognizer left out its mark.
const ASKING: [&str; 21] = [
    "what", "what's", "whats", "how", "how's", "why", "which", "who", "whose", "pourquoi", "combien", "est-ce",
    "qu'est-ce", "quel", "quelle", "quels", "quelles", "warum", "wieso", "perché", "почему",
];
const AUXILIARY: [&str; 14] = ["can", "could", "would", "will", "do", "does", "did", "is", "are", "have", "has", "should", "was", "were"];
const SUBJECT: [&str; 11] = ["you", "we", "i", "it", "there", "they", "he", "she", "this", "that", "your"];

/// The others asked something: a question mark, or a sentence that starts as
/// questions do ("what…", "could you…", « pouvez-vous… »).
pub fn is_question(text: &str) -> bool {
    let text = text.trim();
    if text.contains(['?', '¿', '？', '؟']) {
        return true;
    }
    let mut words = text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-').to_lowercase());
    let Some(first) = words.next() else { return false };
    let second = words.next().unwrap_or_default();
    ASKING.contains(&first.as_str())
        || (AUXILIARY.contains(&first.as_str()) && SUBJECT.contains(&second.as_str()))
        || ["-vous", "-tu", "-on", "-il", "-elle", "-ils", "-elles"].iter().any(|end| first.ends_with(end))
}

/// The last thing the others said that sounds like a question, if it is one of
/// their last two lines in the last minutes.
pub fn latest_question(lines: &[Line], now_s: f64) -> Option<&Line> {
    lines
        .iter()
        .rev()
        .filter(|l| l.speaker != builder::ME && l.end >= now_s - LOCAL_WINDOW_S)
        .take(2)
        .find(|l| is_question(&l.text))
}

fn said(line: &Line) -> String {
    format!("{}: {}", if line.speaker == builder::ME { "Me" } else { "Them" }, line.text.trim())
}

/// The last minutes of the call as the brain reads them: "Me:" and "Them:".
pub fn recent_transcript(lines: &[Line], now_s: f64) -> String {
    lines.iter().filter(|l| l.end >= now_s - LOCAL_WINDOW_S).map(said).collect::<Vec<_>>().join("\n")
}

/// The call in blocks of two minutes, the last `CLAUDE_BLOCKS` of them: the
/// finished blocks, which stay the same from one question to the next (the
/// API caches them), and the one going on.
pub fn transcript_blocks(lines: &[Line], now_s: f64) -> (Vec<String>, String) {
    let current = (now_s.max(0.0) / BLOCK_S) as u64;
    let first = current.saturating_sub(CLAUDE_BLOCKS - 1);
    let mut done: Vec<(u64, Vec<String>)> = Vec::new();
    let mut now = Vec::new();
    for l in lines {
        let block = (l.start.max(0.0) / BLOCK_S) as u64;
        if block < first {
            continue;
        }
        if block >= current {
            now.push(said(l));
        } else {
            match done.last_mut() {
                Some((b, lines)) if *b == block => lines.push(said(l)),
                _ => done.push((block, vec![said(l)])),
            }
        }
    }
    (done.into_iter().map(|(_, lines)| lines.join("\n")).collect(), now.join("\n"))
}

/// The earlier calls that share the most words with the question: their
/// summary and what is still to do, three at most.
pub fn related_calls(notes: &[Note], current: &str, words: &[String]) -> String {
    let mut scored: Vec<(usize, &Note)> = notes
        .iter()
        .filter(|n| n.id != current && n.status == Status::Ready)
        .map(|n| {
            let text = super::searchable(n);
            (words.iter().filter(|w| text.contains(w.as_str())).count(), n)
        })
        .filter(|(score, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.start.cmp(&a.1.start)));
    scored
        .into_iter()
        .take(3)
        .map(|(_, n)| {
            let todo: Vec<&str> = n.todos.iter().filter(|t| !t.done).map(|t| t.text.as_str()).collect();
            let todo = if todo.is_empty() { String::new() } else { format!("\nStill to do: {}", todo.join("; ")) };
            format!("{} ({}): {}{todo}", n.title, n.start_local, n.summary.as_deref().unwrap_or(""))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One part of a reply, without what is left of a label's bold around it.
fn part(s: &str) -> String {
    s.trim().trim_matches('*').trim().to_string()
}

/// Where the answer's label starts a line of the reply, in bold or not: the
/// "A:" of "SLA:" in the question isn't it.
fn answer_label(reply: &str) -> Option<usize> {
    let mut at = 0;
    for line in reply.split_inclusive('\n') {
        let lead = line.len() - line.trim_start_matches(|c: char| c == '*' || c.is_whitespace()).len();
        if line[lead..].starts_with("A:") {
            return Some(at + lead);
        }
        at += line.len();
    }
    None
}

/// "Q: …" then "A: …" → (question, answer); without them, the whole reply is
/// the answer. A brain that writes its labels in bold ("**Q:**") says the same.
pub fn parse_answer(reply: &str) -> (String, String) {
    let reply = reply.trim();
    let q = reply.find("Q:");
    // On a line of its own as asked, else wherever it is.
    let a = answer_label(reply).or_else(|| reply.find("A:"));
    match (q, a) {
        (Some(q), Some(a)) if q < a => (part(&reply[q + 2..a]), part(&reply[a + 2..])),
        (None, Some(a)) => (String::new(), part(&reply[a + 2..])),
        _ => (String::new(), reply.to_string()),
    }
}

/// The same, while the reply is still being written: a label half written is
/// nothing yet, and a question whose answer hasn't come is only the question.
pub fn parse_partial(reply: &str) -> (String, String) {
    let reply = reply.trim_start();
    let start = reply.trim_start_matches('*');
    if start.len() < 2 && "Q:".starts_with(start) {
        return (String::new(), String::new());
    }
    match start.strip_prefix("Q:") {
        Some(question) if answer_label(reply).is_none() => (part(question.lines().next().unwrap_or_default()), String::new()),
        _ => parse_answer(reply),
    }
}

// ── What the brain is given ───────────────────────────────────────────────────

/// Who writes the answer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Route {
    /// The user's own Claude (agent/).
    Agent,
    /// Claude, with the user's key.
    Claude,
    Local,
}

/// One question to the brain: the call first (stable, cached by Claude), then
/// what changes with each question.
struct Prompt {
    /// The meeting, if the calendar named it.
    meeting: String,
    /// Claude's finished blocks of the call.
    blocks: Vec<String>,
    /// The block going on (Claude), or the last minutes (the others), then
    /// the files, the earlier calls and which question to answer.
    tail: String,
}

impl Prompt {
    fn claude_body(&self) -> Value {
        let mut content: Vec<Value> = Vec::new();
        if !self.meeting.is_empty() {
            content.push(json!({ "type": "text", "text": self.meeting }));
        }
        for (i, block) in self.blocks.iter().enumerate() {
            let text = if i == 0 { format!("Transcript of the call:\n{block}") } else { block.clone() };
            content.push(json!({ "type": "text", "text": text }));
        }
        // The call up to the block going on stays the same until the next question.
        if !self.blocks.is_empty() {
            if let Some(last) = content.last_mut() {
                last["cache_control"] = json!({ "type": "ephemeral" });
            }
        }
        content.push(json!({ "type": "text", "text": self.tail }));
        json!({
            "model": ANSWER_MODEL,
            "max_tokens": MAX_TOKENS,
            "system": SYSTEM,
            // Said in a live call: the first words matter more than long thought.
            "thinking": { "type": "disabled" },
            "output_config": { "effort": "low" },
            "messages": [{ "role": "user", "content": content }],
        })
    }

    /// The whole of it as one text: a local model's, or the user's own Claude's.
    fn plain(&self) -> String {
        if self.meeting.is_empty() {
            self.tail.clone()
        } else {
            format!("{}\n\n{}", self.meeting, self.tail)
        }
    }
}

/// The words to look for in the folder and the earlier calls: the question,
/// the others' last words, and the meeting's title.
fn search_words(note: &Note, question: Option<&Line>, now_s: f64) -> Vec<String> {
    let mut text: Vec<&str> = question.map(|l| l.text.as_str()).into_iter().collect();
    text.extend(
        note.lines.iter().rev().filter(|l| l.speaker != builder::ME && l.end >= now_s - LOCAL_WINDOW_S).take(2).map(|l| l.text.as_str()),
    );
    text.extend(note.meeting.as_ref().map(|m| m.title.as_str()));
    folder::words(&text.join(" "))
}

/// The passages of the company folder that fit: the folder is read when the
/// call starts, and only what changed since is read again.
fn folder_passages(desk: &Desk, dir: &str, words: &[String]) -> String {
    let mut slot = desk.index.lock().unwrap();
    let stale = slot.as_ref().is_none_or(|(index, _)| index.dir() != Path::new(dir));
    if stale {
        *slot = Some((folder::Index::new(Path::new(dir)), Instant::now()));
        slot.as_mut().unwrap().0.refresh();
    } else if let Some((index, checked)) = slot.as_mut().filter(|(_, at)| at.elapsed() >= FOLDER_CHECK) {
        index.refresh();
        *checked = Instant::now();
    }
    let (index, _) = slot.as_ref().unwrap();
    index.search(words, FOLDER_BUDGET).iter().map(|(name, p)| format!("[{name}]\n{p}")).collect::<Vec<_>>().join("\n\n")
}

async fn prompt(app: &AppHandle, taker: &Arc<NoteTaker>, route: Route, note: &Note, question: Option<&Line>, now_s: f64) -> Prompt {
    let meeting = note
        .meeting
        .as_ref()
        .map(|m| {
            let agenda = m.agenda.as_deref().map(|a| format!("\nAgenda: {a}")).unwrap_or_default();
            format!("The meeting: {}{agenda}", m.title)
        })
        .unwrap_or_default();
    let words = search_words(note, question, now_s);
    let (company, reads_notes) = {
        let shared = app.state::<crate::Shared>();
        let s = shared.settings.lock().unwrap();
        (s.company_folder.trim().to_string(), s.brain_reads.notes)
    };
    let files = if company.is_empty() {
        String::new()
    } else {
        let taker = taker.clone();
        let words = words.clone();
        tauri::async_runtime::spawn_blocking(move || folder_passages(&taker.desk, &company, &words)).await.unwrap_or_default()
    };
    // Earlier calls on the same subject: to Anthropic only if the user lets the brain read the notes.
    let earlier = if route == Route::Local || reads_notes {
        let notes = taker.notes.lock().unwrap().clone();
        related_calls(&notes, &note.id, &words)
    } else {
        String::new()
    };
    let (blocks, now) = match route {
        Route::Claude => transcript_blocks(&note.lines, now_s),
        Route::Agent | Route::Local => (Vec::new(), recent_transcript(&note.lines, now_s)),
    };
    let mut tail = String::new();
    if !now.is_empty() {
        tail.push_str(if blocks.is_empty() { "Transcript of the last minutes:\n" } else { "Then:\n" });
        tail.push_str(&now);
        tail.push_str("\n\n");
    }
    if !files.is_empty() {
        tail.push_str(&format!("From the company's files:\n{files}\n\n"));
    }
    if !earlier.is_empty() {
        tail.push_str(&format!("Earlier calls:\n{earlier}\n\n"));
    }
    match question {
        Some(q) => tail.push_str(&format!("The question to answer: « {} »", q.text.trim())),
        None => tail.push_str("Answer the most recent question or request put to the user."),
    }
    Prompt { meeting, blocks, tail }
}

// ── The cards ─────────────────────────────────────────────────────────────────

/// One of the answer key's cards, as the island gets it. `stays`: for
/// `SHOWN_MS`, rather than as long as any notification (Settings →
/// Notifications).
fn key_card(id: String, title: String, body: String, buttons: Vec<Button>, stays: bool, now: u64) -> notify::Notification {
    notify::Notification {
        id,
        source: "ciel".into(),
        app: "Ciel".into(),
        title,
        body,
        icon: None,
        at: now as f64,
        tag: Some(TAG.into()),
        buttons,
        until: stays.then(|| (now + SHOWN_MS) as f64),
        time: None,
    }
}

fn show_card(app: &AppHandle, id: &str, title: String, body: String, buttons: Vec<Button>, target: Target, stays: bool) {
    notify::show(app, key_card(id.to_string(), title, body, buttons, stays, super::now_ms()), target);
}

/// A word from the answer key that isn't an answer, on a card of its own: it
/// takes an answer's place in the island, and Ciel is left out of screen
/// shares again, if so set. It folds away as any notification does.
pub(super) fn word(app: &AppHandle, title: String, body: String) {
    crate::capture::answer_gone(app);
    show_card(app, &notify::next_id("answer"), title, body, Vec::new(), Target::None, false);
}

/// The same on the card that waited for an answer, when none came.
fn say(app: &AppHandle, card: &str, body: String) {
    crate::capture::answer_gone(app);
    notify::update(app, notify::Update { id: card.to_string(), title: t("Answer"), body, buttons: None, until: Some(notify::AS_ANY) });
}

fn thinking() -> String {
    tf("{0} is thinking…", &[("0", "Ciel")])
}

fn no_answer() -> String {
    t("Ciel's brain didn't answer. Try the key again.")
}

/// What the card says of an answer so far: its question for a title once the
/// brain has named it, and what there is of the answer.
fn card_text(text: &str, done: bool) -> (String, String) {
    let (question, answer) = if done { parse_answer(text) } else { parse_partial(text) };
    let title = if !question.is_empty() {
        question
    } else if answer.is_empty() {
        thinking()
    } else {
        t("Answer")
    };
    (title, answer)
}

/// The others may see the answers (Settings → General).
fn in_shares(app: &AppHandle) -> bool {
    app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().unwrap().answers_in_shares)
}

fn set_ready(app: &AppHandle, ready: bool) {
    let value = ready.then_some("ready");
    if pill::get(app).is_some_and(|p| p.state().note_answer == value) {
        return;
    }
    pill::update(app, |s| s.note_answer = value);
}

/// The job is done and shown: the answer whole on its card, with Copy, and
/// kept in the note; or the word that says why there is none.
fn conclude(app: &AppHandle, taker: &NoteTaker, card: &str, note: &str, at: f64, text: &str, failed: Option<&str>) {
    if let Some(why) = failed {
        say(app, card, why.to_string());
        return;
    }
    let (question, answer) = parse_answer(text);
    if answer.is_empty() {
        say(app, card, no_answer());
        return;
    }
    // In the share before it shows whole, so the others see it as the user
    // does, unless the user keeps the answers to themselves.
    if in_shares(app) {
        crate::capture::answer_shown(app, SHOWN_MS);
    }
    // Copy puts the answer on the clipboard, and its card stays (notify.rs).
    notify::retarget(card, Target::Answer(answer.clone()));
    notify::update(
        app,
        notify::Update {
            id: card.to_string(),
            title: if question.is_empty() { t("Answer") } else { question.clone() },
            body: answer.clone(),
            buttons: Some(vec![Button { id: "copy".into(), label: t("Copy") }]),
            until: Some((super::now_ms() + SHOWN_MS) as f64),
        },
    );
    taker.change(app, note, |n| n.answers.push(Answer { at, question, answer }));
}

// ── Writing an answer ─────────────────────────────────────────────────────────

/// Where the time went, for the log.
struct Timing {
    /// The press, or when the question was heard.
    from: Instant,
    /// The words just said, transcribed at the press.
    words: Option<Duration>,
    kind: &'static str,
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

/// Why there is no answer.
enum Failure {
    /// No Claude key, no local model, and no Claude of the user's own.
    NoBrain,
    /// Claude's safeguards declined the question (`stop_reason: "refusal"`).
    Declined,
    Error(String),
}

/// Starts writing an answer, in place of the one before. `card`: the press's
/// card, which shows it as it comes; none for an answer prepared ahead.
/// `agent`: whether the user's own Claude answers, when the press has looked
/// already.
fn begin(app: &AppHandle, taker: &Arc<NoteTaker>, note: Note, question: Option<Line>, card: Option<String>, agent: Option<bool>, timing: Timing) {
    let seq = taker.desk.seq.fetch_add(1, Ordering::Relaxed) + 1;
    let now_s = super::now_ms().saturating_sub(note.start) as f64 / 1000.0;
    *taker.desk.job.lock().unwrap() = Some(Job {
        seq,
        note: note.id.clone(),
        line: question.as_ref().map(|l| l.id.clone()),
        started: Instant::now(),
        at: now_s,
        text: String::new(),
        done: false,
        failed: None,
        card,
        last_card: None,
        shared: false,
        task: None,
    });
    set_ready(app, false);
    let (app2, taker2) = (app.clone(), taker.clone());
    let task = tauri::async_runtime::spawn(async move {
        let (app, taker) = (app2, taker2);
        // The user's own Claude when they chose it for Ciel's brain, else
        // Claude with its key or a local model.
        let agent = match agent {
            Some(known) => known,
            None => {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || crate::agent::wanted(&app)).await.unwrap_or(false)
            }
        };
        let engine = ai::current(&app);
        let mut context = Duration::ZERO;
        let mut first: Option<Duration> = None;
        let mut usage = None;
        let mut by = "no brain";
        let mut reply: Option<String> = None;
        if agent {
            let asked = Instant::now();
            let ask = prompt(&app, &taker, Route::Agent, &note, question.as_ref(), now_s).await.plain();
            context = asked.elapsed();
            let answer = tauri::async_runtime::spawn_blocking(move || crate::agent::complete(SYSTEM, &ask)).await.ok().flatten();
            reply = answer.map(|r| ai::visible_text(&r).trim().to_string()).filter(|r| !r.is_empty());
            if reply.is_some() {
                by = "the user's claude";
            }
        }
        let result = match (reply, &engine) {
            (Some(text), _) => Ok(text),
            (None, Some(Engine::Claude)) => {
                by = "claude";
                let asked = Instant::now();
                let body = prompt(&app, &taker, Route::Claude, &note, question.as_ref(), now_s).await.claude_body();
                context = asked.elapsed();
                let on_text = |text: &str| {
                    first.get_or_insert_with(|| timing.from.elapsed());
                    progress(&app, &taker, seq, text);
                };
                match crate::claude::stream(&body, CLAUDE_TIMEOUT, on_text).await {
                    Ok(s) if s.stop_reason.as_deref() == Some("refusal") => Err(Failure::Declined),
                    Ok(s) => {
                        let text = s.text.clone();
                        usage = Some(s);
                        Ok(text)
                    }
                    Err(err) => Err(Failure::Error(err)),
                }
            }
            (None, Some(Engine::Local(local))) => {
                by = "local";
                let asked = Instant::now();
                let user = prompt(&app, &taker, Route::Local, &note, question.as_ref(), now_s).await.plain();
                context = asked.elapsed();
                let on_text = |text: &str| {
                    first.get_or_insert_with(|| timing.from.elapsed());
                    progress(&app, &taker, seq, text);
                };
                let how = ai::Ask { max_tokens: MAX_TOKENS, json: false, context: None, timeout: LOCAL_TIMEOUT };
                ai::stream_complete(local, SYSTEM, &user, how, on_text).await.map_err(Failure::Error)
            }
            (None, None) if agent => Err(Failure::Error("the user's own Claude gave no answer".into())),
            (None, None) => Err(Failure::NoBrain),
        };
        let words = timing.words.map(|d| format!("words {} ms, ", ms(d))).unwrap_or_default();
        let first = first.map(|d| format!("{} ms", ms(d))).unwrap_or_else(|| "none".into());
        let tokens = usage.map(|u| format!("; prompt {} tokens + {} from the cache", u.input_tokens, u.cache_read)).unwrap_or_default();
        crate::log::line(format!(
            "notes: answer ({}, {by}): {words}context {} ms, first words {first}, done {} ms{tokens}",
            timing.kind,
            ms(context),
            ms(timing.from.elapsed()),
        ));
        finish(&app, &taker, seq, result);
    });
    if let Some(job) = taker.desk.job.lock().unwrap().as_mut().filter(|j| j.seq == seq && !j.done) {
        job.task = Some(task);
    }
}

/// More of the answer: on the card if it shows, at most `CARD_INTERVAL` apart.
fn progress(app: &AppHandle, taker: &NoteTaker, seq: u64, text: &str) {
    let (card, title, body, first_words) = {
        let mut slot = taker.desk.job.lock().unwrap();
        let Some(job) = slot.as_mut().filter(|j| j.seq == seq) else { return };
        job.text = text.to_string();
        let Some(card) = job.card.clone() else { return };
        if job.last_card.is_some_and(|at| at.elapsed() < CARD_INTERVAL) {
            return;
        }
        job.last_card = Some(Instant::now());
        let (title, body) = card_text(text, false);
        let first_words = !body.is_empty() && !job.shared;
        job.shared |= first_words;
        (card, title, body, first_words)
    };
    // In the share as its first words show, so the others see it as the user
    // does, unless the user keeps the answers to themselves.
    if first_words && in_shares(app) {
        crate::capture::answer_shown(app, SHOWN_MS);
    }
    notify::update(app, notify::Update::text(&card, title, body));
}

fn finish(app: &AppHandle, taker: &NoteTaker, seq: u64, result: Result<String, Failure>) {
    let shown = {
        let mut slot = taker.desk.job.lock().unwrap();
        let Some(job) = slot.as_mut().filter(|j| j.seq == seq) else { return };
        job.done = true;
        job.task = None;
        match result {
            Ok(text) if !parse_answer(&text).1.is_empty() => job.text = text,
            Ok(_) => {
                crate::log::line("notes: the brain's answer was empty");
                job.failed = Some(no_answer());
            }
            Err(Failure::NoBrain) => {
                crate::log::line("notes: the answer key has no brain (no Claude key, no local model, and no signed-in claude of the user's)");
                job.failed = Some(t("Ciel needs Claude or a local model for this: set one up in Island → Chat."));
            }
            Err(Failure::Declined) => {
                crate::log::line("notes: the brain declined the question");
                job.failed = Some(t("Ciel's brain declined this question."));
            }
            Err(Failure::Error(err)) => {
                crate::log::line(format!("notes: no answer from the brain: {err}"));
                job.failed = Some(no_answer());
            }
        }
        job.card.clone().map(|card| (card, job.note.clone(), job.at, job.text.clone(), job.failed.clone()))
    };
    match shown {
        Some((card, note, at, text, failed)) => conclude(app, taker, &card, &note, at, &text, failed.as_deref()),
        None => prepared(app, taker, seq),
    }
}

/// An answer prepared ahead is ready: the key's button lights up until it is
/// shown, another question comes, or its time is up.
fn prepared(app: &AppHandle, taker: &NoteTaker, seq: u64) {
    // Not if the press came meanwhile and showed it; lit under the lock a
    // press takes to show it, so the two never cross.
    let left = {
        let slot = taker.desk.job.lock().unwrap();
        let Some(job) = slot.as_ref().filter(|j| j.seq == seq && j.card.is_none() && j.failed.is_none()) else { return };
        set_ready(app, true);
        PREPARED_FOR.saturating_sub(job.started.elapsed())
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(left).await;
        let Some(taker) = super::get(&app) else { return };
        let still = taker.desk.job.lock().unwrap().as_ref().is_some_and(|j| j.seq == seq && j.card.is_none());
        if still {
            set_ready(&app, false);
        }
    });
}

/// The press, for an answer prepared ahead to the question just asked: shown
/// at once, or as it comes if it is still being written.
fn reveal(app: &AppHandle, taker: &NoteTaker, note: &str, question: Option<&str>, card: &str) -> bool {
    let (seq, at, text, done) = {
        let mut slot = taker.desk.job.lock().unwrap();
        let Some(job) = slot.as_mut() else { return false };
        let fits = job.note == note
            && job.card.is_none()
            && job.failed.is_none()
            && job.line.is_some()
            && job.line.as_deref() == question
            && job.started.elapsed() < PREPARED_FOR;
        if !fits {
            return false;
        }
        job.card = Some(card.to_string());
        crate::log::line(format!(
            "notes: answer shown at once, {} ({} s after the question)",
            if job.done { "prepared" } else { "being prepared" },
            job.started.elapsed().as_secs()
        ));
        (job.seq, job.at, job.text.clone(), job.done)
    };
    set_ready(app, false);
    if done {
        conclude(app, taker, card, note, at, &text, None);
    } else if !text.is_empty() {
        progress(app, taker, seq, &text);
    }
    true
}

// ── From the Notetaker ────────────────────────────────────────────────────────

/// The key, or the strip's button.
pub async fn press(app: &AppHandle) {
    let pressed = Instant::now();
    let Some(taker) = super::get(app) else { return };
    let Some(_one) = Pressing::begin() else { return };
    let id = taker.recording.lock().unwrap().as_ref().map(|r| r.id.clone());
    let Some(id) = id else {
        // No call noted: nothing is recorded before the user says so.
        show_card(
            app,
            &notify::next_id("answer"),
            t("Take notes of this call?"),
            t("Ciel records both sides of the call, and suggests an answer when you press the key again. Let the others know it is recorded."),
            vec![Button { id: "notes".into(), label: t("Take notes") }],
            Target::Notetaker,
            true,
        );
        return;
    };
    // One answer at a time on the card.
    if taker.desk.job.lock().unwrap().as_ref().is_some_and(|j| j.card.is_some() && !j.done) {
        return;
    }
    // Something to see at once. It takes the last answer's place, so Ciel is
    // left out of screen shares until the next. Dismissed, it is an answer's
    // card already: what is written for it stops (`dismissed`).
    crate::capture::answer_gone(app);
    let card = notify::next_id("answer");
    show_card(app, &card, thinking(), String::new(), Vec::new(), Target::Answer(String::new()), true);
    // Whose brain, looked at while the last words are transcribed.
    let wanted = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || crate::agent::wanted(&app))
    };
    // The words just said, in order with the live transcript.
    let (flush_app, flushing) = (app.clone(), taker.clone());
    tauri::async_runtime::spawn_blocking(move || super::flush_live(&flush_app, &flushing)).await.unwrap_or_default();
    let words = pressed.elapsed();
    let agent = wanted.await.unwrap_or(false);
    if taker.desk.gone.lock().unwrap().as_deref() == Some(card.as_str()) {
        return;
    }
    let Some(note) = taker.note(&id) else {
        say(app, &card, no_answer());
        return;
    };
    let now_s = super::now_ms().saturating_sub(note.start) as f64 / 1000.0;
    let question = latest_question(&note.lines, now_s).cloned();
    if reveal(app, &taker, &id, question.as_ref().map(|l| l.id.as_str()), &card) {
        return;
    }
    if !note.lines.iter().any(|l| l.speaker != builder::ME && l.end >= now_s - LOCAL_WINDOW_S) {
        crate::log::line("notes: the answer key heard no one else in the last minutes");
        say(app, &card, t("Ciel hasn't heard the others ask anything in the last minutes."));
        return;
    }
    // The brain finds the question itself: the last line that looks like one may not be it.
    begin(app, &taker, note, None, Some(card), Some(agent), Timing { from: pressed, words: Some(words), kind: "press" });
}

/// The live transcript heard the others: a question is answered ahead, when
/// the user wants it.
pub fn heard(app: &AppHandle, taker: &Arc<NoteTaker>, note: &str, line: &Line) {
    if line.speaker == builder::ME || !is_question(&line.text) {
        return;
    }
    let on = app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().unwrap().prepare_answers);
    if !on {
        return;
    }
    // No brain to ask: only a build with the user's own Claude may still find one.
    if ai::current(app).is_none() && !cfg!(feature = "claude-code") {
        return;
    }
    // The user is reading one being written.
    if taker.desk.job.lock().unwrap().as_ref().is_some_and(|j| j.card.is_some() && !j.done) {
        return;
    }
    if taker.desk.prepared.fetch_add(1, Ordering::Relaxed) >= MAX_PREPARED {
        return;
    }
    let Some(note) = taker.note(note) else { return };
    begin(app, taker, note, Some(line.clone()), None, None, Timing { from: Instant::now(), words: None, kind: "prepared" });
}

/// An answer's card left the island (notify.rs): what was being written for
/// it stops.
pub fn dismissed(app: &AppHandle, card: &str) {
    let Some(taker) = super::get(app) else { return };
    *taker.desk.gone.lock().unwrap() = Some(card.to_string());
    let mut slot = taker.desk.job.lock().unwrap();
    if slot.as_ref().is_some_and(|j| j.card.as_deref() == Some(card) && !j.done) {
        *slot = None;
    }
}

/// A call starts to be noted: the company folder is read meanwhile.
pub fn started(app: &AppHandle, taker: &Arc<NoteTaker>) {
    taker.desk.prepared.store(0, Ordering::Relaxed);
    let dir = app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().company_folder.trim().to_string()).unwrap_or_default();
    if dir.is_empty() {
        return;
    }
    let taker = taker.clone();
    std::thread::spawn(move || {
        let began = Instant::now();
        let words: [String; 0] = [];
        folder_passages(&taker.desk, &dir, &words);
        let files = taker.desk.index.lock().unwrap().as_ref().map_or(0, |(i, _)| i.files());
        crate::log::line(format!("notes: company folder read, {files} files in {} ms", ms(began.elapsed())));
    });
}

/// The call is no longer noted: an answer prepared goes, and so does the folder.
pub fn stopped(app: &AppHandle, taker: &NoteTaker) {
    {
        let mut slot = taker.desk.job.lock().unwrap();
        // One the user is reading ends on its card.
        if slot.as_ref().is_some_and(|j| j.card.is_none() || j.done) {
            *slot = None;
        }
    }
    *taker.desk.index.lock().unwrap() = None;
    *taker.desk.gone.lock().unwrap() = None;
    set_ready(app, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(speaker: &str, start: f64, text: &str) -> Line {
        Line { id: format!("{speaker}{start}"), speaker: speaker.into(), start, end: start + 2.0, text: text.into() }
    }

    #[test]
    fn a_question_is_heard_by_its_mark_or_the_way_it_starts() {
        assert!(is_question("What's the price for ten seats?"));
        assert!(is_question("Et pour vingt postes ?"));
        assert!(is_question("¿Cuánto cuesta"));
        assert!(is_question("what about the second phase"));
        assert!(is_question("Could you send the deck by Friday"));
        assert!(is_question("Pouvez-vous nous envoyer le devis"));
        assert!(is_question("Est-ce que c'est possible"));
        assert!(!is_question("We sent the deck yesterday."));
        assert!(!is_question("I think that works for us"));
        assert!(!is_question("Can't wait to see it"));
        assert!(!is_question(""));
    }

    #[test]
    fn the_latest_question_is_one_of_the_others_last_two_lines() {
        let lines = vec![
            line("1", 100.0, "What's the price?"),
            line(builder::ME, 103.0, "Ninety euros."),
            line("1", 106.0, "OK."),
            line("1", 109.0, "And the delivery?"),
        ];
        assert_eq!(latest_question(&lines, 115.0).map(|l| l.text.as_str()), Some("And the delivery?"));
        let later = [lines.clone(), vec![line("1", 112.0, "Fine."), line("1", 115.0, "Thanks.")]].concat();
        assert_eq!(latest_question(&later, 120.0), None, "a question three lines back is not the one being asked");
        assert_eq!(latest_question(&lines, 1000.0), None, "nor one from long ago");
    }

    #[test]
    fn the_call_goes_in_two_minute_blocks_the_last_one_apart() {
        let lines = vec![line("1", 10.0, "Hello"), line(builder::ME, 50.0, "Hi"), line("1", 130.0, "Agenda?"), line("1", 250.0, "Price?")];
        let (done, now) = transcript_blocks(&lines, 260.0);
        assert_eq!(done, vec!["Them: Hello\nMe: Hi".to_string(), "Them: Agenda?".to_string()]);
        assert_eq!(now, "Them: Price?");
        // An hour in, only the last half hour.
        let (done, _) = transcript_blocks(&lines, 3600.0);
        assert!(done.is_empty());
        assert_eq!(recent_transcript(&lines, 260.0), "Them: Agenda?\nThem: Price?");
    }

    #[test]
    fn claude_gets_the_call_cached_up_to_the_block_going_on_without_thinking() {
        let p = Prompt { meeting: "The meeting: Q4".into(), blocks: vec!["Them: a".into(), "Them: b".into()], tail: "Then:\nThem: c".into() };
        let body = p.claude_body();
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 4);
        assert!(content[1]["text"].as_str().unwrap().starts_with("Transcript of the call:"));
        assert!(content[1].get("cache_control").is_none());
        assert_eq!(content[2]["cache_control"]["type"], "ephemeral");
        assert!(content[3].get("cache_control").is_none());
        assert_eq!(body["model"], ANSWER_MODEL);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert!(body.get("fallbacks").is_none() && body.get("temperature").is_none());
        // A call just begun: nothing finished to cache.
        let p = Prompt { meeting: String::new(), blocks: Vec::new(), tail: "x".into() };
        let body = p.claude_body();
        assert_eq!(body["messages"][0]["content"].as_array().unwrap().len(), 1);
        assert!(body["messages"][0]["content"][0].get("cache_control").is_none());
        // A local model, or the user's own Claude: one text, the meeting first.
        assert_eq!(p.plain(), "x");
        let p = Prompt { meeting: "The meeting: Q4".into(), blocks: Vec::new(), tail: "x".into() };
        assert_eq!(p.plain(), "The meeting: Q4\n\nx");
    }

    #[test]
    fn the_reply_is_read_as_it_comes_and_whole() {
        assert_eq!(
            parse_answer("Q: Price for ten seats\nA: It's 90 euros a seat, so 900 a month."),
            ("Price for ten seats".to_string(), "It's 90 euros a seat, so 900 a month.".to_string())
        );
        assert_eq!(parse_answer("Just this."), (String::new(), "Just this.".to_string()));
        // Labels in bold: their asterisks stay out of the card.
        assert_eq!(
            parse_answer("**Q:** Price for ten seats?\n\n**A:** It's 900 euros a month."),
            ("Price for ten seats?".to_string(), "It's 900 euros a month.".to_string())
        );
        // The second sentence keeps its line.
        assert_eq!(parse_answer("Q: Price\nA: It's 90.\nCheck the discount.").1, "It's 90.\nCheck the discount.");
        // The answer's label starts a line: "SLA:" in the question isn't it.
        assert_eq!(parse_answer("Q: Is the SLA: 99.9%?\nA: Yes, 99.9%."), ("Is the SLA: 99.9%?".to_string(), "Yes, 99.9%.".to_string()));
        assert_eq!(parse_partial("Q: Is the SLA: 99"), ("Is the SLA: 99".to_string(), String::new()));
        assert_eq!(parse_answer("Q: Price A: 90 euros."), ("Price".to_string(), "90 euros.".to_string()), "on one line all the same");

        assert_eq!(parse_partial(""), (String::new(), String::new()));
        assert_eq!(parse_partial("Q"), (String::new(), String::new()), "half a label is nothing yet");
        assert_eq!(parse_partial("**"), (String::new(), String::new()));
        assert_eq!(parse_partial("Q: Price for te"), ("Price for te".to_string(), String::new()));
        assert_eq!(parse_partial("**Q:** Price\n\n**A"), ("Price".to_string(), String::new()));
        assert_eq!(parse_partial("Q: Price\nA: It's 90"), ("Price".to_string(), "It's 90".to_string()));
        assert_eq!(parse_partial("It's 90"), (String::new(), "It's 90".to_string()));
    }

    #[test]
    fn the_card_says_ciel_is_thinking_then_the_question_then_the_answer() {
        assert_eq!(card_text("", false), ("Ciel is thinking…".to_string(), String::new()));
        assert_eq!(card_text("Q: Price", false), ("Price".to_string(), String::new()));
        assert_eq!(card_text("Q: Price\nA: It's 90", false), ("Price".to_string(), "It's 90".to_string()));
        assert_eq!(card_text("It's 90 euros.", true), ("Answer".to_string(), "It's 90 euros.".to_string()));
    }

    #[test]
    fn one_press_at_a_time_and_only_what_is_waited_for_stays() {
        // A press while Ciel looks into one changes nothing; the next one does.
        let first = Pressing::begin();
        assert!(first.is_some());
        assert!(Pressing::begin().is_none());
        drop(first);
        assert!(Pressing::begin().is_some());

        // A word folds as any notification does; an answer, the wait for it
        // and the question before recording stay three minutes.
        let word = key_card("answer-1".into(), "Answer".into(), "Ciel hasn't heard the others.".into(), Vec::new(), false, 1000);
        assert_eq!(word.until, None);
        let wait = key_card("answer-2".into(), "Ciel is thinking…".into(), String::new(), Vec::new(), true, 1000);
        assert_eq!(wait.until, Some((1000 + SHOWN_MS) as f64));
        // One tag: each takes the place of the last in the island.
        assert_eq!(word.tag, wait.tag);
    }
}
