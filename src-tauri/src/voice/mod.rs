// « OK Ciel » on Windows and Linux: say it, then ask anything. Port of the
// Mac's Voice/ (VoiceEngine.swift and its friends).
//
// While it is on, the microphone is open and a cheap energy detector cuts what
// you say into stretches; only a stretch of speech goes to Parakeet, on this
// computer, and only a stretch that starts with « OK Ciel » goes further:
// what you said after it goes to Ciel's brain (brain.rs, ai.rs) — Claude with
// your own key, or the chat's local model — never the audio. Ciel shows it all
// in the pill at the bottom of the screen — listening, thinking, then the
// answer, said out loud by the system's voice (the pill's speechSynthesis),
// Kitten on this computer (kitten.rs) or ElevenLabs with your key.
//
// It stops listening while you dictate, while a call is recorded and while it
// speaks itself.

pub mod brain;
pub mod kitten;
pub mod wake;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::flow::pill;
use crate::speech::{self, audio};

/// What the pill shows of a conversation.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct VoiceView {
    /// "listening", "thinking", "speaking".
    pub phase: String,
    /// What you said (after « OK Ciel »).
    pub heard: String,
    /// Ciel's answer.
    pub answer: String,
}

struct Running {
    stop: Arc<AtomicBool>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

/// Rust waits on these for the island's answers and the end of speech.
fn pending() -> &'static Mutex<HashMap<u64, mpsc::Sender<String>>> {
    static P: OnceLock<Mutex<HashMap<u64, mpsc::Sender<String>>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Whether « OK Ciel » can run: the voice model, and a brain to answer — a
/// Claude key or a connected local model (ai.rs).
pub fn requirements_met(app: &AppHandle) -> bool {
    speech::is_ready() && (crate::ai::current(app).is_some() || crate::agent::wanted(app))
}

/// What the pill says when no answer came.
fn cannot_answer(app: &AppHandle) -> String {
    match crate::ai::current(app) {
        Some(crate::ai::Engine::Local(local)) => crate::i18n::tf(
            "Ciel couldn't answer: check that {server} is running with {model}.",
            &[("server", &crate::i18n::t(local.name)), ("model", &local.model)],
        ),
        _ => crate::i18n::t("Ciel couldn't answer: check your Claude key in Notch → Chat."),
    }
}

/// Starts or stops listening to follow the preference.
pub fn sync(app: &AppHandle) {
    let (wanted, output) = crate::flow::get(app)
        .map(|f| {
            let p = &f.store.lock().unwrap().prefs;
            (p.voice, p.voice_output.clone())
        })
        .unwrap_or_default();
    kitten::sync(wanted && output == "kitten");
    let mut running = RUNNING.lock().unwrap();
    match (wanted && requirements_met(app), running.is_some()) {
        (true, false) => {
            let stop = Arc::new(AtomicBool::new(false));
            let app2 = app.clone();
            let stop2 = stop.clone();
            std::thread::Builder::new().name("ciel-voice".into()).spawn(move || run(app2, stop2)).ok();
            *running = Some(Running { stop });
            speech::preload();
            crate::log::line("voice: « OK Ciel » listening");
        }
        (false, true) => {
            if let Some(r) = running.take() {
                r.stop.store(true, Ordering::Relaxed);
            }
            let dictation = crate::flow::get(app).is_some_and(|f| f.store.lock().unwrap().prefs.enabled);
            if !dictation && !crate::notes::is_recording(app) {
                speech::unload();
            }
            crate::log::line("voice: « OK Ciel » off");
        }
        _ => {}
    }
}

/// « OK Ciel » is listening (the voice model stays loaded).
pub fn is_listening() -> bool {
    RUNNING.lock().unwrap().is_some()
}

/// Starts again on another microphone.
pub fn restart(app: &AppHandle) {
    if let Some(r) = RUNNING.lock().unwrap().take() {
        r.stop.store(true, Ordering::Relaxed);
    }
    sync(app);
}

fn show(app: &AppHandle, view: Option<VoiceView>) {
    pill::update(app, |s| s.voice = view);
}

enum Mode {
    /// Waiting for « OK Ciel ».
    Wake,
    /// Heard « OK Ciel » alone: what comes next is the question.
    Command { text: String, until: Instant },
    /// Ciel asked something back: an answer comes without « OK Ciel ».
    FollowUp { until: Instant },
}

fn run(app: AppHandle, stop: Arc<AtomicBool>) {
    let mic = crate::flow::get(&app).map(|f| f.store.lock().unwrap().prefs.mic.clone()).unwrap_or_default();
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let capture = audio::Capture::start(audio::Source::Mic(if mic.is_empty() { None } else { Some(mic) }), move |s| {
        let _ = tx.send(s.to_vec());
    });
    let mut capture = match capture {
        Ok(c) => c,
        Err(err) => {
            crate::log::line(format!("voice: microphone: {err}"));
            *RUNNING.lock().unwrap() = None;
            return;
        }
    };
    let mut vad = wake::Vad::default();
    let mut mode = Mode::Wake;
    let mut brain = brain::Brain::default();
    let mut hide_at: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        let chunk = match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let now = Instant::now();
        // The pill's last answer goes away after a while.
        if hide_at.is_some_and(|t| now > t) {
            hide_at = None;
            show(&app, None);
        }
        match &mode {
            Mode::Command { until, .. } | Mode::FollowUp { until } if now > *until => {
                mode = Mode::Wake;
                vad.hangover = 25;
                show(&app, None);
            }
            _ => {}
        }
        // Dictation and calls have the microphone: Ciel doesn't listen meanwhile.
        if pill::dictating(&app) || crate::notes::is_recording(&app) {
            vad.reset();
            continue;
        }
        let Some(chunk) = chunk else { continue };
        for segment in vad.feed(&chunk) {
            if segment.len() < speech::SAMPLE_RATE as usize * 35 / 100 {
                continue;
            }
            let Some(heard) = speech::transcribe(&segment).map(|t| t.text.trim().to_string()) else { continue };
            if heard.is_empty() {
                continue;
            }
            let question = match &mut mode {
                Mode::Wake => match wake::after_wake(&heard) {
                    Some(rest) => {
                        crate::log::line("voice: « OK Ciel » heard");
                        // A local model loads, or Claude Code starts, while the question comes.
                        crate::ai::warm_up(&app);
                        crate::agent::warm_up(&app);
                        hide_at = None;
                        if crate::flow::text::word_count(&rest) >= 2 {
                            Some(rest)
                        } else {
                            show(&app, Some(VoiceView { phase: "listening".into(), heard: rest.clone(), answer: String::new() }));
                            mode = Mode::Command { text: rest, until: now + Duration::from_secs(7) };
                            // A question may come with pauses in it.
                            vad.hangover = 40;
                            None
                        }
                    }
                    None => None,
                },
                Mode::Command { text, .. } => {
                    let full = format!("{text} {heard}").trim().to_string();
                    Some(full)
                }
                Mode::FollowUp { .. } => Some(heard),
            };
            let Some(question) = question else { continue };
            mode = Mode::Wake;
            vad.hangover = 25;
            show(&app, Some(VoiceView { phase: "thinking".into(), heard: question.clone(), answer: String::new() }));
            *HEARD.lock().unwrap() = question.clone();
            let reply = brain.respond(&app, &question);
            let answer = reply.as_ref().map(|r| r.text.clone()).unwrap_or_default();
            if answer.is_empty() {
                let done = reply.is_some_and(|r| r.acted);
                show(&app, Some(VoiceView {
                    phase: "speaking".into(),
                    heard: question,
                    answer: if done { "✓".into() } else { cannot_answer(&app) },
                }));
                hide_at = Some(Instant::now() + Duration::from_secs(5));
                continue;
            }
            show(&app, Some(VoiceView { phase: "speaking".into(), heard: question.clone(), answer: answer.clone() }));
            speak(&app, &answer);
            vad.reset();
            // Drop what the microphone heard of Ciel's own voice.
            while rx.try_recv().is_ok() {}
            if answer.trim_end().ends_with('?') {
                mode = Mode::FollowUp { until: Instant::now() + Duration::from_secs(8) };
                vad.hangover = 40;
                show(&app, Some(VoiceView { phase: "listening".into(), heard: String::new(), answer }));
            } else {
                hide_at = Some(Instant::now() + Duration::from_secs(6));
            }
        }
    }
    capture.stop();
    show(&app, None);
}

/// The question being answered, for `waiting_for_ok`.
static HEARD: Mutex<String> = Mutex::new(String::new());

/// The brain on Claude Code asked for a click in the island (pipe.rs): the
/// pill says so under the question, until the answer comes.
pub fn waiting_for_ok(app: &AppHandle) {
    let heard = HEARD.lock().unwrap().clone();
    show(app, Some(VoiceView { phase: "thinking".into(), heard, answer: crate::i18n::t("Your OK is needed in the island.") }));
}

// ── Speaking ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Say {
    id: u64,
    text: String,
    lang: Option<&'static str>,
    /// ElevenLabs' MP3 or Kitten's WAV, base64.
    audio: Option<String>,
    /// The type of `audio`.
    mime: &'static str,
    gender: String,
}

/// Says `text` and returns once it is said (or after a sensible time).
fn speak(app: &AppHandle, text: &str) {
    let (output, gender) = crate::flow::get(app)
        .map(|f| {
            let p = &f.store.lock().unwrap().prefs;
            (p.voice_output.clone(), p.voice_gender.clone())
        })
        .unwrap_or_else(|| ("system".into(), "female".into()));
    if output == "off" {
        std::thread::sleep(Duration::from_millis(1500));
        return;
    }
    let (audio, mime) = match output.as_str() {
        "elevenlabs" => (elevenlabs(text, &gender).map(|mp3| base64(&mp3)), "audio/mpeg"),
        "kitten" => (kitten::say(text, &gender).map(|wav| base64(&wav)), "audio/wav"),
        _ => (None, "audio/mpeg"),
    };
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    pending().lock().unwrap().insert(id, tx);
    let _ = app.emit_to(pill::LABEL, "voice-say", Say { id, text: text.into(), lang: wake::speech_lang(text), audio, mime, gender });
    let words = crate::flow::text::word_count(text) as u64;
    let _ = rx.recv_timeout(Duration::from_millis(4000 + words * 600));
    pending().lock().unwrap().remove(&id);
}

/// The pill says it has finished speaking (`id`), or the island answers a question.
#[tauri::command]
pub fn voice_done(id: u64, text: Option<String>) {
    if let Some(tx) = pending().lock().unwrap().remove(&id) {
        let _ = tx.send(text.unwrap_or_default());
    }
}

/// OK Ciel → Kitten → Download.
#[tauri::command]
pub fn voice_kitten_download(app: AppHandle) {
    kitten::download(app);
}

/// OK Ciel → Kitten → Remove the voice.
#[tauri::command]
pub fn voice_kitten_remove(app: AppHandle) {
    kitten::remove(&app);
}

/// Where the webview has no voice (WebKitGTK often): speech-dispatcher or
/// espeak-ng, when installed.
#[tauri::command]
pub async fn voice_say_native(text: String, lang: Option<String>) -> bool {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "linux")]
        {
            let lang = lang.unwrap_or_default();
            let short = lang.split('-').next().unwrap_or("").to_string();
            if let Some(spd) = crate::platform::find_on_path("spd-say") {
                let mut cmd = std::process::Command::new(spd);
                cmd.arg("-w");
                if !short.is_empty() {
                    cmd.args(["-l", &short]);
                }
                return cmd.arg(&text).status().is_ok_and(|s| s.success());
            }
            if let Some(espeak) = crate::platform::find_on_path("espeak-ng") {
                let mut cmd = std::process::Command::new(espeak);
                if !short.is_empty() {
                    cmd.args(["-v", &short]);
                }
                return cmd.arg(&text).status().is_ok_and(|s| s.success());
            }
            false
        }
        #[cfg(windows)]
        {
            let _ = (text, lang);
            false
        }
    })
    .await
    .unwrap_or(false)
}

/// ElevenLabs' MP3 for `text`, with the user's key; None and the system voice speaks.
fn elevenlabs(text: &str, gender: &str) -> Option<Vec<u8>> {
    let key = crate::secrets::get("elevenlabs-api-key")?;
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().ok()?;
        let voices: Value = client.get("https://api.elevenlabs.io/v1/voices").header("xi-api-key", &key).send().await.ok()?.json().await.ok()?;
        let voice = pick_voice(voices["voices"].as_array()?, gender)?;
        let response = client
            .post(format!("https://api.elevenlabs.io/v1/text-to-speech/{voice}?output_format=mp3_44100_128"))
            .header("xi-api-key", &key)
            .header("accept", "audio/mpeg")
            .json(&json!({ "text": text, "model_id": "eleven_flash_v2_5" }))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            crate::log::line(format!("voice: ElevenLabs {}", response.status()));
            return None;
        }
        Some(response.bytes().await.ok()?.to_vec())
    })
}

/// First English voice of the gender (a young one first for a man); else any
/// voice of the gender; else the first one. Same choice as the Mac.
pub fn pick_voice(voices: &[Value], gender: &str) -> Option<String> {
    let label = |v: &Value, k: &str| v["labels"][k].as_str().unwrap_or("").to_lowercase();
    let english = |v: &Value| {
        let lang = label(v, "language");
        if !lang.is_empty() {
            return lang.starts_with("en");
        }
        let accent = label(v, "accent");
        accent.is_empty() || ["american", "british", "australian", "irish", "english", "us", "uk"].iter().any(|a| accent.contains(a))
    };
    let of_gender: Vec<&Value> = voices.iter().filter(|v| label(v, "gender") == gender).collect();
    let english_ones: Vec<&&Value> = of_gender.iter().filter(|v| english(v)).collect();
    let young = if gender == "male" { english_ones.iter().find(|v| label(v, "age").contains("young")).copied() } else { None };
    let v = young.or_else(|| english_ones.first().copied()).copied().or_else(|| of_gender.first().copied()).or_else(|| voices.first())?;
    v["voice_id"].as_str().map(String::from)
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

// ── The tools ─────────────────────────────────────────────────────────────────

/// Asks the island (it knows the pills and the services' data) and waits for its answer.
pub fn ask_island(app: &AppHandle, kind: &str, input: Value) -> Option<String> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    pending().lock().unwrap().insert(id, tx);
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "voice-ask", json!({ "id": id, "kind": kind, "input": input }));
    let answer = rx.recv_timeout(Duration::from_secs(5)).ok();
    pending().lock().unwrap().remove(&id);
    answer
}

/// Runs one of the tools, for Claude or a local model alike: what goes back to
/// the model, and whether something changed. `cloud`: the brain is Claude,
/// and reads only what the user let it (`may_read`), whatever it asks for.
pub fn run_tool(app: &AppHandle, name: &str, input: &Value, cloud: bool) -> (String, bool) {
    let allowed = |what: &str| current_settings(app).is_some_and(|s| may_read(&s, cloud, what));
    const NOT_ALLOWED: &str = "The user hasn't let this brain read that (Settings → Ciel's brain).";
    match name {
        "pills" => {
            let answer = ask_island(app, "pills", input.clone()).unwrap_or_else(|| "the island did not answer".into());
            let acted = input["action"].as_str() != Some("list") && !answer.contains("unknown");
            (answer, acted)
        }
        "service_info" => {
            let info = ask_island(app, "info", json!({})).unwrap_or_else(|| "no data".into());
            // The Calendar's card is there too: it goes with the calendar's own switch.
            (if allowed("calendar") { info } else { without_service(&info, "Calendar") }, false)
        }
        "music" => music(app, input),
        "finance" if !allowed("finance") => (NOT_ALLOWED.into(), false),
        "finance" => finance(app, input),
        "calendar" if !allowed("calendar") => (NOT_ALLOWED.into(), false),
        "calendar" => (crate::calendar::for_brain(input["day"].as_str()), false),
        "notes" if !allowed("notes") => (NOT_ALLOWED.into(), false),
        "notes" => {
            let limit = input["limit"].as_u64().unwrap_or(3) as usize;
            (crate::notes::for_brain(app, input["query"].as_str().unwrap_or(""), limit), false)
        }
        "open_app" => open_app(input["name"].as_str().unwrap_or("")),
        "prepare_email" => {
            let to = input["recipient"].as_str().unwrap_or("").trim().to_string();
            if !to.contains('@') {
                return ("No email address: ask the user to spell it.".into(), false);
            }
            let enc = |s: &str| -> String {
                s.bytes()
                    .map(|b| match b {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                        _ => format!("%{b:02X}"),
                    })
                    .collect()
            };
            let url = format!(
                "mailto:{}?subject={}&body={}",
                enc(&to).replace("%40", "@"),
                enc(input["subject"].as_str().unwrap_or("")),
                enc(input["body"].as_str().unwrap_or(""))
            );
            crate::platform::open_url(&url);
            (format!("The mail app opened with the email to {to}. Nothing is sent until the user sends it."), true)
        }
        _ => (format!("unknown tool {name}"), false),
    }
}

// ── What the brain may read ───────────────────────────────────────────────────

fn current_settings(app: &AppHandle) -> Option<crate::settings::Settings> {
    use tauri::Manager;
    app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().clone())
}

/// What « OK Ciel » may read: a pill's data once its pill is on; then with a
/// local model all of it, as it stays on this computer, and with Claude
/// (`cloud`) only what the user turned on in Settings → Ciel's brain, since
/// it then leaves the computer. Finance is off there until they do.
pub fn may_read(settings: &crate::settings::Settings, cloud: bool, what: &str) -> bool {
    let on = |id: &str| settings.main_pill == id || settings.active_integrations.iter().any(|x| x == id);
    let reads = &settings.brain_reads;
    match what {
        "finance" => on(crate::finance::PILL_ID) && (!cloud || reads.finance),
        "calendar" => on(crate::calendar::PILL_ID) && (!cloud || reads.calendar),
        "notes" => !cloud || reads.notes,
        _ => true,
    }
}

/// The island's data (`service_info`) without one service's card.
fn without_service(info: &str, service: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(info) else { return info.to_string() };
    if let Some(list) = v.get_mut("services").and_then(Value::as_array_mut) {
        list.retain(|s| s["service"] != service);
    }
    v.to_string()
}

// ── Finance (the Finance pill's folder) ──────────────────────────────────────

fn finance_folder(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let shared = app.try_state::<crate::Shared>()?;
    let settings = shared.settings.lock().unwrap().clone();
    Some(crate::finance::folder(&settings))
}

/// The category of the user's list closest to the one heard: the same name,
/// then one that contains the other, then "Other" (or the last one).
fn closest_category(heard: &str, categories: &[String]) -> Option<String> {
    let norm = |s: &str| s.trim().to_lowercase();
    let h = norm(heard);
    if let Some(c) = categories.iter().find(|c| norm(c) == h) {
        return Some(c.clone());
    }
    if !h.is_empty() {
        if let Some(c) = categories.iter().find(|c| {
            let c = norm(c);
            c.contains(&h) || h.contains(&c) || c.split(|ch: char| !ch.is_alphanumeric()).any(|w| w.len() > 2 && h.contains(w))
        }) {
            return Some(c.clone());
        }
    }
    categories.iter().find(|c| norm(c) == "other").or(categories.last()).cloned()
}

/// The month in a few lines the model can read back.
fn finance_text(s: &crate::finance::Summary) -> String {
    let cur = &s.base_currency;
    let mut out = format!("Month {}: spent {:.2} {cur} ({} entries), today {:.2} {cur}.", s.month, s.spent_month, s.month_count, s.spent_today);
    match (s.budget, s.remaining, s.percent_used) {
        (Some(b), Some(r), Some(p)) => out.push_str(&format!(" Budget {b:.2} {cur}: {p:.0}% used, {r:.2} {cur} left.")),
        _ => out.push_str(" No monthly budget set."),
    }
    if !s.top_categories.is_empty() {
        let top: Vec<String> = s.top_categories.iter().map(|c| format!("{} {:.2}", c.category, c.amount)).collect();
        out.push_str(&format!(" Top categories: {}.", top.join(", ")));
    }
    out.push_str(&format!(" Categories to use: {}. Currencies: {} (usual: {cur}).", s.categories.join(", "), s.currencies.join(", ")));
    if !s.unconverted.is_empty() {
        out.push_str(&format!(" Left out of the totals, no rate: {}.", s.unconverted.join(", ")));
    }
    out
}

fn finance(app: &AppHandle, input: &Value) -> (String, bool) {
    let Some(dir) = finance_folder(app) else { return ("The Finance pill isn't ready.".into(), false) };
    let summary = match crate::finance::summary(&dir) {
        Ok(s) => s,
        Err(e) => return (format!("Could not read the finance folder: {e}"), false),
    };
    if input["action"].as_str() != Some("add") {
        return (finance_text(&summary), false);
    }
    let amount = input["amount"].as_f64().or_else(|| input["amount"].as_str().and_then(|s| s.trim().replace(',', ".").parse().ok()));
    let Some(amount) = amount else { return ("No amount: ask the user how much it was.".into(), false) };
    let currency = input["currency"].as_str().map(|c| c.trim().to_uppercase()).filter(|c| !c.is_empty()).unwrap_or_else(|| summary.base_currency.clone());
    let Some(category) = closest_category(input["category"].as_str().unwrap_or(""), &summary.categories) else {
        return ("The finance folder has no categories.".into(), false);
    };
    let new = crate::finance::NewExpense {
        amount: (amount * 100.0).round() / 100.0,
        currency,
        category,
        note: input["note"].as_str().unwrap_or("").to_string(),
        date: input["date"].as_str().map(String::from),
    };
    match crate::finance::add_expense(&dir, &new) {
        Ok(e) => (format!("Added {:.2} {} to {} on {}.", e.amount, e.currency, e.category, e.date), true),
        Err(e) => (format!("Not added: {e}"), false),
    }
}

fn music(app: &AppHandle, input: &Value) -> (String, bool) {
    let action = input["action"].as_str().unwrap_or("");
    let (control, value) = match action {
        "play_pause" => ("playPause", None),
        "next" => ("next", None),
        "previous" => ("previous", None),
        "set_volume" => ("volume", Some(input["volume"].as_f64().unwrap_or(50.0).clamp(0.0, 100.0))),
        "search" => {
            let q = input["query"].as_str().unwrap_or("").trim();
            if q.is_empty() {
                return ("missing query".into(), false);
            }
            let encoded: String = q.chars().map(|c| if c.is_alphanumeric() { c.to_string() } else { "%20".into() }).collect();
            crate::platform::open_url(&format!("spotify:search:{encoded}"));
            return (format!("Spotify opened on a search for {q}: the user picks what to play."), true);
        }
        _ => return ("unknown action".into(), false),
    };
    let app2 = app.clone();
    let done = tauri::async_runtime::block_on(crate::spotify::spotify_control(app2, control.into(), value));
    if done {
        ("done".into(), true)
    } else {
        ("Spotify is not running or not playing: tell the user to open it.".into(), false)
    }
}

/// Opens an app whose name matches: a Start menu shortcut on Windows, a
/// .desktop entry on Linux. Nothing else is ever run.
fn open_app(name: &str) -> (String, bool) {
    let wanted = wake::normalize(name);
    if wanted.is_empty() {
        return ("no name".into(), false);
    }
    match find_app(&wanted) {
        Some((label, launch)) => {
            launch();
            (format!("{label} opened"), true)
        }
        None => (format!("No app named {name} on this computer."), false),
    }
}

#[cfg(windows)]
fn find_app(wanted: &str) -> Option<(String, Box<dyn FnOnce()>)> {
    let mut roots = Vec::new();
    if let Some(p) = std::env::var_os("APPDATA") {
        roots.push(std::path::PathBuf::from(p).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(p) = std::env::var_os("ProgramData") {
        roots.push(std::path::PathBuf::from(p).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    let mut best: Option<(usize, std::path::PathBuf)> = None;
    let mut stack = roots;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                let stem = wake::normalize(&path.file_stem().unwrap_or_default().to_string_lossy());
                let score = if stem == wanted { 0 } else if stem.starts_with(wanted) { 1 } else if stem.contains(wanted) { 2 } else { continue };
                if best.as_ref().is_none_or(|(s, p)| score < *s || (score == *s && path.as_os_str().len() < p.as_os_str().len())) {
                    best = Some((score, path));
                }
            }
        }
    }
    let (_, path) = best?;
    let label = path.file_stem()?.to_string_lossy().to_string();
    Some((label, Box::new(move || crate::platform::reveal_folder(&path.to_string_lossy()))))
}

#[cfg(target_os = "linux")]
fn find_app(wanted: &str) -> Option<(String, Box<dyn FnOnce()>)> {
    let mut dirs = vec![std::path::PathBuf::from("/usr/share/applications"), std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications")];
    dirs.push(crate::platform::home_dir().join(".local/share/applications"));
    dirs.push(crate::platform::home_dir().join(".local/share/flatpak/exports/share/applications"));
    let mut best: Option<(usize, String, String)> = None;
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            if text.lines().any(|l| l.trim() == "NoDisplay=true") {
                continue;
            }
            let Some(app_name) = text.lines().find_map(|l| l.strip_prefix("Name=")).map(str::trim) else { continue };
            let n = wake::normalize(app_name);
            let score = if n == wanted { 0 } else if n.starts_with(wanted) { 1 } else if n.contains(wanted) { 2 } else { continue };
            let id = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            if best.as_ref().is_none_or(|(s, _, _)| score < *s) {
                best = Some((score, app_name.to_string(), id));
            }
        }
    }
    let (_, label, id) = best?;
    Some((label, Box::new(move || {
        let _ = std::process::Command::new("gtk-launch").arg(&id).spawn();
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_reads_only_what_the_user_let_it_and_a_local_model_reads_it_all() {
        let mut s = crate::settings::Settings::default();
        // A pill's data needs its pill, whatever the brain.
        for cloud in [false, true] {
            assert!(!may_read(&s, cloud, "finance"));
            assert!(!may_read(&s, cloud, "calendar"));
        }
        s.active_integrations.push(crate::finance::PILL_ID.into());
        s.active_integrations.push(crate::calendar::PILL_ID.into());
        assert!(may_read(&s, false, "finance"), "a local model: the spending stays here");
        assert!(!may_read(&s, true, "finance"), "Claude: off until the user turns it on");
        assert!(may_read(&s, true, "calendar") && may_read(&s, true, "notes"));
        s.brain_reads = crate::settings::BrainReads { calendar: false, notes: false, finance: true };
        assert!(may_read(&s, true, "finance"));
        assert!(!may_read(&s, true, "calendar") && !may_read(&s, true, "notes"));
        assert!(may_read(&s, false, "calendar") && may_read(&s, false, "notes"));
        assert!(may_read(&s, true, "music"), "the other tools are not data");
    }

    #[test]
    fn a_service_card_can_be_left_out_of_the_islands_data() {
        let info = r#"{"pills":[],"services":[{"service":"Stripe","headline":"1"},{"service":"Calendar","headline":"20/10 14:00"}]}"#;
        let out: Value = serde_json::from_str(&without_service(info, "Calendar")).unwrap();
        assert_eq!(out["services"], json!([{"service":"Stripe","headline":"1"}]));
        assert_eq!(without_service("not json", "Calendar"), "not json");
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn the_voice_is_picked_like_on_the_mac() {
        let voices: Vec<Value> = serde_json::from_str(r#"[
            {"voice_id":"a","labels":{"gender":"male","accent":"american","age":"middle aged"}},
            {"voice_id":"b","labels":{"gender":"male","accent":"british","age":"young"}},
            {"voice_id":"c","labels":{"gender":"female","accent":"american"}},
            {"voice_id":"d","labels":{"gender":"female","language":"fr"}}
        ]"#).unwrap();
        assert_eq!(pick_voice(&voices, "male").as_deref(), Some("b"));
        assert_eq!(pick_voice(&voices, "female").as_deref(), Some("c"));
        assert_eq!(pick_voice(&[], "female"), None);
    }
}

/// With the models downloaded: `CIEL_TEST_WAKE_WAV=… cargo test -- --ignored wake_in`.
#[cfg(test)]
mod with_models {
    #[test]
    #[ignore]
    fn hears_the_wake_phrase_in_a_recording() {
        let path = std::env::var("CIEL_TEST_WAKE_WAV").expect("CIEL_TEST_WAKE_WAV");
        let bytes = std::fs::read(&path).unwrap();
        let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let pcm: Vec<f32> = bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
        let mut r = crate::speech::audio::Resampler::new(rate);
        let mut samples = Vec::new();
        r.process(&pcm, &mut samples);
        let mut vad = super::wake::Vad::default();
        let mut heard = Vec::new();
        for chunk in samples.chunks(1600) {
            for segment in vad.feed(chunk) {
                let text = crate::speech::transcribe(&segment).expect("models").text;
                println!("segment {:.2} s: {text}", segment.len() as f32 / 16_000.0);
                heard.push(super::wake::after_wake(&text));
            }
        }
        println!("{heard:?}");
        assert!(heard.iter().any(|h| h.as_deref().is_some_and(|q| !q.is_empty())));
    }
}

#[cfg(test)]
mod finance_tests {
    use super::*;

    fn cats() -> Vec<String> {
        ["Housing", "Groceries", "Eating out", "Transport", "Health & Gym", "Other"].iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_spoken_category_finds_the_users_own() {
        assert_eq!(closest_category("groceries", &cats()).as_deref(), Some("Groceries"));
        assert_eq!(closest_category("Gym", &cats()).as_deref(), Some("Health & Gym"));
        assert_eq!(closest_category("eating out with friends", &cats()).as_deref(), Some("Eating out"));
        // Nothing close: "Other", or the last category when there is no "Other".
        assert_eq!(closest_category("rocket fuel", &cats()).as_deref(), Some("Other"));
        let no_other: Vec<String> = vec!["Food".into(), "Fun".into()];
        assert_eq!(closest_category("", &no_other).as_deref(), Some("Fun"));
        assert_eq!(closest_category("x", &[]), None);
    }

    #[test]
    fn the_month_reads_back_in_the_base_currency() {
        let dir = std::env::temp_dir().join(format!("ciel-voice-finance-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let new = |amount: f64, category: &str| crate::finance::NewExpense {
            amount,
            currency: "USD".into(),
            category: category.into(),
            note: String::new(),
            date: None,
        };
        crate::finance::add_expense(&dir, &new(12.5, "Groceries")).unwrap();
        crate::finance::add_expense(&dir, &new(7.5, "Transport")).unwrap();
        let text = finance_text(&crate::finance::summary(&dir).unwrap());
        assert!(text.contains("spent 20.00 USD (2 entries)"), "{text}");
        assert!(text.contains("No monthly budget set."), "{text}");
        assert!(text.contains("Top categories: Groceries 12.50, Transport 7.50."), "{text}");
        assert!(text.contains("Categories to use: Housing"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
