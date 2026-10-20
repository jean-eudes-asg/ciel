//! `ciel-hook` as the bridge of Ciel for Chrome (extension/): the browser
//! starts it through native messaging (`ciel-hook chrome-extension://<id>/`),
//! once per browser, and keeps it while the extension runs.
//!
//! From the browser (stdin): the notifications its pages show, each handed to
//! Ciel on a connection of its own, like a hook event. From Ciel: one
//! connection held open on its own thread, on which Ciel says it is there and
//! sends the island's clicks; they go to the browser on stdout. When Ciel is
//! closed, the extension is told so — the pages then show their notifications
//! as usual — and the bridge tries again every few seconds.
//!
//! Native messaging: each message is a 32-bit length in the machine's byte
//! order, then that much JSON.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::connect;

/// A browser message can be up to 64 MiB; a notification is far less. Beyond
/// this the stream is not one we understand.
const MAX_IN: usize = 1 << 20;
/// How often a closed Ciel is looked for again.
const RETRY: Duration = Duration::from_secs(3);

/// The browser started us: `chrome-extension://…` is among the arguments.
pub fn is_bridge(args: &[String]) -> bool {
    args.iter().any(|a| a.starts_with("chrome-extension://"))
}

/// One native message from `input`, or None at its end (or past what we take).
pub fn read_message(input: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    if len == 0 || len > MAX_IN {
        return None;
    }
    let mut buf = vec![0u8; len];
    input.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

/// One native message to `out`.
pub fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(message)?;
    out.write_all(&(bytes.len() as u32).to_ne_bytes())?;
    out.write_all(&bytes)?;
    out.flush()
}

/// What Ciel gets for a page's notification: its fields as the extension sent
/// them, and which bridge to answer through. None for anything else.
pub fn to_ciel(message: &Value, bridge: &str) -> Option<String> {
    let (kind, fields): (&str, &[&str]) = match message.get("type")?.as_str()? {
        "notification" => ("ciel_notification", &["key", "app", "title", "body", "icon", "tag"]),
        // YouTube Music's player, for its pill (src-tauri/src/ytmusic.rs).
        "media" => ("ciel_media", &["key", "gone", "title", "artist", "album", "art", "duration", "position", "playing"]),
        _ => return None,
    };
    let mut n = serde_json::Map::new();
    for key in fields {
        if let Some(v) = message.get(*key).filter(|v| v.is_string() || v.is_number() || v.is_boolean()) {
            n.insert((*key).into(), v.clone());
        }
    }
    Some(format!("{}\n", json!({kind: n, "ciel_bridge": bridge})))
}

pub fn run() -> ! {
    let bridge = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0));
    let out = Arc::new(Mutex::new(std::io::stdout()));

    // Ciel's side: held open, read line by line, told on to the browser.
    {
        let out = out.clone();
        let bridge = bridge.clone();
        std::thread::spawn(move || {
            let mut told: Option<bool> = None;
            let mut say = |on: bool| {
                if told != Some(on) {
                    told = Some(on);
                    let _ = write_message(&mut *out.lock().unwrap(), &json!({"type": "status", "ciel": on}));
                }
            };
            loop {
                if let Some(mut pipe) = connect() {
                    let hello = format!("{}\n", json!({"ciel_listen": "chrome", "ciel_bridge": bridge}));
                    if pipe.write_all(hello.as_bytes()).is_ok() && pipe.flush().is_ok() {
                        for line in BufReader::new(pipe).lines() {
                            let Ok(line) = line else { break };
                            let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
                            if message.get("type").and_then(Value::as_str) == Some("status") {
                                say(message.get("ciel").and_then(Value::as_bool).unwrap_or(false));
                            } else if write_message(&mut *out.lock().unwrap(), &message).is_err() {
                                // The browser is gone: so are we.
                                std::process::exit(0);
                            }
                        }
                    }
                }
                say(false);
                std::thread::sleep(RETRY);
            }
        });
    }

    // The browser's side: until it closes stdin, which is our end.
    let mut input = std::io::stdin().lock();
    while let Some(message) = read_message(&mut input) {
        if let Some(line) = to_ciel(&message, &bridge) {
            if let Some(mut pipe) = connect() {
                let _ = pipe.write_all(line.as_bytes());
                let _ = pipe.flush();
            }
        }
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_starts_the_bridge_with_the_extensions_origin() {
        assert!(is_bridge(&["chrome-extension://jfpabeoddliadjnngnhbelmmcogkgaje/".into(), "--parent-window=0".into()]));
        assert!(!is_bridge(&["--agent".into(), "codex".into()]));
    }

    #[test]
    fn native_messages_are_a_length_then_json() {
        let mut buf = Vec::new();
        write_message(&mut buf, &json!({"type": "status", "ciel": true})).unwrap();
        let len = u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize;
        assert_eq!(len, buf.len() - 4);
        let back = read_message(&mut &buf[..]).unwrap();
        assert_eq!(back, json!({"type": "status", "ciel": true}));
        // The end of the stream, or a length we don't take, ends the bridge.
        assert!(read_message(&mut &[][..]).is_none());
        let huge = ((MAX_IN + 1) as u32).to_ne_bytes();
        assert!(read_message(&mut &huge[..]).is_none());
    }

    #[test]
    fn a_pages_notification_goes_to_ciel_with_the_bridge_to_answer_through() {
        let m = json!({"type": "notification", "key": "4|a1", "app": "Gmail", "title": "Ana", "body": "Lunch?", "icon": null, "tag": "t", "extra": "dropped"});
        let line = to_ciel(&m, "b7").unwrap();
        assert!(line.ends_with('\n'));
        let v: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["ciel_bridge"], "b7");
        assert_eq!(v["ciel_notification"]["title"], "Ana");
        assert!(v["ciel_notification"].get("icon").is_none(), "only strings pass");
        assert!(v["ciel_notification"].get("extra").is_none());
        assert!(to_ciel(&json!({"type": "hello"}), "b7").is_none());
    }

    #[test]
    fn youtube_musics_player_goes_to_ciel_as_media() {
        let m = json!({"type": "media", "key": "12", "title": "Song", "artist": "Ana", "duration": 213.5,
            "position": 4.2, "playing": true, "art": null, "cookie": "dropped"});
        let v: Value = serde_json::from_str(to_ciel(&m, "b7").unwrap().trim()).unwrap();
        assert_eq!(v["ciel_bridge"], "b7");
        let media = &v["ciel_media"];
        assert_eq!((media["title"].as_str(), media["duration"].as_f64(), media["playing"].as_bool()), (Some("Song"), Some(213.5), Some(true)));
        assert!(media.get("art").is_none() && media.get("cookie").is_none());
        let gone: Value = serde_json::from_str(to_ciel(&json!({"type": "media", "key": "12", "gone": true}), "b7").unwrap().trim()).unwrap();
        assert_eq!(gone["ciel_media"]["gone"], true);
    }
}
