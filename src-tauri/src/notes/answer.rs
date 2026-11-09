// The answer key (Ctrl+Alt+K, or the button on the Notetaker's strip): during
// a call the Notetaker records, Ciel finds the last question the others put to
// the user and suggests an answer on a card in the island, kept in the call's
// note. Pressed before a call is noted, it starts the Notetaker.
//
// Quick, because the slow parts are done before the press or while it shows:
// - the card shows at the press, and the answer is written into it as the
//   brain writes it, its first line a sentence to say as it is;
// - the words just said are transcribed at the press, in order with the live
//   transcript (the gate in mod.rs), so the question is never left out;
// - the company folder is read when the call starts (folder.rs); at a press
//   only what changed since is read again;
// - with Claude, the call goes in blocks of two minutes, and the API keeps the
//   finished ones cached from one question to the next;
// - with « Prepare answers ahead » on (off by default), a question the others
//   ask is answered at once, before the press: the key's button lights up, and
//   the press shows the answer.
// The log says how long each step took.

use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::{builder, folder, Answer, Line, Note, NoteTaker, Status};
use crate::ai::{self, Engine};
use crate::flow::pill;
use crate::i18n::t;
use crate::notify;

const ANSWER_MODEL: &str = "claude-haiku-5-5";
/// What a local model is given of the call: it reads a long prompt slowly.
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
/// The card stays this long, or until dismissed.
const SHOWN_MS: u64 = 3 * 60_000;
/// A prepared answer is offered this long after its question was heard.
const PREPARED_FOR: Duration = Duration::from_secs(120);
/// At most this many answers prepared in one call.
const MAX_PREPARED: u32 = 200;
/// The card is written into at most this often while the answer comes.
const CARD_INTERVAL: Duration = Duration::from_millis(1000 / 12);
/// A new answer's card takes the place of the last.
const TAG: &str = "meeting-answer";

const SYSTEM: &str = "You help the user during a live call, which they are recording with the other people's knowledge. \
In the transcript, \"Me\" is the user and \"Them\" the others. \
Answer the question named at the end; if none is named, the most recent question or request put to the user.\n\
Reply in exactly this form:\n\
Q: the question, in a few words\n\
A: one sentence the user can say as it is, in the first person, in the language of the call.\n\
Then, only if it helps, one or two short sentences more: a figure, a detail, or what to check.\n\
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

/// "Q: …" then "A: …" → (question, answer); without them, the whole reply is the answer.
pub fn parse_answer(reply: &str) -> (String, String) {
    let reply = reply.trim();
    let q = reply.find("Q:");
    let a = reply.find("A:");
    match (q, a) {
        (Some(q), Some(a)) if q < a => (reply[q + 2..a].trim().to_string(), reply[a + 2..].trim().to_string()),
        (None, Some(a)) => (String::new(), reply[a + 2..].trim().to_string()),
        _ => (String::new(), reply.to_string()),
    }
}

/// The same, while the reply is still being written: a question whose answer
/// hasn't come yet is only the question.
pub fn parse_partial(reply: &str) -> (String, String) {
    let reply = reply.trim_start();
    match reply.strip_prefix("Q:") {
        Some(question) if !reply.contains("A:") => (question.trim().to_string(), String::new()),
        _ => parse_answer(reply),
    }
}

// ── What the brain is given ───────────────────────────────────────────────────

/// One question to the brain: the call first (stable, cached by Claude), then
/// what changes with each question.
struct Prompt {
    /// The meeting, if the calendar named it.
    meeting: String,
    /// Claude's finished blocks of the call.
    blocks: Vec<String>,
    /// The block going on (Claude), or the last minutes (a local model), then
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

    fn local_user(&self) -> String {
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

async fn prompt(app: &AppHandle, taker: &Arc<NoteTaker>, engine: &Engine, note: &Note, question: Option<&Line>, now_s: f64) -> Prompt {
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
    // Earlier calls on the same subject: with Claude, only if the user lets the brain read the notes.
    let earlier = if matches!(engine, Engine::Local(_)) || reads_notes {
        let notes = taker.notes.lock().unwrap().clone();
        related_calls(&notes, &note.id, &words)
    } else {
        String::new()
    };
    let (blocks, now) = match engine {
        Engine::Claude => transcript_blocks(&note.lines, now_s),
        Engine::Local(_) => (Vec::new(), recent_transcript(&note.lines, now_s)),
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

// ── The card ──────────────────────────────────────────────────────────────────

fn show_card(app: &AppHandle, id: &str, title: String, body: String) {
    let now = super::now_ms();
    notify::show(
        app,
        notify::Notification {
            id: id.to_string(),
            source: "ciel".into(),
            app: "Ciel".into(),
            title,
            body,
            icon: None,
            at: now as f64,
            tag: Some(TAG.into()),
            buttons: Vec::new(),
            until: Some((now + SHOWN_MS) as f64),
            time: None,
        },
        notify::Target::None,
    );
}

/// What the card says of an answer so far.
fn card_text(text: &str, done: bool) -> (String, String) {
    let (question, answer) = if done { parse_answer(text) } else { parse_partial(text) };
    let title = if question.is_empty() { t("Answer") } else { question };
    let body = if answer.is_empty() { t("Thinking…") } else { answer };
    (title, body)
}

fn set_ready(app: &AppHandle, ready: bool) {
    let value = ready.then_some("ready");
    if pill::get(app).is_some_and(|p| p.state().note_answer == value) {
        return;
    }
    pill::update(app, |s| s.note_answer = value);
}

/// The job is done and shown: its last words on the card, and kept in the note.
fn conclude(app: &AppHandle, taker: &NoteTaker, card: &str, note: &str, at: f64, text: &str, failed: Option<&str>) {
    if let Some(why) = failed {
        notify::update(app, card, t("Answer"), why.to_string());
        return;
    }
    let (title, body) = card_text(text, true);
    notify::update(app, card, title, body);
    let (question, answer) = parse_answer(text);
    if !answer.is_empty() {
        taker.change(app, note, |n| n.answers.push(Answer { at, question, answer }));
    }
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

/// Starts writing an answer, in place of the one before. `card`: the press's
/// card, which shows it as it comes; none for an answer prepared ahead.
fn begin(app: &AppHandle, taker: &Arc<NoteTaker>, engine: Engine, note: Note, question: Option<Line>, card: Option<String>, timing: Timing) {
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
        task: None,
    });
    set_ready(app, false);
    let (app2, taker2) = (app.clone(), taker.clone());
    let task = tauri::async_runtime::spawn(async move {
        let (app, taker) = (app2, taker2);
        let asked = Instant::now();
        let prompt = prompt(&app, &taker, &engine, &note, question.as_ref(), now_s).await;
        let context = asked.elapsed();
        let mut first: Option<Duration> = None;
        let on_text = |text: &str| {
            first.get_or_insert_with(|| timing.from.elapsed());
            progress(&app, &taker, seq, text);
        };
        let (result, usage) = match &engine {
            Engine::Claude => match crate::claude::stream(&prompt.claude_body(), CLAUDE_TIMEOUT, on_text).await {
                Ok(s) if s.stop_reason.as_deref() == Some("refusal") => (Err(Failure::Declined), Some(s)),
                Ok(s) => (Ok(s.text.clone()), Some(s)),
                Err(err) => (Err(Failure::Error(err)), None),
            },
            Engine::Local(local) => {
                let how = ai::Ask { max_tokens: MAX_TOKENS, json: false, context: None, timeout: LOCAL_TIMEOUT };
                (ai::stream_complete(local, SYSTEM, &prompt.local_user(), how, on_text).await.map_err(Failure::Error), None)
            }
        };
        let words = timing.words.map(|d| format!("words {} ms, ", ms(d))).unwrap_or_default();
        let first = first.map(|d| format!("{} ms", ms(d))).unwrap_or_else(|| "none".into());
        let tokens = usage.map(|u| format!("; prompt {} tokens + {} from the cache", u.input_tokens, u.cache_read)).unwrap_or_default();
        crate::log::line(format!(
            "notes: answer ({}, {}): {words}context {} ms, first words {first}, done {} ms{tokens}",
            timing.kind,
            if matches!(engine, Engine::Claude) { "claude" } else { "local" },
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
    let mut slot = taker.desk.job.lock().unwrap();
    let Some(job) = slot.as_mut().filter(|j| j.seq == seq) else { return };
    job.text = text.to_string();
    let Some(card) = job.card.clone() else { return };
    if job.last_card.is_some_and(|at| at.elapsed() < CARD_INTERVAL) {
        return;
    }
    job.last_card = Some(Instant::now());
    let (title, body) = card_text(text, false);
    notify::update(app, &card, title, body);
}

/// Why there is no answer.
enum Failure {
    /// Claude's safeguards declined the question (`stop_reason: "refusal"`).
    Declined,
    Error(String),
}

fn finish(app: &AppHandle, taker: &NoteTaker, seq: u64, result: Result<String, Failure>) {
    let shown = {
        let mut slot = taker.desk.job.lock().unwrap();
        let Some(job) = slot.as_mut().filter(|j| j.seq == seq) else { return };
        job.done = true;
        job.task = None;
        match result {
            Ok(text) if !text.trim().is_empty() => job.text = text,
            Ok(_) => job.failed = Some(t("Ciel's brain didn't answer. Try the key again.")),
            Err(Failure::Declined) => {
                crate::log::line("notes: the brain declined the question");
                job.failed = Some(t("Ciel's brain declined this question."));
            }
            Err(Failure::Error(err)) => {
                crate::log::line(format!("notes: no answer from the brain: {err}"));
                job.failed = Some(t("Ciel's brain didn't answer. Try the key again."));
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
    let shown = {
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
        if job.done {
            Some((job.at, job.text.clone()))
        } else {
            let (title, body) = card_text(&job.text, false);
            notify::update(app, card, title, body);
            job.last_card = Some(Instant::now());
            None
        }
    };
    set_ready(app, false);
    if let Some((at, text)) = shown {
        conclude(app, taker, card, note, at, &text, None);
    }
    true
}

// ── From the Notetaker ────────────────────────────────────────────────────────

/// The key, or the strip's button.
pub async fn press(app: &AppHandle) {
    let pressed = Instant::now();
    let Some(taker) = super::get(app) else { return };
    let id = taker.recording.lock().unwrap().as_ref().map(|r| r.id.clone());
    let Some(id) = id else {
        match super::start(app) {
            Ok(_) => show_card(
                app,
                &notify::next_id("answer"),
                t("Ciel is listening"),
                t("The call is being noted. Press the key again when you're asked something."),
            ),
            Err(err) => super::say_why(app, err),
        }
        return;
    };
    let Some(engine) = ai::current(app) else {
        show_card(app, &notify::next_id("answer"), t("Answer"), t("Ciel needs Claude or a local model for this: set one up in Island → Chat."));
        return;
    };
    // One answer at a time on the card.
    if taker.desk.job.lock().unwrap().as_ref().is_some_and(|j| j.card.is_some() && !j.done) {
        return;
    }
    let card = notify::next_id("answer");
    show_card(app, &card, t("Answer"), t("Thinking…"));
    // The words just said, in order with the live transcript.
    let (flush_app, flushing) = (app.clone(), taker.clone());
    tauri::async_runtime::spawn_blocking(move || super::flush_live(&flush_app, &flushing)).await.unwrap_or_default();
    let words = pressed.elapsed();
    let Some(note) = taker.note(&id) else { return };
    let now_s = super::now_ms().saturating_sub(note.start) as f64 / 1000.0;
    let question = latest_question(&note.lines, now_s).cloned();
    if reveal(app, &taker, &id, question.as_ref().map(|l| l.id.as_str()), &card) {
        return;
    }
    if !note.lines.iter().any(|l| l.speaker != builder::ME && l.end >= now_s - LOCAL_WINDOW_S) {
        notify::update(app, &card, t("Answer"), t("Ciel hasn't heard the others ask anything in the last minutes."));
        return;
    }
    // The brain finds the question itself: the last line that looks like one may not be it.
    begin(app, &taker, engine, note, None, Some(card), Timing { from: pressed, words: Some(words), kind: "press" });
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
    let Some(engine) = ai::current(app) else { return };
    // The user is reading one being written.
    if taker.desk.job.lock().unwrap().as_ref().is_some_and(|j| j.card.is_some() && !j.done) {
        return;
    }
    if taker.desk.prepared.fetch_add(1, Ordering::Relaxed) >= MAX_PREPARED {
        return;
    }
    let Some(note) = taker.note(note) else { return };
    begin(app, taker, engine, note, Some(line.clone()), None, Timing { from: Instant::now(), words: None, kind: "prepared" });
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
    }

    #[test]
    fn the_reply_is_read_as_it_comes_and_whole() {
        assert_eq!(
            parse_answer("Q: Price for ten seats\nA: It's 90 euros a seat, so 900 a month."),
            ("Price for ten seats".to_string(), "It's 90 euros a seat, so 900 a month.".to_string())
        );
        assert_eq!(parse_answer("Just this."), (String::new(), "Just this.".to_string()));
        assert_eq!(parse_partial("Q: Price for te"), ("Price for te".to_string(), String::new()));
        assert_eq!(parse_partial("Q: Price\nA: It's 90"), ("Price".to_string(), "It's 90".to_string()));
        assert_eq!(parse_partial("It's 90"), (String::new(), "It's 90".to_string()));
        assert_eq!(card_text("Q: Price", false), ("Price".to_string(), t("Thinking…")));
        assert_eq!(card_text("", false), (t("Answer"), t("Thinking…")));
    }
}
