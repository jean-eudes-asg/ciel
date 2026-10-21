// What the Printers pill remembers and reports: each problem's start and end
// for 30 days (history.json, in Ciel's data folder), a plain-text report of
// every printer now, and the same as CSV for a spreadsheet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::reading::{label, Problem};
use crate::alerts::Severity;
use crate::i18n::{n_, t, tn};

const KEEP_MS: f64 = 30.0 * 24.0 * 3600.0 * 1000.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    /// The problem's alert key: printer, code and what tells it apart.
    pub key: String,
    pub printer: String,
    pub name: String,
    pub code: String,
    pub severity: Severity,
    pub detail: String,
    pub start: f64,
    pub end: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    pub incidents: Vec<Incident>,
}

impl History {
    pub fn path() -> PathBuf {
        crate::settings::local_dir().join("printers").join("history.json")
    }

    pub fn load(path: &Path) -> History {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            crate::platform::ensure_private_dir(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    /// A problem began. Once only, while it lasts.
    pub fn opened(&mut self, key: &str, printer: &str, name: &str, p: &Problem, at: f64) {
        if self.incidents.iter().any(|i| i.key == key && i.end.is_none()) {
            return;
        }
        self.incidents.push(Incident {
            key: key.to_string(),
            printer: printer.to_string(),
            name: name.to_string(),
            code: p.code.clone(),
            severity: p.severity,
            detail: p.detail.clone(),
            start: at,
            end: None,
        });
    }

    pub fn closed(&mut self, key: &str, at: f64) {
        for i in self.incidents.iter_mut().filter(|i| i.key == key && i.end.is_none()) {
            i.end = Some(at);
        }
    }

    /// Ends what is still open for a printer that left the list.
    pub fn close_printer(&mut self, printer: &str, at: f64) {
        for i in self.incidents.iter_mut().filter(|i| i.printer == printer && i.end.is_none()) {
            i.end = Some(at);
        }
    }

    /// Forgets what ended more than 30 days before `now`.
    pub fn prune(&mut self, now: f64) {
        self.incidents.retain(|i| i.end.is_none_or(|end| now - end <= KEEP_MS));
    }

    pub fn count(&self, printer: &str, now: f64) -> usize {
        self.incidents.iter().filter(|i| i.printer == printer && now - i.start <= KEEP_MS).count()
    }
}

/// One printer as the report says it.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub name: String,
    pub location: String,
    pub host: String,
    pub model: String,
    pub serial: String,
    /// Already in words: "Idle", "Not responding"…
    pub state: String,
    pub problems: Vec<Problem>,
    /// "Black Toner 64%", in the printer's order.
    pub supplies: Vec<String>,
    pub pages: Option<u64>,
    pub checked: f64,
}

/// A problem in words: its label, and what more there is.
pub fn describe(p: &Problem) -> String {
    let base = t(label(&p.code));
    match (p.code.as_str(), p.detail.is_empty()) {
        // Their detail is the whole story ("Cyan Toner 4%", the printer's words).
        ("supplyLow" | "alert", false) => p.detail.clone(),
        (_, false) => format!("{base} ({})", p.detail),
        _ => base,
    }
}

/// The plain-text report: a heading, each printer, then the last 30 days.
pub fn text(lines: &[Line], history: &History, heading: &str, now: f64) -> String {
    let mut out = format!("{heading}\n\n");
    for l in lines {
        let place = [l.location.as_str(), l.host.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
        out.push_str(&format!("{} ({place})\n", l.name));
        let problems = if l.problems.is_empty() {
            t(n_("No problems"))
        } else {
            l.problems.iter().map(describe).collect::<Vec<_>>().join("; ")
        };
        out.push_str(&format!("  {} · {problems}\n", l.state));
        if !l.supplies.is_empty() {
            out.push_str(&format!("  {}\n", l.supplies.join(" · ")));
        }
        if let Some(pages) = l.pages {
            out.push_str(&format!("  {}\n", tn("{count} page printed", "{count} pages printed", pages, &[])));
        }
    }
    let recent: Vec<&Incident> = history.incidents.iter().filter(|i| now - i.start <= KEEP_MS).collect();
    out.push('\n');
    out.push_str(&tn("Last 30 days: {count} problem", "Last 30 days: {count} problems", recent.len() as u64, &[]));
    out.push('\n');
    let mut by_printer: Vec<(String, usize)> = Vec::new();
    for i in &recent {
        match by_printer.iter_mut().find(|(n, _)| *n == i.name) {
            Some((_, c)) => *c += 1,
            None => by_printer.push((i.name.clone(), 1)),
        }
    }
    by_printer.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (name, count) in by_printer {
        out.push_str(&format!("  {name}: {count}\n"));
    }
    out
}

fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', ';']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

fn row(cells: &[String]) -> String {
    cells.iter().map(|c| field(c)).collect::<Vec<_>>().join(",") + "\r\n"
}

/// Every printer now, one row each.
pub fn csv_status(lines: &[Line]) -> String {
    let head = [
        n_("Printer"),
        n_("Location"),
        n_("Address"),
        n_("Model"),
        n_("Serial number"),
        n_("State"),
        n_("Problems"),
        n_("Supplies"),
        n_("Pages"),
        n_("Checked"),
    ];
    let mut out = row(&head.iter().map(|h| t(h)).collect::<Vec<_>>());
    for l in lines {
        out.push_str(&row(&[
            l.name.clone(),
            l.location.clone(),
            l.host.clone(),
            l.model.clone(),
            l.serial.clone(),
            l.state.clone(),
            l.problems.iter().map(describe).collect::<Vec<_>>().join("; "),
            l.supplies.join("; "),
            l.pages.map(|p| p.to_string()).unwrap_or_default(),
            iso(l.checked),
        ]));
    }
    out
}

/// The last 30 days' problems, oldest first.
pub fn csv_history(history: &History) -> String {
    let head = [n_("Printer"), n_("Problem"), n_("Severity"), n_("Started"), n_("Ended")];
    let mut out = row(&head.iter().map(|h| t(h)).collect::<Vec<_>>());
    let mut list: Vec<&Incident> = history.incidents.iter().collect();
    list.sort_by(|a, b| a.start.total_cmp(&b.start));
    for i in list {
        let p = Problem { code: i.code.clone(), severity: i.severity, detail: i.detail.clone(), sub: String::new() };
        let severity = t(match i.severity {
            Severity::Critical => n_("Critical"),
            Severity::Warning => n_("Warning"),
            Severity::Info => n_("Info"),
        });
        out.push_str(&row(&[i.name.clone(), describe(&p), severity, iso(i.start), i.end.map(iso).unwrap_or_default()]));
    }
    out
}

/// Unix ms as "2026-10-21T14:05:00Z", which every spreadsheet reads.
pub fn iso(ms: f64) -> String {
    if !ms.is_finite() || ms <= 0.0 {
        return String::new();
    }
    let secs = (ms / 1000.0).floor() as i64;
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printers::reading::problem;

    const DAY: f64 = 24.0 * 3600.0 * 1000.0;

    #[test]
    fn a_problem_is_opened_once_and_closed_when_it_ends() {
        let mut h = History::default();
        let jam = problem("jammed", "");
        h.opened("printer:p1:jammed:", "p1", "Canon 2F", &jam, 1000.0);
        h.opened("printer:p1:jammed:", "p1", "Canon 2F", &jam, 2000.0);
        assert_eq!(h.incidents.len(), 1);
        h.closed("printer:p1:jammed:", 5000.0);
        assert_eq!(h.incidents[0].end, Some(5000.0));
        // The same problem again later is a new incident.
        h.opened("printer:p1:jammed:", "p1", "Canon 2F", &jam, 9000.0);
        assert_eq!(h.incidents.len(), 2);
        assert_eq!(h.count("p1", 10_000.0), 2);
        assert_eq!(h.count("p2", 10_000.0), 0);
    }

    #[test]
    fn thirty_days_are_kept_and_what_is_still_open_always() {
        let mut h = History::default();
        let now = 100.0 * DAY;
        let p = problem("noPaper", "");
        h.opened("old", "p1", "A", &p, now - 40.0 * DAY);
        h.closed("old", now - 39.0 * DAY);
        h.opened("recent", "p1", "A", &p, now - 10.0 * DAY);
        h.closed("recent", now - 9.0 * DAY);
        h.opened("open", "p1", "A", &p, now - 45.0 * DAY);
        h.prune(now);
        let keys: Vec<&str> = h.incidents.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["recent", "open"]);
    }

    #[test]
    fn the_history_survives_a_save_and_a_load() {
        let dir = std::env::temp_dir().join(format!("ciel-printers-{}", std::process::id()));
        let path = dir.join("history.json");
        let mut h = History::default();
        h.opened("k", "p1", "Kyocera", &problem("doorOpen", ""), 1.0);
        h.save(&path).unwrap();
        assert_eq!(History::load(&path), h);
        assert_eq!(History::load(&dir.join("missing.json")), History::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn line() -> Line {
        Line {
            name: "Canon 2F".into(),
            location: "2nd floor".into(),
            host: "10.0.2.40".into(),
            model: "Canon MF650C, Series".into(),
            serial: "XKZ01234".into(),
            state: "Idle".into(),
            problems: vec![problem("jammed", ""), Problem { detail: "Cyan Toner 4%".into(), ..problem("supplyLow", "") }],
            supplies: vec!["Black Toner 64%".into(), "Cyan Toner 4%".into()],
            pages: Some(48210),
            checked: 1_761_055_500_000.0,
        }
    }

    #[test]
    fn the_text_report_says_each_printer_then_the_month() {
        let mut h = History::default();
        h.opened("a", "p1", "Canon 2F", &problem("jammed", ""), 1_761_000_000_000.0);
        h.opened("b", "p1", "Canon 2F", &problem("noPaper", ""), 1_761_000_000_000.0);
        h.opened("c", "p2", "Brother 1F", &problem("noPaper", ""), 1_761_000_000_000.0);
        let report = text(&[line()], &h, "Printers, 21 Oct 2026 17:05", 1_761_055_500_000.0);
        assert!(report.starts_with("Printers, 21 Oct 2026 17:05\n\nCanon 2F (2nd floor, 10.0.2.40)\n"));
        assert!(report.contains("  Idle · Paper jam; Cyan Toner 4%\n"));
        assert!(report.contains("  Black Toner 64% · Cyan Toner 4%\n"));
        assert!(report.contains("48210 pages printed"));
        assert!(report.contains("Last 30 days: 3 problems\n  Canon 2F: 2\n  Brother 1F: 1\n"));
    }

    #[test]
    fn csv_quotes_what_needs_it() {
        let csv = csv_status(&[line()]);
        let mut rows = csv.split("\r\n");
        assert_eq!(rows.next(), Some("Printer,Location,Address,Model,Serial number,State,Problems,Supplies,Pages,Checked"));
        assert_eq!(
            rows.next(),
            Some("Canon 2F,2nd floor,10.0.2.40,\"Canon MF650C, Series\",XKZ01234,Idle,\"Paper jam; Cyan Toner 4%\",\"Black Toner 64%; Cyan Toner 4%\",48210,2025-10-21T14:05:00Z")
        );
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        let mut h = History::default();
        h.opened("a", "p1", "Canon 2F", &problem("doorOpen", ""), 1_761_000_000_000.0);
        assert!(csv_history(&h).contains("Canon 2F,Door open,Critical,2025-10-20T22:40:00Z,\r\n"));
    }

    #[test]
    fn iso_dates_are_utc_calendar_dates() {
        assert_eq!(iso(0.0), "");
        assert_eq!(iso(1000.0), "1970-01-01T00:00:01Z");
        assert_eq!(iso(951_782_400_000.0), "2000-02-29T00:00:00Z");
        assert_eq!(iso(1_792_591_200_000.0), "2026-10-21T14:00:00Z");
    }
}
