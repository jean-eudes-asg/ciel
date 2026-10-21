// The Claude Code of one conversation: found, checked, started with Ciel's
// command line (agent/mod.rs), given the user's turns on its stdin, and
// stopped once the conversation is over or Ciel quits. Built with the
// `claude-code` feature only.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::stream::{self, Event};
use super::Status;
use crate::voice::brain::Reply;

/// A conversation is over after this long without a word, as the usual brain's (brain.rs).
const IDLE: Duration = Duration::from_secs(90);
/// The longest a turn may take: Claude Code starting, its tools, and a click
/// in the island (which waits 110 s at most).
const TURN: Duration = Duration::from_secs(240);
/// For `claude --version` and `claude auth status`.
const CHECK: Duration = Duration::from_secs(15);
/// What was found of `claude` is looked at again after this.
const STATUS_KEPT: Duration = Duration::from_secs(60);

struct Session {
    child: Child,
    stdin: ChildStdin,
    events: mpsc::Receiver<Event>,
    last: Instant,
}

static SESSION: LazyLock<Mutex<Option<Session>>> = LazyLock::new(|| Mutex::new(None));
static STATUS: LazyLock<Mutex<Option<(Instant, Status)>>> = LazyLock::new(|| Mutex::new(None));
/// A thread is watching for the conversation to go quiet.
static WATCHING: AtomicBool = AtomicBool::new(false);

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    }
}

/// `claude` on the PATH, else where its installer puts it. Never a `.cmd`
/// shim, which can't be started without a shell.
fn find() -> Option<PathBuf> {
    let on_path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|d| d.join(exe_name())).collect()).unwrap_or_default();
    let installed = crate::platform::home_dir().join(".local").join("bin").join(exe_name());
    on_path.into_iter().chain(std::iter::once(installed)).find(|p| p.is_file())
}

fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    crate::platform::no_console(&mut cmd);
    cmd
}

/// `claude <args>`: whether it succeeded and what it printed, within CHECK.
fn run(path: &Path, args: &[&str]) -> Option<(bool, String)> {
    let child = command(path).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = rx.recv_timeout(CHECK).ok()?.ok()?;
    Some((out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string()))
}

/// Where `claude` is, its version, and whether the user is signed in, as
/// `claude auth status` says by its exit code. What it prints of the account
/// is not read.
pub fn status() -> Status {
    if let Some((at, s)) = STATUS.lock().unwrap().clone() {
        if at.elapsed() < STATUS_KEPT {
            return s;
        }
    }
    let mut s = Status { built: true, ..Status::default() };
    if let Some(path) = find() {
        s.path = Some(path.to_string_lossy().to_string());
        s.version = run(&path, &["--version"]).filter(|(ok, _)| *ok).and_then(|(_, out)| out.lines().next().map(String::from));
        s.signed_in = run(&path, &["auth", "status"]).is_some_and(|(ok, _)| ok);
    }
    *STATUS.lock().unwrap() = Some((Instant::now(), s.clone()));
    s
}

pub fn wanted(app: &AppHandle) -> bool {
    let on = crate::flow::get(app).is_some_and(|f| f.store.lock().unwrap().prefs.voice_agent);
    on && {
        let s = status();
        s.path.is_some() && s.signed_in
    }
}

/// A fresh token for the session's tools (agent/mod.rs `mcp_reply`).
fn new_token() -> Result<String, String> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut bytes = [0u8; 16];
    SystemRandom::new().fill(&mut bytes).map_err(|_| "no randomness".to_string())?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn start(app: &AppHandle) -> Result<Session, String> {
    let path = find().ok_or("claude not found")?;
    let dir = crate::settings::local_dir().join("Agent");
    crate::platform::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    let token = new_token()?;
    // The token in a file of Ciel's private folder rather than on the command line.
    let config = dir.join("mcp.json");
    let relay = crate::settings::hook_exe_path();
    std::fs::write(&config, super::mcp_config(&relay.to_string_lossy(), &token)).map_err(|e| e.to_string())?;
    let approvals = super::approval_settings(&crate::agents::relay_command(crate::agents::Shell::Sh, "--agent ciel PermissionRequest"));
    let prompt = crate::voice::brain::agent_prompt(app);
    let mut child = command(&path)
        .args(super::arguments(&config.to_string_lossy(), &approvals, &prompt, cfg!(windows)))
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            for event in stream::parse(&line) {
                if tx.send(event).is_err() {
                    return;
                }
            }
        }
    });
    super::set_token(Some(token));
    crate::log::line("agent: Claude Code started");
    Ok(Session { child, stdin, events: rx, last: Instant::now() })
}

fn end(mut session: Session) {
    super::set_token(None);
    let _ = session.child.kill();
    let _ = session.child.wait();
    crate::log::line("agent: Claude Code stopped");
}

pub fn stop() {
    let ended = SESSION.lock().unwrap().take();
    if let Some(session) = ended {
        end(session);
    }
}

/// Ends the conversation once it has gone quiet: no process outlives it.
fn watch() {
    if WATCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(5));
        let mut slot = SESSION.lock().unwrap();
        let over = match slot.as_ref() {
            None => true,
            Some(s) => s.last.elapsed() > IDLE,
        };
        if over {
            let ended = slot.take();
            drop(slot);
            if let Some(session) = ended {
                end(session);
            }
            WATCHING.store(false, Ordering::SeqCst);
            return;
        }
    });
}

/// A session to talk to: the running one, or a new one if it went quiet or ended.
fn ready<'a>(app: &AppHandle, slot: &'a mut Option<Session>) -> Option<&'a mut Session> {
    let gone = slot.as_mut().is_some_and(|s| s.last.elapsed() > IDLE || s.child.try_wait().ok().flatten().is_some());
    if gone {
        if let Some(old) = slot.take() {
            end(old);
        }
    }
    if slot.is_none() {
        match start(app) {
            Ok(session) => *slot = Some(session),
            Err(err) => {
                crate::log::line(format!("agent: Claude Code didn't start: {err}"));
                return None;
            }
        }
    }
    watch();
    slot.as_mut()
}

pub fn warm_up(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if !wanted(&app) {
            return;
        }
        let mut slot = SESSION.lock().unwrap();
        if let Some(session) = ready(&app, &mut slot) {
            session.last = Instant::now();
        }
    });
}

pub fn ask(app: &AppHandle, question: &str) -> Option<Reply> {
    let mut slot = SESSION.lock().unwrap();
    let session = ready(app, &mut slot)?;
    while session.events.try_recv().is_ok() {}
    let sent = writeln!(session.stdin, "{}", stream::user_line(question)).and_then(|_| session.stdin.flush());
    if sent.is_err() {
        crate::log::line("agent: Claude Code stopped listening");
        if let Some(old) = slot.take() {
            end(old);
        }
        return None;
    }
    let deadline = Instant::now() + TURN;
    let mut acted = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match session.events.recv_timeout(left) {
            Ok(Event::Ready) => {}
            Ok(Event::ToolUse(_)) => acted = true,
            Ok(Event::Done { text, failed: false }) => {
                session.last = Instant::now();
                return Some(Reply { text: crate::voice::brain::spoken_text(&text), acted });
            }
            Ok(Event::Done { failed: true, .. }) | Err(_) => {
                crate::log::line("agent: Claude Code gave no answer");
                if let Some(old) = slot.take() {
                    end(old);
                }
                return None;
            }
        }
    }
}
