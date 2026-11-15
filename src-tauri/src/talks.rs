// What was said (Ciel window → Chat → Your work → Remember what we say).
//
// Off until the user turns it on: « OK Ciel »'s conversations otherwise live
// in memory only and are forgotten 90 s after the last word (voice/brain.rs).
// On, each thing asked out loud and its answer are written to Work/talks.md
// in Ciel's own folder, a plain file the user can open, edit or delete, and
// the latest of them go to the brain at the start of a conversation: it
// follows on after the conversation ended, Ciel quit or the computer
// restarted. The words only, never the sound. With Claude, those lines go to
// Anthropic with the question, as the settings say.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::settings::Settings;

/// Past this the file loses its oldest days: a year or so of talking.
const MAX_FILE: usize = 300_000;
/// One question or one answer, as noted.
const LINE_CHARS: usize = 600;
/// What the brain is given of it: Claude, and a local model's small context.
pub const PROMPT_CHARS: usize = 2000;
pub const LOCAL_PROMPT_CHARS: usize = 600;

/// Work/talks.md in Ciel's own folder, beside the list of what must be done.
pub fn file() -> PathBuf {
    crate::settings::local_dir().join("Work").join("talks.md")
}

fn one_line(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > LINE_CHARS {
        text.chars().take(LINE_CHARS).chain("…".chars()).collect()
    } else {
        text
    }
}

/// The file with one more exchange under its day, the oldest days gone once
/// it is too long.
fn noted(file: &str, day: &str, time: &str, heard: &str, said: &str) -> String {
    let eol = if file.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = file.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    let heading = format!("## {day}");
    if out.lines().rev().find(|l| l.starts_with("## ")).map(str::trim) != Some(heading.as_str()) {
        if !out.is_empty() {
            out.push_str(eol);
        }
        out.push_str(&heading);
        out.push_str(eol);
    }
    out.push_str(&format!("- {time} « {} »{eol}  → {}{eol}", one_line(heard), one_line(said)));
    if out.len() > MAX_FILE {
        // From the first day that starts in the newer half.
        let mut at = 0;
        let mut cut = None;
        for line in out.split_inclusive('\n') {
            if at >= out.len() / 2 && line.starts_with("## ") {
                cut = Some(at);
                break;
            }
            at += line.len();
        }
        if let Some(cut) = cut {
            out.drain(..cut);
        }
    }
    out
}

/// The end of the file, `max` bytes or so: whole exchanges, under their day.
fn tail(file: &str, max: usize) -> String {
    let lines: Vec<&str> = file.lines().collect();
    let starts = |l: &str| l.starts_with("- ") || l.starts_with("## ");
    let mut from = lines.len();
    let mut size = 0;
    while from > 0 && size + lines[from - 1].len() + 1 <= max {
        from -= 1;
        size += lines[from].len() + 1;
    }
    while from < lines.len() && !starts(lines[from]) {
        from += 1;
    }
    let mut out = String::new();
    if from < lines.len() && !lines[from].starts_with("## ") {
        if let Some(day) = lines[..from].iter().rev().find(|l| l.starts_with("## ")) {
            out.push_str(day);
            out.push('\n');
        }
    }
    for line in lines[from..].iter().filter(|l| !l.trim().is_empty()) {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// An exchange of « OK Ciel » is over: noted, if the user asked for that.
pub fn note(app: &AppHandle, heard: &str, said: &str) {
    let on = app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().unwrap().remember_talks);
    if !on || heard.trim().is_empty() || said.trim().is_empty() {
        return;
    }
    let t = crate::platform::local_time();
    let day = format!("{:04}-{:02}-{:02}", t.year, t.month, t.day);
    let time = format!("{:02}:{:02}", t.hour, t.minute);
    let path = file();
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if crate::wallpaper::write_atomic(&path, noted(&old, &day, &time, heard, said).as_bytes()).is_err() {
        crate::log::line("talks: what was said couldn't be noted");
    }
}

/// For the brain's instructions: "" while the user didn't ask Ciel to remember,
/// or nothing was said yet.
pub fn for_prompt(settings: &Settings, max: usize) -> String {
    if !settings.remember_talks {
        return String::new();
    }
    tail(&std::fs::read_to_string(file()).unwrap_or_default(), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_exchange_goes_under_its_day() {
        let one = noted("", "2026-11-14", "09:05", "  what is next\non ciel? ", "The work file is left to ship.");
        assert_eq!(one, "## 2026-11-14\n- 09:05 « what is next on ciel? »\n  → The work file is left to ship.\n");
        let two = noted(&one, "2026-11-14", "09:06", "note that", "Noted.");
        assert_eq!(two.matches("## 2026-11-14").count(), 1);
        assert!(two.ends_with("- 09:06 « note that »\n  → Noted.\n"));
        let next_day = noted(&two, "2026-11-15", "08:00", "good morning", "Good morning.");
        assert!(next_day.contains("\n\n## 2026-11-15\n- 08:00 « good morning »\n"));
        // The same day's name earlier in the file is not today's heading.
        let back = noted(&next_day, "2026-11-14", "08:01", "again", "Yes.");
        assert_eq!(back.matches("## 2026-11-14").count(), 2);
        // The line endings of the user's editor are kept.
        let crlf = noted("## 2026-11-14\r\n- 09:05 « a »\r\n  → b\r\n", "2026-11-14", "09:07", "c", "d");
        assert!(crlf.ends_with("  → b\r\n- 09:07 « c »\r\n  → d\r\n"));
    }

    #[test]
    fn a_long_answer_is_cut_and_a_long_file_loses_its_oldest_days() {
        let long = noted("", "2026-11-14", "09:05", "tell me everything", &"word ".repeat(1000));
        assert!(long.lines().nth(2).unwrap().chars().count() <= LINE_CHARS + 5);
        let mut file = String::new();
        for day in 1..=40 {
            for minute in 0..30 {
                file = noted(&file, &format!("2026-01-{day:02}"), &format!("10:{minute:02}"), &"q".repeat(300), &"a".repeat(300));
            }
        }
        assert!(file.len() <= MAX_FILE, "{}", file.len());
        assert!(file.starts_with("## 2026-01-"), "it starts at a day");
        assert!(!file.contains("## 2026-01-01\n") && file.contains("## 2026-01-40\n"));
    }

    #[test]
    fn the_brain_gets_the_latest_exchanges_whole_under_their_day() {
        let mut file = String::new();
        for (day, time, heard, said) in [
            ("2026-11-13", "18:00", "old question", "old answer"),
            ("2026-11-14", "09:05", "what is next on ciel", "The work file."),
            ("2026-11-14", "09:06", "and after that", "The sessions."),
        ] {
            file = noted(&file, day, time, heard, said);
        }
        assert_eq!(tail(&file, 10_000), "## 2026-11-13\n- 18:00 « old question »\n  → old answer\n## 2026-11-14\n- 09:05 « what is next on ciel »\n  → The work file.\n- 09:06 « and after that »\n  → The sessions.\n");
        // Room for the last exchange only: it comes whole, with its day.
        assert_eq!(tail(&file, 60), "## 2026-11-14\n- 09:06 « and after that »\n  → The sessions.\n");
        // Room for half of it: nothing rather than an answer with no question.
        assert_eq!(tail(&file, 25), "");
        assert_eq!(tail("", 1000), "");
    }

    #[test]
    fn nothing_is_read_until_the_user_asks_ciel_to_remember() {
        let settings = Settings::default();
        assert!(!settings.remember_talks);
        assert_eq!(for_prompt(&settings, PROMPT_CHARS), "");
    }
}
