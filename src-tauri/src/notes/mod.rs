// The Notetaker: records a call (your microphone, and what the computer plays
// for the other people), shows a live transcript, then writes the whole
// transcript person by person with Parakeet and the diarizer, and a summary
// with the to-dos. Port of NoteTaker.swift for Windows and Linux.
//
// Everything is stored in the app's folder (Notes/<id>/note.json); the audio is
// deleted once the transcript is written. No bot joins the call: the other
// people should be told it is recorded.

pub mod builder;
pub mod folder;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::flow::{pill, store::SummaryEngine};
use crate::i18n::{t, tf};
use crate::speech::{self, audio};
use builder::{Line, Speaker, TimedWord, Todo};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Recording,
    Processing,
    Ready,
    Failed,
}

/// The calendar's meeting a call was noted in (calendar/mod.rs): its title
/// names the note, and who was invited and its agenda help the summary.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub title: String,
    #[serde(default)]
    pub attendees: Vec<String>,
    #[serde(default)]
    pub agenda: Option<String>,
}

/// A call noted by the Notetaker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    /// Set once you rename the note: the summary then keeps your title.
    #[serde(default)]
    pub title_edited: bool,
    /// Milliseconds since 1970.
    pub start: u64,
    #[serde(default)]
    pub end: Option<u64>,
    /// The app in front when the recording started ("Teams", "Firefox").
    #[serde(default)]
    pub app: Option<String>,
    pub status: Status,
    #[serde(default)]
    pub speakers: Vec<Speaker>,
    #[serde(default)]
    pub lines: Vec<Line>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub todos: Vec<Todo>,
    #[serde(default)]
    pub error: Option<String>,
    /// The start written out in local time ("2026-10-14 22:31"), for the Markdown.
    #[serde(default)]
    pub start_local: String,
    /// Started from the calendar's card: the meeting.
    #[serde(default)]
    pub meeting: Option<Meeting>,
    /// What Ciel suggested during the call, on the answer key (`answer`).
    #[serde(default)]
    pub answers: Vec<Answer>,
}

/// A question put to the user during the call, and the answer Ciel suggested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    /// Seconds from the start of the recording.
    pub at: f64,
    pub question: String,
    pub answer: String,
}

/// One side of the call being written to disk, with what the live transcript
/// has not taken yet.
struct Channel {
    writer: Option<audio::WavWriter>,
    /// Samples since the last live chunk.
    live: Vec<f32>,
    /// Where `live` starts, in samples from the start of the recording.
    live_start: usize,
    total: usize,
}

impl Channel {
    fn new(path: &std::path::Path) -> std::io::Result<Channel> {
        Ok(Channel { writer: Some(audio::WavWriter::create(path)?), live: Vec::new(), live_start: 0, total: 0 })
    }

    fn push(&mut self, samples: &[f32]) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write(samples);
        }
        self.live.extend_from_slice(samples);
        self.total += samples.len();
    }

    /// A stretch ending on a pause (or a long one) for the live transcript.
    fn take_chunk(&mut self) -> Option<(f64, Vec<f32>)> {
        let rate = speech::SAMPLE_RATE as usize;
        if self.live.len() < rate * 3 / 2 {
            return None;
        }
        let tail = &self.live[self.live.len().saturating_sub(rate / 2)..];
        let quiet = tail.iter().fold(0.0f32, |m, &s| m.max(s.abs())) < 0.01;
        if !quiet && self.live.len() < rate * 12 {
            return None;
        }
        let start = self.live_start as f64 / rate as f64;
        let chunk = std::mem::take(&mut self.live);
        self.live_start = self.total;
        let peak = chunk.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        if peak < 0.01 {
            return None;
        }
        Some((start, chunk))
    }

    /// Whatever the live transcript hasn't taken, pause or not: the answer
    /// key wants the words just said, not those before the next pause.
    fn take_rest(&mut self) -> Option<(f64, Vec<f32>)> {
        let rate = speech::SAMPLE_RATE as usize;
        if self.live.len() < rate / 3 {
            return None;
        }
        let start = self.live_start as f64 / rate as f64;
        let chunk = std::mem::take(&mut self.live);
        self.live_start = self.total;
        (chunk.iter().fold(0.0f32, |m, &s| m.max(s.abs())) >= 0.01).then_some((start, chunk))
    }

    fn close(&mut self) {
        if let Some(w) = self.writer.take() {
            let _ = w.finish();
        }
    }
}

struct Recording {
    id: String,
    mic: audio::Capture,
    system: Option<audio::Capture>,
    mic_channel: Arc<Mutex<Channel>>,
    system_channel: Option<Arc<Mutex<Channel>>>,
    stop_live: Arc<AtomicBool>,
}

pub struct NoteTaker {
    notes: Mutex<Vec<Note>>,
    recording: Mutex<Option<Recording>>,
    /// Notes whose transcript or summary is being written.
    working: Mutex<Vec<String>>,
    starting: AtomicBool,
}

pub fn folder() -> PathBuf {
    crate::settings::local_dir().join("Notes")
}

fn note_dir(id: &str) -> PathBuf {
    folder().join(id)
}

fn get(app: &AppHandle) -> Option<Arc<NoteTaker>> {
    app.try_state::<Arc<NoteTaker>>().map(|s| s.inner().clone())
}

pub fn is_recording(app: &AppHandle) -> bool {
    get(app).is_some_and(|n| n.recording.lock().unwrap().is_some())
}

fn now_ms() -> u64 {
    crate::flow::store::now_ms()
}

fn local_stamp() -> String {
    let t = crate::platform::local_time();
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute)
}

fn person_name(n: usize) -> String {
    tf("Person {0}", &[("0", &n.to_string())])
}

impl NoteTaker {
    fn save(&self, note: &Note) {
        let dir = note_dir(&note.id);
        if crate::platform::ensure_private_dir(&dir).is_err() {
            return;
        }
        if let Ok(text) = serde_json::to_string_pretty(note) {
            let tmp = dir.join("note.json.tmp");
            if std::fs::write(&tmp, text).is_ok() {
                let _ = std::fs::rename(tmp, dir.join("note.json"));
            }
        }
    }

    fn note(&self, id: &str) -> Option<Note> {
        self.notes.lock().unwrap().iter().find(|n| n.id == id).cloned()
    }

    /// Writes a change onto the note as it is now (it may have been renamed meanwhile).
    fn change(&self, app: &AppHandle, id: &str, f: impl FnOnce(&mut Note)) -> Option<Note> {
        let updated = {
            let mut notes = self.notes.lock().unwrap();
            let note = notes.iter_mut().find(|n| n.id == id)?;
            f(note);
            note.clone()
        };
        self.save(&updated);
        let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &updated.id);
        Some(updated)
    }
}

pub fn setup(app: &AppHandle) {
    let mut notes: Vec<Note> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder()) {
        for entry in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path().join("note.json")) {
                if let Ok(note) = serde_json::from_str::<Note>(&text) {
                    notes.push(note);
                }
            }
        }
    }
    notes.sort_by(|a, b| b.start.cmp(&a.start));
    // A call left recording or being written when Ciel last quit: written now.
    let unfinished: Vec<String> = notes
        .iter()
        .filter(|n| matches!(n.status, Status::Recording | Status::Processing))
        .map(|n| n.id.clone())
        .collect();
    for n in notes.iter_mut().filter(|n| n.status == Status::Recording) {
        n.status = Status::Processing;
        n.end.get_or_insert(n.start);
    }
    let taker = Arc::new(NoteTaker {
        notes: Mutex::new(notes),
        recording: Mutex::new(None),
        working: Mutex::new(Vec::new()),
        starting: AtomicBool::new(false),
    });
    app.manage(taker.clone());
    for id in unfinished {
        let app = app.clone();
        std::thread::spawn(move || process(&app, &id));
    }
}

/// The pill's record button: starts or stops, and says what is missing.
pub fn toggle(app: &AppHandle) {
    if is_recording(app) {
        stop(app);
    } else if let Err(err) = start(app) {
        say_why(app, err);
    }
}

/// The calendar card's Take notes: the meeting is noted, unless a call
/// already is.
pub fn start_meeting(app: &AppHandle, meeting: Meeting) {
    if is_recording(app) {
        return;
    }
    if let Err(err) = start_with(app, Some(meeting)) {
        say_why(app, err);
    }
}

/// What kept the Notetaker from starting, on the pill.
fn say_why(app: &AppHandle, err: String) {
    let (text, action) = if err == "models" {
        (t("Download the voice model first, in Ciel → Settings → Recognition."), Some((t("Open Settings"), "openSettings")))
    } else {
        (err, None)
    };
    pill::announce(app, "error", "#FF6B6B", text, action, 8);
}

pub fn start(app: &AppHandle) -> Result<String, String> {
    start_with(app, None)
}

fn start_with(app: &AppHandle, meeting: Option<Meeting>) -> Result<String, String> {
    let taker = get(app).ok_or("not ready")?;
    if taker.recording.lock().unwrap().is_some() || taker.starting.swap(true, Ordering::Relaxed) {
        return Err("already recording".into());
    }
    let result = start_inner(app, &taker, meeting);
    taker.starting.store(false, Ordering::Relaxed);
    result
}

fn start_inner(app: &AppHandle, taker: &Arc<NoteTaker>, meeting: Option<Meeting>) -> Result<String, String> {
    if !speech::is_ready() {
        return Err("models".into());
    }
    let now = now_ms();
    let stamp = local_stamp();
    let id = crate::flow::text::new_id();
    // The calendar's title names the note, and the summary keeps it, as it
    // keeps a title you typed.
    let meeting_title = meeting.as_ref().map(|m| m.title.trim().to_string()).filter(|t| !t.is_empty());
    let note = Note {
        id: id.clone(),
        title: meeting_title.clone().unwrap_or_else(|| tf("Call · {0}", &[("0", stamp.get(5..).unwrap_or(&stamp))])),
        title_edited: meeting_title.is_some(),
        start: now,
        end: None,
        app: crate::flow::os::foreground_app().filter(|(id, _)| !crate::flow::is_shell(id)).map(|(_, name)| name),
        status: Status::Recording,
        speakers: Vec::new(),
        lines: Vec::new(),
        summary: None,
        key_points: Vec::new(),
        todos: Vec::new(),
        error: None,
        start_local: stamp,
        meeting,
        answers: Vec::new(),
    };
    let dir = note_dir(&id);
    crate::platform::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    let mic_channel = Arc::new(Mutex::new(Channel::new(&dir.join("mic.wav")).map_err(|e| e.to_string())?));
    let mic_pref = crate::flow::get(app).map(|f| f.store.lock().unwrap().prefs.mic.clone()).unwrap_or_default();
    let sink = mic_channel.clone();
    let mic = audio::Capture::start(audio::Source::Mic(if mic_pref.is_empty() { None } else { Some(mic_pref) }), move |s| {
        sink.lock().unwrap().push(s)
    })
    .map_err(|err| {
        crate::log::line(format!("notes: microphone: {err}"));
        let _ = std::fs::remove_dir_all(&dir);
        t("Ciel can't hear you: the microphone is not allowed or not available.")
    })?;
    // Without the computer's sound, the call is noted from the microphone alone.
    let (system, system_channel) = match Channel::new(&dir.join("system.wav")) {
        Ok(channel) => {
            let channel = Arc::new(Mutex::new(channel));
            let sink = channel.clone();
            match audio::Capture::start(audio::Source::System, move |s| sink.lock().unwrap().push(s)) {
                Ok(capture) => (Some(capture), Some(channel)),
                Err(err) => {
                    crate::log::line(format!("notes: system sound unavailable: {err}"));
                    let _ = std::fs::remove_file(dir.join("system.wav"));
                    (None, None)
                }
            }
        }
        Err(_) => (None, None),
    };
    let stop_live = Arc::new(AtomicBool::new(false));
    taker.notes.lock().unwrap().insert(0, note.clone());
    taker.save(&note);
    *taker.recording.lock().unwrap() = Some(Recording {
        id: id.clone(),
        mic,
        system,
        mic_channel: mic_channel.clone(),
        system_channel: system_channel.clone(),
        stop_live: stop_live.clone(),
    });
    speech::preload();
    spawn_live(app.clone(), taker.clone(), id.clone(), mic_channel, system_channel, stop_live);
    pill::update(app, |s| s.note_started_at = Some(note.start));
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &id);
    crate::log::line("notes: recording");
    Ok(id)
}

// ── The answer key ────────────────────────────────────────────────────────────

/// How far back in the call the question is looked for.
const ANSWER_WINDOW_S: f64 = 180.0;
/// The company folder's passages sent with the question, at most (characters).
const FOLDER_BUDGET: usize = 6000;
const ANSWER_MODEL: &str = "claude-haiku-4-5";
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);
const ANSWER_LOCAL_TIMEOUT: Duration = Duration::from_secs(90);
/// An answer's card stays this long, or until dismissed, and is in screen
/// shares meanwhile. So do the card that waits for it, until the answer or a
/// word takes its place, and the one that asks before recording.
const ANSWER_SHOWN_MS: u64 = 3 * 60_000;
/// What the brain is asked. The answer is read at a glance, while speaking:
/// its first sentence has to do alone.
const ANSWER_PROMPT: &str = "You help the user during a live call, which they are recording with the other people's knowledge. \
    From the transcript (\"Me\" is the user, \"Them\" the others), find the most recent question or request put to the user, \
    and suggest what the user can answer: plain, factual, in the first person, in the language of the call. \
    The user reads it at a glance while speaking. The first sentence is the answer itself, short enough to say as it is, \
    with the figure, the date or the yes or no that was asked for; then at most three sentences of detail, only what helps. \
    Use the company's files and earlier calls when they hold the answer, and never invent a figure, a date or a promise. \
    If the answer needs facts you don't have, say in a few words what to check and give a sensible answer meanwhile. \
    Reply in plain text, without Markdown, in exactly two parts: \"Q:\" and the question in a few words, then \"A:\" and the answer.";

/// One answer at a time: a press while Ciel looks for one changes nothing.
static ANSWERING: AtomicBool = AtomicBool::new(false);

/// Held while the answer key works.
struct Answering;

impl Answering {
    /// None while an earlier press is still being answered.
    fn begin() -> Option<Self> {
        (!ANSWERING.swap(true, Ordering::AcqRel)).then_some(Self)
    }
}

impl Drop for Answering {
    fn drop(&mut self) {
        ANSWERING.store(false, Ordering::Release);
    }
}

/// The last minutes of the call as the brain reads them: "Me:" and "Them:".
pub fn recent_transcript(lines: &[Line], now_s: f64) -> String {
    lines
        .iter()
        .filter(|l| l.end >= now_s - ANSWER_WINDOW_S)
        .map(|l| format!("{}: {}", if l.speaker == builder::ME { "Me" } else { "Them" }, l.text.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The earlier calls that share the most words with the question: their
/// summary and what is still to do, three at most.
pub fn related_calls(notes: &[Note], current: &str, words: &[String]) -> String {
    let mut scored: Vec<(usize, &Note)> = notes
        .iter()
        .filter(|n| n.id != current && n.status == Status::Ready)
        .map(|n| {
            let text = searchable(n);
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

/// "Q: …" then "A: …" → (question, answer); without them, the whole reply is
/// the answer. A brain that writes its labels in bold ("**Q:**") says the same.
pub fn parse_answer(reply: &str) -> (String, String) {
    let reply = reply.trim();
    // What is left of a label's bold around a part is not its text.
    let part = |s: &str| s.trim().trim_matches('*').trim().to_string();
    let q = reply.find("Q:");
    let a = reply.find("A:");
    match (q, a) {
        (Some(q), Some(a)) if q < a => (part(&reply[q + 2..a]), part(&reply[a + 2..])),
        (None, Some(a)) => (String::new(), part(&reply[a + 2..])),
        _ => (String::new(), reply.to_string()),
    }
}

/// A word from the answer key that isn't an answer: it takes an answer's
/// place in the island, and Ciel is left out of screen shares again, if so
/// set. It folds away as any notification does.
fn answer_card(app: &AppHandle, title: String, body: String) {
    crate::capture::answer_gone(app);
    card(app, title, body, Vec::new(), crate::notify::Target::None, false);
}

/// One of the answer key's cards, as the island gets it. They share a tag: a
/// new one takes the place of the last. `stays`: for ANSWER_SHOWN_MS, rather
/// than as long as any notification (Settings → Notifications).
fn answer_key_card(title: String, body: String, buttons: Vec<crate::notify::Button>, stays: bool, now: u64) -> crate::notify::Notification {
    crate::notify::Notification {
        id: crate::notify::next_id("answer"),
        source: "ciel".into(),
        app: "Ciel".into(),
        title,
        body,
        icon: None,
        at: now as f64,
        tag: Some("meeting-answer".into()),
        buttons,
        until: stays.then(|| (now + ANSWER_SHOWN_MS) as f64),
        time: None,
    }
}

fn card(app: &AppHandle, title: String, body: String, buttons: Vec<crate::notify::Button>, target: crate::notify::Target, stays: bool) {
    crate::notify::show(app, answer_key_card(title, body, buttons, stays, now_ms()), target);
}

/// The card's Take notes, when the answer key was pressed with no call noted.
pub fn start_for_answers(app: &AppHandle) {
    if is_recording(app) {
        return;
    }
    match start(app) {
        Ok(_) => answer_card(app, t("Ciel is listening"), t("The call is being noted. Press the key again when you're asked something.")),
        Err(err) => say_why(app, err),
    }
}

/// What the answer key found.
struct Suggested {
    /// The question in a few words; empty when the brain gave none.
    question: String,
    answer: String,
    /// The user's own Claude answered, rather than Ciel's brain.
    by_agent: bool,
    /// The others may see the answers (`answers_in_shares`).
    in_shares: bool,
}

/// The answer key, during a call the Notetaker records: Ciel finds the last
/// question the others put to the user in the live transcript, and suggests
/// an answer on a card in the island, kept in the note. A card says Ciel is
/// thinking meanwhile, and another press changes nothing until the answer,
/// or a word, took its place. While the answer is there, Ciel is in screen
/// shares (capture.rs), unless the user turned that off
/// (`answers_in_shares`). No call noted yet: a card asks first, and only its
/// Take notes starts the Notetaker.
pub async fn answer(app: &AppHandle) {
    let Some(taker) = get(app) else { return };
    let Some(_one) = Answering::begin() else { return };
    let began = std::time::Instant::now();
    // The call's two sides, taken with its id: the call may end meanwhile.
    let recording = taker.recording.lock().unwrap().as_ref().map(|rec| {
        let channels: Vec<(Arc<Mutex<Channel>>, &'static str)> =
            std::iter::once((rec.mic_channel.clone(), builder::ME)).chain(rec.system_channel.clone().map(|c| (c, "others"))).collect();
        (rec.id.clone(), channels)
    });
    let Some((id, channels)) = recording else {
        let take = crate::notify::Button { id: "notes".into(), label: t("Take notes") };
        card(
            app,
            t("Take notes of this call?"),
            t("Ciel records both sides of the call, and suggests an answer when you press the key again. Let the others know it is recorded."),
            vec![take],
            crate::notify::Target::Notetaker,
            true,
        );
        return;
    };
    // Something to see at once: the answer takes a few seconds. It takes the
    // last answer's place, so Ciel is left out of screen shares until the next.
    crate::capture::answer_gone(app);
    card(app, tf("{0} is thinking…", &[("0", "Ciel")]), String::new(), Vec::new(), crate::notify::Target::None, true);
    match suggest(app, &taker, &id, channels).await {
        Ok(found) => {
            let by = if found.by_agent { "the user's claude" } else { "Ciel's brain" };
            crate::log::line(format!("notes: answered in {:.1} s, through {by}", began.elapsed().as_secs_f64()));
            // In the share before it shows, so the others see it as the user does,
            // unless the user keeps the answers to themselves (Settings → General).
            if found.in_shares {
                crate::capture::answer_shown(app, ANSWER_SHOWN_MS);
            }
            let title = if found.question.is_empty() { t("Answer") } else { found.question };
            // Copy puts the answer on the clipboard, and its card stays (notify.rs).
            let copy = crate::notify::Button { id: "copy".into(), label: t("Copy") };
            card(app, title, found.answer.clone(), vec![copy], crate::notify::Target::Answer(found.answer), true);
        }
        Err(word) => answer_card(app, t("Answer"), word),
    }
}

/// The last question put to the user and an answer to suggest, kept in the
/// note; else the word to show in its place.
async fn suggest(app: &AppHandle, taker: &Arc<NoteTaker>, id: &str, channels: Vec<(Arc<Mutex<Channel>>, &'static str)>) -> Result<Suggested, String> {
    fn no_answer() -> String {
        t("Ciel's brain didn't answer. Try the key again.")
    }
    // The user's own Claude when they chose it for Ciel's brain (agent/, in a
    // build that has it), else Claude with its key or a local model. Looked
    // at while the last words are transcribed.
    let wanted = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || crate::agent::wanted(&app))
    };
    // The words just said, at once, rather than at the next pause.
    let heard = tauri::async_runtime::spawn_blocking(move || {
        channels
            .into_iter()
            .filter_map(|(channel, speaker)| {
                let (start, samples) = channel.lock().unwrap().take_rest()?;
                let text = speech::transcribe(&samples)?.text.trim().to_string();
                let end = start + samples.len() as f64 / speech::SAMPLE_RATE as f64;
                (!text.is_empty()).then(|| Line { id: crate::flow::text::new_id(), speaker: speaker.into(), start, end, text })
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    if !heard.is_empty() {
        taker.change(app, id, |n| {
            n.lines.extend(heard);
            n.lines.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
        });
    }
    let agent = wanted.await.unwrap_or(false);
    let engine = crate::ai::current(app);
    if engine.is_none() && !agent {
        crate::log::line("notes: the answer key has no brain (no Claude key, no local model, and no signed-in claude of the user's)");
        return Err(t("Ciel needs Claude or a local model for this: set one up in Island → Chat."));
    }
    let Some(note) = taker.note(id) else { return Err(no_answer()) };
    let now_s = now_ms().saturating_sub(note.start) as f64 / 1000.0;
    let transcript = recent_transcript(&note.lines, now_s);
    if !transcript.contains("Them:") {
        crate::log::line("notes: the answer key heard no one else in the last minutes");
        return Err(t("Ciel hasn't heard the others ask anything in the last minutes."));
    }
    let meeting = note.meeting.as_ref().map(|m| {
        let agenda = m.agenda.as_deref().map(|a| format!("\nAgenda: {a}")).unwrap_or_default();
        format!("The meeting: {}{agenda}\n", m.title)
    });
    // What to look for: the others' last words, and the meeting's title.
    let recent_them: String = note.lines.iter().rev().filter(|l| l.speaker != builder::ME).take(4).map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ");
    let question_words = folder::words(&format!("{recent_them} {}", note.meeting.as_ref().map(|m| m.title.as_str()).unwrap_or("")));
    let (company, reads_notes, in_shares) = {
        let shared = app.state::<crate::Shared>();
        let s = shared.settings.lock().unwrap();
        (s.company_folder.trim().to_string(), s.brain_reads.notes, s.answers_in_shares)
    };
    let files = if company.is_empty() {
        String::new()
    } else {
        let words = question_words.clone();
        let found = tauri::async_runtime::spawn_blocking(move || folder::search(std::path::Path::new(&company), &words, FOLDER_BUDGET))
            .await
            .unwrap_or_default();
        found.iter().map(|(name, p)| format!("[{name}]\n{p}")).collect::<Vec<_>>().join("\n\n")
    };
    // Earlier calls on the same subject: with Claude, only if the user lets the brain read the notes.
    let local_only = !agent && matches!(engine, Some(crate::ai::Engine::Local(_)));
    let earlier = if local_only || reads_notes {
        let notes = taker.notes.lock().unwrap().clone();
        related_calls(&notes, id, &question_words)
    } else {
        String::new()
    };
    let mut ask = meeting.unwrap_or_default();
    if !files.is_empty() {
        ask.push_str(&format!("From the company's files:\n{files}\n\n"));
    }
    if !earlier.is_empty() {
        ask.push_str(&format!("Earlier calls:\n{earlier}\n\n"));
    }
    ask.push_str(&format!("Transcript of the last minutes:\n{transcript}"));
    let from_agent = if agent {
        let ask = ask.clone();
        tauri::async_runtime::spawn_blocking(move || crate::agent::complete(ANSWER_PROMPT, &ask)).await.ok().flatten()
    } else {
        None
    };
    let by_agent = from_agent.is_some();
    let reply = match (from_agent, &engine) {
        (Some(text), _) => Ok(text),
        (None, Some(crate::ai::Engine::Claude)) => crate::claude::complete(ANSWER_MODEL, ANSWER_PROMPT, &ask, 400, ANSWER_TIMEOUT).await,
        (None, Some(crate::ai::Engine::Local(local))) => {
            let how = crate::ai::Ask { max_tokens: 400, json: false, context: None, timeout: ANSWER_LOCAL_TIMEOUT };
            crate::ai::complete(local, ANSWER_PROMPT, &ask, how).await
        }
        (None, None) => Err("the user's own Claude gave no answer".to_string()),
    };
    let reply = match reply {
        Ok(r) => crate::ai::visible_text(&r).trim().to_string(),
        Err(err) => {
            crate::log::line(format!("notes: no answer from the brain: {err}"));
            return Err(no_answer());
        }
    };
    let (question, answer) = parse_answer(&reply);
    if answer.is_empty() {
        crate::log::line("notes: the brain's answer was empty");
        return Err(no_answer());
    }
    taker.change(app, id, |n| n.answers.push(Answer { at: now_s, question: question.clone(), answer: answer.clone() }));
    Ok(Suggested { question, answer, by_agent, in_shares })
}

/// Every second while recording: the stretches ending on a pause go to
/// Parakeet for the live transcript (the final one is written again from the
/// whole recording).
fn spawn_live(
    app: AppHandle,
    taker: Arc<NoteTaker>,
    id: String,
    mic: Arc<Mutex<Channel>>,
    system: Option<Arc<Mutex<Channel>>>,
    stop: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(1));
            let mut channels = vec![(mic.clone(), builder::ME)];
            if let Some(s) = &system {
                channels.push((s.clone(), "others"));
            }
            for (channel, speaker) in channels {
                let chunk = channel.lock().unwrap().take_chunk();
                let Some((start, samples)) = chunk else { continue };
                let Some(transcript) = speech::transcribe(&samples) else { continue };
                let text = transcript.text.trim().to_string();
                if text.is_empty() || stop.load(Ordering::Relaxed) {
                    continue;
                }
                let end = start + samples.len() as f64 / speech::SAMPLE_RATE as f64;
                taker.change(&app, &id, |n| {
                    n.lines.push(Line { id: crate::flow::text::new_id(), speaker: speaker.into(), start, end, text });
                    n.lines.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
                });
            }
        }
    });
}

pub fn stop(app: &AppHandle) {
    let Some(taker) = get(app) else { return };
    let Some(mut rec) = taker.recording.lock().unwrap().take() else { return };
    crate::capture::answer_gone(app);
    rec.stop_live.store(true, Ordering::Relaxed);
    rec.mic.stop();
    if let Some(s) = rec.system.as_mut() {
        s.stop();
    }
    rec.mic_channel.lock().unwrap().close();
    if let Some(c) = &rec.system_channel {
        c.lock().unwrap().close();
    }
    taker.change(app, &rec.id, |n| {
        n.end = Some(now_ms());
        n.status = Status::Processing;
    });
    pill::update(app, |s| s.note_started_at = None);
    let app = app.clone();
    let id = rec.id.clone();
    std::thread::spawn(move || process(&app, &id));
}

/// Ciel quits during a call: the recordings are closed properly, and the
/// transcript is written at the next launch.
pub fn shutdown(app: &AppHandle) {
    let Some(taker) = get(app) else { return };
    let Some(mut rec) = taker.recording.lock().unwrap().take() else { return };
    crate::capture::answer_gone(app);
    rec.stop_live.store(true, Ordering::Relaxed);
    rec.mic.stop();
    if let Some(s) = rec.system.as_mut() {
        s.stop();
    }
    rec.mic_channel.lock().unwrap().close();
    if let Some(c) = &rec.system_channel {
        c.lock().unwrap().close();
    }
    taker.change(app, &rec.id, |n| {
        n.end = Some(now_ms());
        n.status = Status::Processing;
    });
}

fn words_in(path: &std::path::Path) -> Result<(Vec<TimedWord>, Vec<f32>), String> {
    if !path.exists() {
        return Ok((Vec::new(), Vec::new()));
    }
    let samples = audio::read_wav(path).map_err(|e| e.to_string())?;
    let transcript = speech::transcribe(&samples).ok_or_else(|| t("Download the voice model first, in Ciel → Settings → Recognition."))?;
    Ok((transcript.words, samples))
}

/// Writes the transcript from the recordings, then the summary, then deletes the audio.
fn process(app: &AppHandle, id: &str) {
    let Some(taker) = get(app) else { return };
    {
        let mut working = taker.working.lock().unwrap();
        if working.iter().any(|w| w == id) {
            return;
        }
        working.push(id.to_string());
    }
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
    let result = transcript(app, &taker, id);
    if let Err(err) = &result {
        crate::log::line(format!("notes: transcript failed: {err}"));
        taker.change(app, id, |n| {
            n.status = Status::Failed;
            n.error = Some(err.clone());
        });
    }
    taker.working.lock().unwrap().retain(|w| w != id);
    if result.is_ok() {
        let engine = crate::flow::get(app).map(|f| f.store.lock().unwrap().prefs.summary_engine).unwrap_or_default();
        if engine == SummaryEngine::Claude && crate::ai::current(app).is_some() {
            summarize(app, id);
        }
        let title = taker.note(id).map(|n| n.title).unwrap_or_default();
        pill::announce(
            app,
            "check",
            "#34D399",
            tf("Notes ready: {0}", &[("0", &title)]),
            Some((t("See"), "openNotes")),
            12,
        );
    }
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
}

fn transcript(app: &AppHandle, taker: &Arc<NoteTaker>, id: &str) -> Result<(), String> {
    let dir = note_dir(id);
    let mic_path = dir.join("mic.wav");
    let system_path = dir.join("system.wav");
    if !mic_path.exists() {
        return Err(t("The recording of this call is gone."));
    }
    let me = t("Me");
    let (mine, mic_samples) = words_in(&mic_path)?;
    let (theirs, system_samples) = words_in(&system_path)?;
    let built = if theirs.len() >= 5 {
        // A call: you on the microphone, the others on the computer's sound.
        let turns = speech::diarize(&system_samples);
        builder::build(&mine, &theirs, &turns, &me, person_name)
    } else if mine.len() >= 20 {
        // People around the computer: one recording, the voices told apart.
        let turns = speech::diarize(&mic_samples);
        let voices: std::collections::HashSet<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
        if voices.len() > 1 {
            builder::build_room(&mine, &turns, person_name)
        } else {
            builder::build(&mine, &[], &[], &me, person_name)
        }
    } else {
        builder::build(&mine, &theirs, &[], &me, person_name)
    };
    let (speakers, lines) = built;
    let count = lines.len();
    taker.change(app, id, |n| {
        n.speakers = speakers;
        n.lines = lines;
        n.status = Status::Ready;
        n.error = None;
    });
    let _ = std::fs::remove_file(&mic_path);
    let _ = std::fs::remove_file(&system_path);
    crate::log::line(format!("notes: transcript of {count} lines"));
    Ok(())
}

fn plain_transcript(note: &Note) -> String {
    note.lines
        .iter()
        .map(|l| {
            let name = note.speakers.iter().find(|s| s.id == l.speaker).map(|s| s.name.clone()).unwrap_or_else(|| l.speaker.clone());
            format!("{name}: {}", l.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A local model's context is small: a long call keeps its start and its end
/// (where the to-dos usually are), without the middle.
const LOCAL_MAX_TRANSCRIPT: usize = 48_000;

/// `text` cut to at most `max` bytes, keeping its start and its end.
fn without_the_middle(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut head = max * 2 / 5;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - (max - head);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}\n[…]\n{}", &text[..head], &text[tail..])
}

/// Writes (or writes again) the summary, key points and to-dos of a note,
/// with Ciel's brain: Claude, or the chat's local model.
fn summarize(app: &AppHandle, id: &str) {
    let Some(taker) = get(app) else { return };
    let Some(note) = taker.note(id) else { return };
    if note.lines.is_empty() {
        return;
    }
    let Some(engine) = crate::ai::current(app) else { return };
    taker.working.lock().unwrap().push(id.to_string());
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
    let mut transcript = plain_transcript(&note);
    let context = note.meeting.as_ref().map(builder::meeting_context).unwrap_or_default();
    let reply = match engine {
        crate::ai::Engine::Claude => {
            let model = app
                .try_state::<crate::Shared>()
                .map(|s| s.settings.lock().unwrap().model.clone())
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| crate::claude::DEFAULT_MODEL.to_string());
            if transcript.len() > 400_000 {
                let mut cut = 400_000;
                while !transcript.is_char_boundary(cut) {
                    cut -= 1;
                }
                transcript.truncate(cut);
            }
            let user = format!("{context}<transcript>\n{transcript}\n</transcript>");
            tauri::async_runtime::block_on(async {
                match crate::claude::complete(&model, builder::SUMMARY_INSTRUCTIONS, &user, 2048, Duration::from_secs(120)).await {
                    Ok(text) => Some(text),
                    Err(_) => crate::claude::complete(crate::flow::cleanup::MODEL, builder::SUMMARY_INSTRUCTIONS, &user, 2048, Duration::from_secs(120))
                        .await
                        .ok(),
                }
            })
        }
        crate::ai::Engine::Local(local) => {
            let user = format!("{context}<transcript>\n{}\n</transcript>", without_the_middle(&transcript, LOCAL_MAX_TRANSCRIPT));
            let how = crate::ai::Ask {
                max_tokens: 2048,
                json: true,
                context: crate::ai::context_for(builder::SUMMARY_INSTRUCTIONS.len() + user.len(), 2048),
                // A long call on a model that has to load first.
                timeout: Duration::from_secs(300),
            };
            let reply = tauri::async_runtime::block_on(crate::ai::complete(&local, builder::SUMMARY_INSTRUCTIONS, &user, how));
            reply.map_err(|err| crate::log::line(format!("notes: local summary failed: {err}"))).ok()
        }
    };
    taker.working.lock().unwrap().retain(|w| w != id);
    let Some(reply) = reply else {
        let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", id);
        return;
    };
    taker.change(app, id, |n| match builder::parse_summary(&reply) {
        Some(parsed) => {
            if let Some(title) = parsed.title.filter(|_| !n.title_edited) {
                n.title = title;
            }
            n.summary = Some(parsed.summary);
            n.key_points = parsed.key_points;
            n.todos = parsed.todos;
        }
        None => n.summary = Some(reply.trim().to_string()),
    });
}

/// The note as Markdown: title, date, summary, key points, to-dos, then the transcript.
pub fn markdown(note: &Note) -> String {
    let mut out = format!("# {}\n\n", note.title);
    let minutes = (((note.end.unwrap_or(note.start).saturating_sub(note.start)) as f64 / 60_000.0).round() as u64).max(1);
    out += &format!("_{} · {} min_", note.start_local, minutes);
    if !note.speakers.is_empty() {
        out += &format!("  \n_{}_", note.speakers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "));
    }
    out += "\n\n";
    if let Some(summary) = note.summary.as_ref().filter(|s| !s.is_empty()) {
        out += &format!("## {}\n\n{summary}\n\n", t("Summary"));
    }
    if !note.key_points.is_empty() {
        out += &format!("## {}\n\n", t("Key points"));
        out += &note.key_points.iter().map(|p| format!("- {p}")).collect::<Vec<_>>().join("\n");
        out += "\n\n";
    }
    if !note.todos.is_empty() {
        out += &format!("## {}\n\n", t("To do"));
        let lines: Vec<String> = note
            .todos
            .iter()
            .map(|todo| {
                let mut line = format!("- [{}] {}", if todo.done { "x" } else { " " }, todo.text);
                let extra: Vec<&str> = [todo.owner.as_deref(), todo.due.as_deref()].into_iter().flatten().filter(|s| !s.is_empty()).collect();
                if !extra.is_empty() {
                    line += &format!(" ({})", extra.join(", "));
                }
                line
            })
            .collect();
        out += &lines.join("\n");
        out += "\n\n";
    }
    if !note.lines.is_empty() {
        out += &format!("## {}\n\n", t("Transcript"));
        let lines: Vec<String> = note
            .lines
            .iter()
            .map(|l| {
                let name = note.speakers.iter().find(|s| s.id == l.speaker).map(|s| s.name.as_str()).unwrap_or(&l.speaker);
                format!("**{name}** ({}) : {}", builder::clock(l.start), l.text)
            })
            .collect();
        out += &lines.join("\n\n");
        out += "\n";
    }
    out
}

// ── For « OK Ciel » ───────────────────────────────────────────────────────────

/// The calls « OK Ciel » is told of (voice/mod.rs, once the user lets its
/// brain read them), newest first: the ones whose title, people, summary, key
/// points or to-dos hold every word of `query`, or the latest, at most
/// `limit` (1 to 5). Never the transcript. JSON text.
pub fn for_brain(app: &AppHandle, query: &str, limit: usize) -> String {
    let notes = get(app).map(|t| t.notes.lock().unwrap().clone()).unwrap_or_default();
    brain_text(&notes, query, limit)
}

fn brain_text(notes: &[Note], query: &str, limit: usize) -> String {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let found: Vec<Value> = notes
        .iter()
        .filter(|n| n.status == Status::Ready)
        .filter(|n| {
            let text = searchable(n);
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .take(limit.clamp(1, 5))
        .map(|n| {
            let minutes = (n.end.unwrap_or(n.start).saturating_sub(n.start) as f64 / 60_000.0).round() as u64;
            json!({
                "title": n.title,
                "date": n.start_local,
                "minutes": minutes.max(1),
                "people": n.speakers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
                "invited": n.meeting.as_ref().map(|m| m.attendees.clone()).unwrap_or_default(),
                "summary": n.summary,
                "keyPoints": n.key_points,
                "todos": n.todos.iter().map(|t| json!({ "text": t.text, "owner": t.owner, "due": t.due, "done": t.done })).collect::<Vec<_>>(),
            })
        })
        .collect();
    if found.is_empty() {
        return if words.is_empty() { "No call noted yet." } else { "No call matches." }.into();
    }
    json!({ "calls": found }).to_string()
}

/// What a search looks through: everything but the words said.
fn searchable(n: &Note) -> String {
    let mut parts: Vec<&str> = vec![n.title.as_str(), n.summary.as_deref().unwrap_or("")];
    parts.extend(n.speakers.iter().map(|s| s.name.as_str()));
    parts.extend(n.key_points.iter().map(String::as_str));
    for t in &n.todos {
        parts.push(&t.text);
        parts.extend(t.owner.as_deref());
    }
    if let Some(m) = &n.meeting {
        parts.extend(m.attendees.iter().map(String::as_str));
    }
    parts.join(" ").to_lowercase()
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesState {
    notes: Vec<Note>,
    recording: Option<String>,
    working: Vec<String>,
}

#[tauri::command]
pub fn notes_state(taker: State<Arc<NoteTaker>>) -> NotesState {
    NotesState {
        notes: taker.notes.lock().unwrap().clone(),
        recording: taker.recording.lock().unwrap().as_ref().map(|r| r.id.clone()),
        working: taker.working.lock().unwrap().clone(),
    }
}

#[tauri::command]
pub async fn notes_start(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        start(&app).map_err(|e| if e == "models" { t("Download the voice model first, in Ciel → Settings → Recognition.") } else { e })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn notes_answer(app: AppHandle) {
    answer(&app).await;
}

/// The company folder: is it there, and how many files Ciel can read in it.
#[tauri::command]
pub async fn notes_folder_info(path: String) -> folder::FolderInfo {
    tauri::async_runtime::spawn_blocking(move || folder::info(std::path::Path::new(path.trim()))).await.unwrap_or(folder::FolderInfo { exists: false, files: 0 })
}

#[tauri::command]
pub fn notes_stop(app: AppHandle) {
    stop(&app);
}

#[tauri::command]
pub fn notes_rename(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, title: String) {
    let title = title.trim().to_string();
    if title.is_empty() {
        return;
    }
    taker.change(&app, &id, |n| {
        n.title = title;
        n.title_edited = true;
    });
}

#[tauri::command]
pub fn notes_rename_speaker(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, speaker: String, name: String) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    taker.change(&app, &id, |n| {
        if let Some(s) = n.speakers.iter_mut().find(|s| s.id == speaker) {
            s.name = name;
        }
    });
}

#[tauri::command]
pub fn notes_toggle_todo(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String, todo: String) {
    taker.change(&app, &id, |n| {
        if let Some(t) = n.todos.iter_mut().find(|t| t.id == todo) {
            t.done = !t.done;
        }
    });
}

#[tauri::command]
pub fn notes_delete(app: AppHandle, taker: State<Arc<NoteTaker>>, id: String) {
    if taker.recording.lock().unwrap().as_ref().is_some_and(|r| r.id == id) {
        return;
    }
    taker.notes.lock().unwrap().retain(|n| n.id != id);
    let _ = std::fs::remove_dir_all(note_dir(&id));
    let _ = app.emit_to(crate::HUB_LABEL, "notes-changed", &id);
}

#[tauri::command]
pub fn notes_summarize(app: AppHandle, id: String) {
    std::thread::spawn(move || summarize(&app, &id));
}

#[tauri::command]
pub fn notes_retry(app: AppHandle, id: String) {
    std::thread::spawn(move || process(&app, &id));
}

#[tauri::command]
pub fn notes_markdown(taker: State<Arc<NoteTaker>>, id: String) -> Option<String> {
    taker.note(&id).map(|n| markdown(&n))
}

/// Saves the note as Markdown in Documents (or the home folder) and shows it.
#[tauri::command]
pub fn notes_export(taker: State<Arc<NoteTaker>>, id: String) -> Result<String, String> {
    let note = taker.note(&id).ok_or("no such note")?;
    let home = crate::platform::home_dir();
    let dir = [home.join("Documents"), home.clone()].into_iter().find(|d| d.is_dir()).unwrap_or(home);
    let safe: String = note
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut path = dir.join(format!("{}.md", if safe.is_empty() { "Ciel notes".to_string() } else { safe.clone() }));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{} ({n}).md", if safe.is_empty() { "Ciel notes" } else { &safe }));
        n += 1;
    }
    std::fs::write(&path, markdown(&note)).map_err(|e| e.to_string())?;
    crate::platform::reveal_folder(&dir.to_string_lossy());
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, status: Status, people: &[&str], summary: &str, todo: &str) -> Note {
        Note {
            id: title.into(),
            title: title.into(),
            title_edited: false,
            start: 0,
            end: Some(30 * 60_000),
            app: None,
            status,
            speakers: people.iter().enumerate().map(|(i, p)| Speaker { id: i.to_string(), name: p.to_string(), color: i, is_me: i == 0 }).collect(),
            lines: vec![Line { id: "l".into(), speaker: "0".into(), start: 0.0, end: 1.0, text: "the secret words said".into() }],
            summary: Some(summary.into()),
            key_points: vec!["Hire in Q1".into()],
            todos: vec![Todo { id: "t".into(), text: todo.into(), owner: Some("Paul".into()), due: None, done: false }],
            error: None,
            start_local: "2026-10-20 14:00".into(),
            meeting: None,
            answers: Vec::new(),
        }
    }

    #[test]
    fn one_answer_at_a_time_and_only_what_is_waited_for_stays() {
        // A press while Ciel looks for an answer changes nothing; the next one does.
        let first = Answering::begin();
        assert!(first.is_some());
        assert!(Answering::begin().is_none());
        drop(first);
        assert!(Answering::begin().is_some());

        // A word folds as any notification does; an answer, the wait for it
        // and the question before recording stay three minutes.
        let word = answer_key_card("Answer".into(), "Ciel hasn't heard the others.".into(), Vec::new(), false, 1000);
        assert_eq!(word.until, None);
        let answer = answer_key_card("Price".into(), "Ninety euros a seat.".into(), Vec::new(), true, 1000);
        assert_eq!(answer.until, Some((1000 + ANSWER_SHOWN_MS) as f64));
        // One tag: each takes the place of the last in the island.
        assert_eq!(word.tag, answer.tag);
        assert_ne!(word.id, answer.id);
    }

    #[test]
    fn the_answer_key_reads_the_last_minutes_and_a_two_part_reply() {
        let line = |speaker: &str, end: f64, text: &str| Line { id: "l".into(), speaker: speaker.into(), start: end - 2.0, end, text: text.into() };
        let lines = vec![line("1", 10.0, "Old talk"), line(builder::ME, 300.0, "Hello"), line("1", 305.0, "What's the price for ten seats?")];
        assert_eq!(recent_transcript(&lines, 310.0), "Me: Hello\nThem: What's the price for ten seats?");
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
    }

    #[test]
    fn ok_ciel_finds_calls_by_their_words_or_people_but_never_hears_the_transcript() {
        let notes = vec![
            note("Q4 budget", Status::Ready, &["Me", "Marie Dupont"], "We cut the tools budget.", "Send the figures"),
            note("Hiring", Status::Ready, &["Me", "Ana"], "Two hires.", "Post the job"),
            note("Still recording", Status::Recording, &["Me"], "", ""),
        ];
        let latest: Value = serde_json::from_str(&brain_text(&notes, "", 3)).unwrap();
        assert_eq!(latest["calls"].as_array().unwrap().len(), 2, "only calls whose notes are ready");
        let marie: Value = serde_json::from_str(&brain_text(&notes, "marie BUDGET", 3)).unwrap();
        assert_eq!(marie["calls"][0]["title"], "Q4 budget");
        assert_eq!(marie["calls"][0]["minutes"], 30);
        assert_eq!(marie["calls"][0]["todos"][0], json!({ "text": "Send the figures", "owner": "Paul", "due": null, "done": false }));
        assert!(!brain_text(&notes, "", 5).contains("secret words"));
        assert_eq!(brain_text(&notes, "nobody", 3), "No call matches.");
        assert_eq!(brain_text(&[], "", 3), "No call noted yet.");
        assert_eq!(serde_json::from_str::<Value>(&brain_text(&notes, "", 0)).unwrap()["calls"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_long_call_keeps_its_start_and_its_end_for_a_local_model() {
        assert_eq!(without_the_middle("short", 100), "short");
        let text = format!("{}{}{}", "début ".repeat(100), "milieu ".repeat(1000), "à faire : envoyer le devis");
        let cut = without_the_middle(&text, 1000);
        assert!(cut.len() <= 1000 + "\n[…]\n".len());
        assert!(cut.starts_with("début début"));
        assert!(cut.ends_with("à faire : envoyer le devis"));
        assert!(cut.contains("\n[…]\n"));
        // Never in the middle of a character.
        let accents = "é".repeat(5000);
        assert!(without_the_middle(&accents, 1001).chars().all(|c| c == 'é' || "\n[…]".contains(c)));
    }
}
