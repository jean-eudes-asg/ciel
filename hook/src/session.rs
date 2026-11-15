//! `ciel-hook --session <token>`: started by Ciel in a new terminal tab once
//! the user said yes to a coding session they asked for by voice
//! (src-tauri/src/agent/launch.rs). It asks Ciel over the pipe what to start —
//! the user's own `claude`, its arguments and the project's folder — and
//! starts exactly that, in this terminal.
//!
//! The token is all the terminal's command line carries: the user's words
//! never cross a shell or a terminal's own command line, where a `;` or a
//! quote would mean something. A token is good once, for a minute; when Ciel
//! isn't there or doesn't know it, nothing starts.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

/// How long Ciel has to say what to start.
const BUDGET: Duration = Duration::from_secs(5);
/// A line that says nothing started stays this long before the tab closes.
const READ: Duration = Duration::from_secs(4);
/// Set by whoever started Ciel, if that was a session itself: not this one's.
const NOT_OURS: [&str; 2] = ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"];

/// What Ciel said to start.
#[derive(Debug, PartialEq)]
struct Launch {
    exe: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
}

pub fn is_session(args: &[String]) -> bool {
    args.iter().any(|a| a == "--session")
}

/// The token after `--session`: 32 hex digits, as Ciel makes them.
fn token(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|a| a == "--session")?;
    let token = args.get(at + 1)?;
    (token.len() == 32 && token.bytes().all(|b| b.is_ascii_hexdigit())).then_some(token.as_str())
}

/// Ciel's answer: `{"exe": …, "args": […], "cwd": …}`, all of it or nothing.
fn parse(reply: &str) -> Option<Launch> {
    let v: Value = serde_json::from_str(reply.trim()).ok()?;
    let text = |key: &str| v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
    let args = v.get("args")?.as_array()?.iter().map(|a| a.as_str().map(String::from)).collect::<Option<Vec<_>>>()?;
    Some(Launch { exe: PathBuf::from(text("exe")?), args, cwd: PathBuf::from(text("cwd")?) })
}

pub fn run() -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch = token(&args)
        .and_then(|token| super::mcp::ask_ciel(&json!({ "ciel_session": token }).to_string(), BUDGET))
        .and_then(|reply| parse(&reply));
    let Some(launch) = launch else {
        nothing("Ciel has nothing to start here.");
    };
    let mut cmd = Command::new(&launch.exe);
    cmd.args(&launch.args).current_dir(&launch.cwd);
    for var in NOT_OURS {
        cmd.env_remove(var);
    }
    start(cmd)
}

fn nothing(why: &str) -> ! {
    eprintln!("{why}");
    std::thread::sleep(READ);
    std::process::exit(1);
}

/// The session takes this process's place: the terminal's keys and signals are its own.
#[cfg(unix)]
fn start(mut cmd: Command) -> ! {
    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    nothing(&format!("Ciel couldn't start the session: {err}"));
}

/// Windows has no exec: the session runs as a child, and this process waits
/// for it. Ctrl+C is the session's to hear; taken here, the tab would close
/// under it.
#[cfg(windows)]
fn start(mut cmd: Command) -> ! {
    use windows::core::BOOL;
    use windows::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT};

    unsafe extern "system" fn stay(kind: u32) -> BOOL {
        BOOL::from(kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT)
    }
    // SAFETY: `stay` is a plain function that lives as long as the process.
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(stay), true);
    }
    match cmd.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(0)),
        Err(err) => nothing(&format!("Ciel couldn't start the session: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_a_token_of_ciels_is_asked_about() {
        let good = "0123456789abcdef0123456789ABCDEF";
        assert!(is_session(&args(&["--session", good])));
        assert_eq!(token(&args(&["--session", good])), Some(good));
        // Anything else on the command line is never sent anywhere.
        for bad in [&["--session"][..], &["--session", "abc"], &["--session", "0123456789abcdef0123456789abcdeg"], &["--session", "; calc"], &["--mcp"]] {
            assert_eq!(token(&args(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn what_to_start_is_taken_whole_or_not_at_all() {
        let launch = parse(r#"{"exe":"C:\\Users\\me\\.local\\bin\\claude.exe","args":["--continue","fix the test; then stop"],"cwd":"D:\\work\\ciel"}"#).unwrap();
        assert_eq!(launch.exe, PathBuf::from(r"C:\Users\me\.local\bin\claude.exe"));
        // The task is one argument, whatever is in it.
        assert_eq!(launch.args, ["--continue", "fix the test; then stop"]);
        assert_eq!(launch.cwd, PathBuf::from(r"D:\work\ciel"));
        assert_eq!(parse(r#"{"exe":"/usr/bin/claude","args":[],"cwd":"/work"}"#).unwrap().args, Vec::<String>::new());
        for bad in ["", "no", "{}", r#"{"exe":"","args":[],"cwd":"/w"}"#, r#"{"exe":"/c","args":[1],"cwd":"/w"}"#, r#"{"exe":"/c","args":[]}"#, r#"{"exe":"/c","cwd":"/w"}"#] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }
}
