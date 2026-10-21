// What a printer says about itself, read the same way whichever protocol it
// came by: its state, its problems, its supplies and its paper trays.
//
// Over SNMP: the Host Resources MIB's printer status and error bits (RFC 2790)
// and the Printer MIB's supplies, inputs, alerts and front panel (RFC 3805).
// Over IPP: printer-state, printer-state-reasons and the marker-* attributes.

use std::collections::BTreeMap;

use serde::Serialize;

use super::ber::{self, Value};
use super::ipp::{Attributes, IppValue};
use crate::alerts::Severity;
use crate::i18n::n_;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Printing,
    Warmup,
    Stopped,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// One of PROBLEMS: stable, the key of its alert.
    pub code: String,
    pub severity: Severity,
    /// What more there is to say: a cartridge and its level, the printer's own words.
    pub detail: String,
    /// Tells two problems of the same code apart (two cartridges low).
    pub sub: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Supply {
    pub name: String,
    /// "toner", "ink", "drum", "waste", "fuser" or "other".
    pub kind: String,
    /// "#RRGGBB", when the printer says.
    pub color: Option<String>,
    /// What is left, in percent; for a waste box, the room left.
    pub level: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tray {
    pub name: String,
    pub level: Option<u8>,
    pub empty: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub state: State,
    pub model: String,
    pub serial: String,
    pub location: String,
    /// The front panel's text, or the printer's state message.
    pub panel: String,
    pub problems: Vec<Problem>,
    pub supplies: Vec<Supply>,
    pub trays: Vec<Tray>,
    pub pages: Option<u64>,
}

use Severity::{Critical, Info, Warning};

/// Every problem the pill knows: its code, its label (translated where shown;
/// src/views/printers.ts has the same table) and how bad it is.
pub const PROBLEMS: &[(&str, &str, Severity)] = &[
    ("jammed", n_("Paper jam"), Critical),
    ("doorOpen", n_("Door open"), Critical),
    ("noPaper", n_("Out of paper"), Critical),
    ("noToner", n_("Toner empty"), Critical),
    ("offline", n_("Offline"), Critical),
    ("unreachable", n_("Not responding"), Critical),
    ("serviceRequested", n_("Service requested"), Critical),
    ("supplyMissing", n_("Cartridge missing"), Critical),
    ("outputFull", n_("Output tray full"), Critical),
    ("outputTrayMissing", n_("Output tray missing"), Critical),
    ("wasteFull", n_("Waste toner box full"), Critical),
    ("stopped", n_("Stopped"), Critical),
    ("lowPaper", n_("Paper low"), Warning),
    ("inputTrayEmpty", n_("Paper tray empty"), Warning),
    ("inputTrayMissing", n_("Paper tray missing"), Warning),
    ("lowToner", n_("Toner low"), Warning),
    ("outputNearFull", n_("Output tray almost full"), Warning),
    ("wasteNearFull", n_("Waste toner box almost full"), Warning),
    ("maintenance", n_("Maintenance due"), Warning),
    ("supplyLow", n_("Running low"), Warning),
    ("refused", n_("Credentials refused"), Warning),
    ("alert", n_("Printer alert"), Warning),
];

/// The English label of a code (a key of the translations).
pub fn label(code: &str) -> &'static str {
    PROBLEMS.iter().find(|(c, _, _)| *c == code).map_or("Printer alert", |(_, l, _)| l)
}

pub fn problem(code: &str, detail: impl Into<String>) -> Problem {
    let severity = PROBLEMS.iter().find(|(c, _, _)| *c == code).map_or(Warning, |(_, _, s)| *s);
    Problem { code: code.to_string(), severity, detail: detail.into(), sub: String::new() }
}

fn color_hex(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    let hex = if lower.contains("black") || lower.contains("schwarz") || lower.contains("noir") {
        "#000000"
    } else if lower.contains("cyan") {
        "#00A0E0"
    } else if lower.contains("magenta") {
        "#E0007A"
    } else if lower.contains("yellow") || lower.contains("jaune") || lower.contains("gelb") {
        "#FFD400"
    } else {
        return None;
    };
    Some(hex.to_string())
}

fn percent(level: i64, max: i64) -> Option<u8> {
    (max > 0 && level >= 0).then(|| (level.saturating_mul(100) / max).clamp(0, 100) as u8)
}

// ── SNMP ──────────────────────────────────────────────────────────────────────

const SYS_DESCR: &str = "1.3.6.1.2.1.1.1.0";
const SYS_LOCATION: &str = "1.3.6.1.2.1.1.6.0";
const SERIAL: &str = "1.3.6.1.2.1.43.5.1.1.17.1";
const PAGES: &str = "1.3.6.1.2.1.43.10.2.1.4.1.1";
const HR_PRINTER: &str = "1.3.6.1.2.1.25.3.5.1";
const HR_DEVICE_DESCR: &str = "1.3.6.1.2.1.25.3.2.1.3";
const HR_DEVICE_STATUS: &str = "1.3.6.1.2.1.25.3.2.1.5";
const INPUTS: &str = "1.3.6.1.2.1.43.8.2.1";
const SUPPLIES: &str = "1.3.6.1.2.1.43.11.1.1";
const COLORANTS: &str = "1.3.6.1.2.1.43.12.1.1.4";
const CONSOLE: &str = "1.3.6.1.2.1.43.16.5.1.2";
const ALERTS: &str = "1.3.6.1.2.1.43.18.1.1";

/// What one GET asks.
pub fn snmp_scalars() -> Vec<Vec<u32>> {
    [SYS_DESCR, SYS_LOCATION, SERIAL, PAGES].iter().map(|o| ber::parse_oid(o)).collect()
}

/// The tables walked, one after the other.
pub fn snmp_tables() -> Vec<Vec<u32>> {
    [HR_PRINTER, HR_DEVICE_DESCR, HR_DEVICE_STATUS, SUPPLIES, COLORANTS, INPUTS, ALERTS, CONSOLE]
        .iter()
        .map(|o| ber::parse_oid(o))
        .collect()
}

pub type Vars = BTreeMap<Vec<u32>, Value>;

/// A table under `prefix`: each row's index, then each column's value.
fn table<'a>(vars: &'a Vars, prefix: &str) -> BTreeMap<Vec<u32>, BTreeMap<u32, &'a Value>> {
    let prefix = ber::parse_oid(prefix);
    let mut rows: BTreeMap<Vec<u32>, BTreeMap<u32, &Value>> = BTreeMap::new();
    for (oid, value) in vars.range(prefix.clone()..).take_while(|(o, _)| o.starts_with(&prefix)) {
        if let Some((&column, index)) = oid[prefix.len()..].split_first() {
            rows.entry(index.to_vec()).or_default().insert(column, value);
        }
    }
    rows
}

/// The column of a table under `column_prefix`: each row's index and value.
fn column<'a>(vars: &'a Vars, column_prefix: &str) -> Vec<(Vec<u32>, &'a Value)> {
    let prefix = ber::parse_oid(column_prefix);
    vars.range(prefix.clone()..)
        .take_while(|(o, _)| o.starts_with(&prefix))
        .map(|(o, v)| (o[prefix.len()..].to_vec(), v))
        .filter(|(_, v)| v.present())
        .collect()
}

fn scalar<'a>(vars: &'a Vars, oid: &str) -> Option<&'a Value> {
    vars.get(&ber::parse_oid(oid)).filter(|v| v.present())
}

/// hrPrinterDetectedErrorState: two bytes of bits, the first bit the highest.
const ERROR_BITS: [(usize, u8, &str); 15] = [
    (0, 0x80, "lowPaper"),
    (0, 0x40, "noPaper"),
    (0, 0x20, "lowToner"),
    (0, 0x10, "noToner"),
    (0, 0x08, "doorOpen"),
    (0, 0x04, "jammed"),
    (0, 0x02, "offline"),
    (0, 0x01, "serviceRequested"),
    (1, 0x80, "inputTrayMissing"),
    (1, 0x40, "outputTrayMissing"),
    (1, 0x20, "supplyMissing"),
    (1, 0x10, "outputNearFull"),
    (1, 0x08, "outputFull"),
    (1, 0x04, "inputTrayEmpty"),
    (1, 0x02, "maintenance"),
];

pub fn from_snmp(vars: &Vars) -> Reading {
    let mut r = Reading::default();
    let printer = table(vars, HR_PRINTER);
    // The first printer the agent has (hrDeviceIndex, 1 almost always).
    let (device, row) = printer.iter().next().map(|(i, row)| (i.clone(), Some(row))).unwrap_or((vec![1], None));
    let device_descr = column(vars, HR_DEVICE_DESCR).into_iter().find(|(i, _)| *i == device).map(|(_, v)| v.text());
    r.model = device_descr
        .filter(|d| !d.is_empty())
        .or_else(|| scalar(vars, SYS_DESCR).map(Value::text))
        .unwrap_or_default();
    r.location = scalar(vars, SYS_LOCATION).map(Value::text).unwrap_or_default();
    r.serial = scalar(vars, SERIAL).map(Value::text).unwrap_or_default();
    r.pages = scalar(vars, PAGES).and_then(Value::as_i64).and_then(|p| u64::try_from(p).ok());

    let status = row.and_then(|row| row.get(&1)).and_then(|v| v.as_i64());
    r.state = match status {
        Some(3) => State::Idle,
        Some(4) => State::Printing,
        Some(5) => State::Warmup,
        _ => State::Unknown,
    };
    let down = column(vars, HR_DEVICE_STATUS).into_iter().any(|(i, v)| i == device && v.as_i64() == Some(5));
    if down {
        r.state = State::Stopped;
    }

    let bits = row.and_then(|row| row.get(&2)).map(|v| v.bytes().to_vec()).unwrap_or_default();
    for (byte, mask, code) in ERROR_BITS {
        if bits.get(byte).is_some_and(|b| b & mask != 0) {
            r.problems.push(problem(code, ""));
        }
    }

    // The printer's own alerts: critical and warning ones, said once each.
    let mut alerts: Vec<(Severity, String)> = Vec::new();
    for row in table(vars, ALERTS).values() {
        let severity = match row.get(&2).and_then(|v| v.as_i64()) {
            Some(3) => Critical,
            Some(4) | Some(5) => Warning,
            _ => continue,
        };
        let text = row.get(&8).map(|v| v.text()).unwrap_or_default();
        if !text.is_empty() && !alerts.iter().any(|(_, t)| t.eq_ignore_ascii_case(&text)) {
            alerts.push((severity, text));
        }
    }
    alerts.sort_by(|a, b| b.0.cmp(&a.0));
    let said: Vec<&str> = alerts.iter().map(|(_, t)| t.as_str()).collect();
    if r.problems.is_empty() {
        for (severity, text) in &alerts {
            r.problems.push(Problem { code: "alert".into(), severity: *severity, detail: text.clone(), sub: text.clone() });
        }
    } else {
        // "Service requested" says little: the alerts say what.
        for p in r.problems.iter_mut().filter(|p| p.code == "serviceRequested" || p.code == "maintenance") {
            p.detail = said.join(" · ");
        }
    }
    if r.state == State::Stopped && r.problems.is_empty() {
        r.problems.push(problem("stopped", ""));
    }

    let colorants: BTreeMap<Vec<u32>, String> = column(vars, COLORANTS).into_iter().map(|(i, v)| (i, v.text())).collect();
    for (index, row) in table(vars, SUPPLIES) {
        let int = |c: u32| row.get(&c).and_then(|v| v.as_i64());
        let kind = match (int(4), int(5)) {
            (Some(4), _) | (_, Some(4)) | (_, Some(8)) => "waste",
            (_, Some(3)) => "toner",
            (_, Some(5..=7)) => "ink",
            (_, Some(9)) => "drum",
            (_, Some(15)) => "fuser",
            _ => "other",
        };
        let colorant = int(3).and_then(|c| colorants.get(&vec![index[0], u32::try_from(c).ok()?]));
        let name = row.get(&6).map(|v| v.text()).filter(|n| !n.is_empty()).unwrap_or_else(|| kind.to_string());
        let color = colorant.and_then(|c| color_hex(c)).or_else(|| color_hex(&name));
        let level = match (int(9), int(8)) {
            (Some(level), Some(max)) => percent(level, max),
            _ => None,
        };
        r.supplies.push(Supply { name, kind: kind.into(), color, level });
    }

    for (index, row) in table(vars, INPUTS) {
        let int = |c: u32| row.get(&c).and_then(|v| v.as_i64());
        let name = [13, 18, 12]
            .iter()
            .find_map(|c| row.get(c).map(|v| v.text()).filter(|n| !n.is_empty()))
            .unwrap_or_else(|| format!("{}", index.last().copied().unwrap_or(0)));
        let (current, max) = (int(10), int(9));
        r.trays.push(Tray {
            name,
            level: current.zip(max).and_then(|(c, m)| percent(c, m)),
            empty: current == Some(0),
        });
    }

    r.panel = column(vars, CONSOLE).into_iter().map(|(_, v)| v.text()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" · ");
    r
}

// ── IPP ───────────────────────────────────────────────────────────────────────

/// printer-state-reasons' keyword, without its -report, -warning or -error.
fn reason_code(base: &str) -> Option<&'static str> {
    Some(match base {
        "media-jam" => "jammed",
        "door-open" | "cover-open" | "interlock-open" => "doorOpen",
        "media-empty" | "media-needed" => "noPaper",
        "media-low" => "lowPaper",
        "toner-empty" | "marker-supply-empty" => "noToner",
        "toner-low" | "marker-supply-low" => "lowToner",
        "offline" | "shutdown" => "offline",
        "input-tray-missing" => "inputTrayMissing",
        "output-tray-missing" => "outputTrayMissing",
        "output-area-almost-full" => "outputNearFull",
        "output-area-full" => "outputFull",
        "marker-supply-missing" | "toner-missing" => "supplyMissing",
        "marker-waste-almost-full" => "wasteNearFull",
        "marker-waste-full" => "wasteFull",
        "opc-near-eol" | "developer-low" => "maintenance",
        "opc-life-over" | "developer-empty" | "fuser-over-temp" | "fuser-under-temp" => "serviceRequested",
        "paused" | "moving-to-paused" => "stopped",
        _ => return None,
    })
}

fn texts(attrs: &Attributes, name: &str) -> Vec<String> {
    attrs.get(name).map(|v| v.iter().filter_map(|x| x.text().map(str::to_string)).collect()).unwrap_or_default()
}

fn first_text(attrs: &Attributes, name: &str) -> String {
    texts(attrs, name).into_iter().next().unwrap_or_default().trim().to_string()
}

pub fn from_ipp(attrs: &Attributes) -> Reading {
    let mut r = Reading {
        model: first_text(attrs, "printer-make-and-model"),
        location: first_text(attrs, "printer-location"),
        ..Reading::default()
    };
    r.state = match attrs.get("printer-state").and_then(|v| v.first()).and_then(IppValue::int) {
        Some(3) => State::Idle,
        Some(4) => State::Printing,
        Some(5) => State::Stopped,
        _ => State::Unknown,
    };
    // "MFG:Canon;MDL:MF650C;SN:XYZ123;" — the serial number, when it's there.
    r.serial = first_text(attrs, "printer-device-id")
        .split(';')
        .find_map(|part| part.trim().strip_prefix("SN:").map(str::to_string))
        .unwrap_or_default();
    let mut panel = vec![first_text(attrs, "printer-state-message")];
    panel.extend(texts(attrs, "printer-alert-description"));
    // Some printers put keywords there ("toner-low-warning", "none", "unknown"): not words for people.
    panel.retain(|p| {
        let p = p.trim();
        !p.is_empty() && !["none", "unknown", "other"].contains(&p) && !(p.contains('-') && !p.contains(' '))
    });
    panel.dedup();
    r.panel = panel.join(" · ");

    for reason in texts(attrs, "printer-state-reasons") {
        let (base, suffix) = ["-error", "-warning", "-report"]
            .iter()
            .find_map(|s| reason.strip_suffix(s).map(|b| (b, *s)))
            .unwrap_or((reason.as_str(), "-error"));
        // A print server's own states ("cups-waiting-for-job-completed") and a
        // vendor's ("com.example.…") say nothing about the printer.
        if base == "none" || base == "other" || base.is_empty() || base.starts_with("cups-") || base.contains('.') {
            continue;
        }
        let p = match reason_code(base) {
            Some(code) => problem(code, ""),
            // One the pill doesn't know: said as the printer says it, unless it's only a report.
            None if suffix == "-report" => continue,
            None => Problem {
                code: "alert".into(),
                severity: if suffix == "-error" { Critical } else { Warning },
                detail: base.replace('-', " "),
                sub: base.to_string(),
            },
        };
        if !r.problems.iter().any(|q| q.code == p.code && q.sub == p.sub) {
            r.problems.push(p);
        }
    }
    if r.state == State::Stopped && r.problems.is_empty() {
        r.problems.push(problem("stopped", ""));
    }

    let names = texts(attrs, "marker-names");
    let colors = texts(attrs, "marker-colors");
    let types = texts(attrs, "marker-types");
    let levels: Vec<Option<i64>> = attrs.get("marker-levels").map(|v| v.iter().map(IppValue::int).collect()).unwrap_or_default();
    for (i, name) in names.iter().enumerate() {
        let kind = match types.get(i).map(String::as_str).unwrap_or("") {
            t if t.starts_with("waste") => "waste",
            t if t.starts_with("toner") => "toner",
            t if t.starts_with("ink") => "ink",
            "opc" | "photo-conductor" => "drum",
            t if t.starts_with("fuser") => "fuser",
            _ => "other",
        };
        let color = colors
            .get(i)
            .and_then(|c| c.get(..7))
            .filter(|c| c.starts_with('#') && c.len() == 7)
            .map(str::to_uppercase)
            .or_else(|| color_hex(name));
        // A waste box's level means room left to some printers and fill to others: not shown.
        let level = levels.get(i).copied().flatten().filter(|l| (0..=100).contains(l) && kind != "waste").map(|l| l as u8);
        r.supplies.push(Supply { name: name.clone(), kind: kind.into(), color, level });
    }
    r
}

// ── Thresholds ────────────────────────────────────────────────────────────────

/// A cartridge at or under `warn` percent is low, at or under `critical`
/// nearly empty: one problem each, until it's replaced. Where the levels are
/// known they say more than the printer's own "toner low".
pub fn apply_thresholds(r: &mut Reading, warn: u8, critical: u8) {
    let mut known = false;
    let mut critical_one = false;
    for supply in &r.supplies {
        if !matches!(supply.kind.as_str(), "toner" | "ink" | "drum") {
            continue;
        }
        let Some(level) = supply.level else { continue };
        known |= supply.kind != "drum";
        let severity = if level <= critical {
            Critical
        } else if level <= warn {
            Warning
        } else {
            continue;
        };
        critical_one |= severity == Critical;
        r.problems.push(Problem {
            code: "supplyLow".into(),
            severity,
            detail: format!("{} {level}%", supply.name),
            sub: supply.name.clone(),
        });
    }
    if known {
        r.problems.retain(|p| p.code != "lowToner");
    }
    if critical_one {
        r.problems.retain(|p| p.code != "noToner");
    }
    let _ = Info;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(list: &[(&str, Value)]) -> Vars {
        list.iter().map(|(o, v)| (ber::parse_oid(o), v.clone())).collect()
    }

    fn codes(r: &Reading) -> Vec<&str> {
        r.problems.iter().map(|p| p.code.as_str()).collect()
    }

    /// A colour laser over SNMP: jammed, cyan low, the panel saying so.
    fn laser() -> Vars {
        vars(&[
            ("1.3.6.1.2.1.1.1.0", Value::Octets(b"Canon MF650C Series /P".to_vec())),
            ("1.3.6.1.2.1.1.6.0", Value::Octets(b"2nd floor".to_vec())),
            ("1.3.6.1.2.1.25.3.2.1.3.1", Value::Octets(b"Canon MF650C Series".to_vec())),
            ("1.3.6.1.2.1.25.3.2.1.5.1", Value::Int(3)),
            ("1.3.6.1.2.1.25.3.5.1.1.1", Value::Int(3)),
            ("1.3.6.1.2.1.25.3.5.1.2.1", Value::Octets(vec![0x04, 0x00])),
            ("1.3.6.1.2.1.43.5.1.1.17.1", Value::Octets(b"XKZ01234".to_vec())),
            ("1.3.6.1.2.1.43.10.2.1.4.1.1", Value::Unsigned(48210)),
            ("1.3.6.1.2.1.43.11.1.1.3.1.1", Value::Int(1)),
            ("1.3.6.1.2.1.43.11.1.1.3.1.2", Value::Int(2)),
            ("1.3.6.1.2.1.43.11.1.1.4.1.1", Value::Int(3)),
            ("1.3.6.1.2.1.43.11.1.1.4.1.2", Value::Int(3)),
            ("1.3.6.1.2.1.43.11.1.1.4.1.3", Value::Int(4)),
            ("1.3.6.1.2.1.43.11.1.1.5.1.1", Value::Int(3)),
            ("1.3.6.1.2.1.43.11.1.1.5.1.2", Value::Int(3)),
            ("1.3.6.1.2.1.43.11.1.1.5.1.3", Value::Int(4)),
            ("1.3.6.1.2.1.43.11.1.1.6.1.1", Value::Octets(b"Black Toner".to_vec())),
            ("1.3.6.1.2.1.43.11.1.1.6.1.2", Value::Octets(b"Cyan Toner".to_vec())),
            ("1.3.6.1.2.1.43.11.1.1.6.1.3", Value::Octets(b"Waste Toner".to_vec())),
            ("1.3.6.1.2.1.43.11.1.1.8.1.1", Value::Int(100)),
            ("1.3.6.1.2.1.43.11.1.1.8.1.2", Value::Int(2300)),
            ("1.3.6.1.2.1.43.11.1.1.8.1.3", Value::Int(-2)),
            ("1.3.6.1.2.1.43.11.1.1.9.1.1", Value::Int(64)),
            ("1.3.6.1.2.1.43.11.1.1.9.1.2", Value::Int(92)),
            ("1.3.6.1.2.1.43.11.1.1.9.1.3", Value::Int(-3)),
            ("1.3.6.1.2.1.43.12.1.1.4.1.1", Value::Octets(b"black".to_vec())),
            ("1.3.6.1.2.1.43.12.1.1.4.1.2", Value::Octets(b"cyan".to_vec())),
            ("1.3.6.1.2.1.43.8.2.1.9.1.1", Value::Int(250)),
            ("1.3.6.1.2.1.43.8.2.1.10.1.1", Value::Int(0)),
            ("1.3.6.1.2.1.43.8.2.1.13.1.1", Value::Octets(b"Drawer 1".to_vec())),
            ("1.3.6.1.2.1.43.8.2.1.9.1.2", Value::Int(100)),
            ("1.3.6.1.2.1.43.8.2.1.10.1.2", Value::Int(-3)),
            ("1.3.6.1.2.1.43.8.2.1.13.1.2", Value::Octets(b"Multi-purpose Tray".to_vec())),
            ("1.3.6.1.2.1.43.16.5.1.2.1.1", Value::Octets(b"Paper jam.".to_vec())),
            ("1.3.6.1.2.1.43.16.5.1.2.1.2", Value::Octets(b"Open the front cover".to_vec())),
        ])
    }

    #[test]
    fn snmp_says_the_state_the_problems_and_the_levels() {
        let r = from_snmp(&laser());
        assert_eq!(r.state, State::Idle);
        assert_eq!(r.model, "Canon MF650C Series");
        assert_eq!((r.location.as_str(), r.serial.as_str(), r.pages), ("2nd floor", "XKZ01234", Some(48210)));
        assert_eq!(codes(&r), ["jammed"]);
        assert_eq!(r.panel, "Paper jam. · Open the front cover");
        assert_eq!(
            r.supplies,
            [
                Supply { name: "Black Toner".into(), kind: "toner".into(), color: Some("#000000".into()), level: Some(64) },
                Supply { name: "Cyan Toner".into(), kind: "toner".into(), color: Some("#00A0E0".into()), level: Some(4) },
                Supply { name: "Waste Toner".into(), kind: "waste".into(), color: None, level: None },
            ]
        );
        assert_eq!(r.trays[0], Tray { name: "Drawer 1".into(), level: Some(0), empty: true });
        assert_eq!(r.trays[1], Tray { name: "Multi-purpose Tray".into(), level: None, empty: false });
    }

    #[test]
    fn every_error_bit_has_its_problem() {
        let mut v = laser();
        v.insert(ber::parse_oid("1.3.6.1.2.1.25.3.5.1.2.1"), Value::Octets(vec![0xFF, 0xFE]));
        let r = from_snmp(&v);
        assert_eq!(r.problems.len(), 15);
        assert!(r.problems.iter().all(|p| PROBLEMS.iter().any(|(c, _, _)| *c == p.code)));
        // A one-byte value (some agents send only what they need) reads too.
        v.insert(ber::parse_oid("1.3.6.1.2.1.25.3.5.1.2.1"), Value::Octets(vec![0x40]));
        assert_eq!(codes(&from_snmp(&v)), ["noPaper"]);
    }

    #[test]
    fn the_printers_own_alerts_speak_when_the_bits_dont() {
        let mut v = laser();
        v.insert(ber::parse_oid("1.3.6.1.2.1.25.3.5.1.2.1"), Value::Octets(vec![0, 0]));
        v.insert(ber::parse_oid("1.3.6.1.2.1.43.18.1.1.2.1.1"), Value::Int(4));
        v.insert(ber::parse_oid("1.3.6.1.2.1.43.18.1.1.8.1.1"), Value::Octets(b"Replace fuser soon".to_vec()));
        v.insert(ber::parse_oid("1.3.6.1.2.1.43.18.1.1.2.1.2"), Value::Int(1));
        v.insert(ber::parse_oid("1.3.6.1.2.1.43.18.1.1.8.1.2"), Value::Octets(b"Sleep mode".to_vec()));
        let r = from_snmp(&v);
        assert_eq!(r.problems, [Problem {
            code: "alert".into(),
            severity: Warning,
            detail: "Replace fuser soon".into(),
            sub: "Replace fuser soon".into(),
        }]);
        // With "service requested" in the bits, the alert says what the service is.
        v.insert(ber::parse_oid("1.3.6.1.2.1.25.3.5.1.2.1"), Value::Octets(vec![0x01, 0]));
        let r = from_snmp(&v);
        assert_eq!((r.problems[0].code.as_str(), r.problems[0].detail.as_str()), ("serviceRequested", "Replace fuser soon"));
    }

    #[test]
    fn a_device_down_without_a_reason_is_stopped() {
        let mut v = laser();
        v.insert(ber::parse_oid("1.3.6.1.2.1.25.3.5.1.2.1"), Value::Octets(vec![0, 0]));
        v.insert(ber::parse_oid("1.3.6.1.2.1.25.3.2.1.5.1"), Value::Int(5));
        let r = from_snmp(&v);
        assert_eq!((r.state, codes(&r)), (State::Stopped, vec!["stopped"]));
    }

    fn ipp(list: &[(&str, Vec<IppValue>)]) -> Attributes {
        list.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    fn t(s: &str) -> IppValue {
        IppValue::Text(s.into())
    }

    #[test]
    fn ipp_reasons_map_to_the_same_problems() {
        let attrs = ipp(&[
            ("printer-state", vec![IppValue::Int(5)]),
            ("printer-state-reasons", vec![t("media-empty-error"), t("door-open-report"), t("toner-low-warning"), t("cups-waiting-for-job-completed")]),
            ("printer-state-message", vec![t("Load paper in tray 2")]),
            ("printer-make-and-model", vec![t("Brother HL-L6410DN series")]),
            ("printer-location", vec![t("Accounting")]),
            ("printer-device-id", vec![t("MFG:Brother;CMD:PJL,PCL;MDL:HL-L6410DN series;SN:E7Z123456;")]),
            ("marker-names", vec![t("Black Toner"), t("Drum Unit"), t("Waste Toner Box")]),
            ("marker-colors", vec![t("#000000"), t("none"), t("none")]),
            ("marker-types", vec![t("toner-cartridge"), t("opc"), t("waste-toner")]),
            ("marker-levels", vec![IppValue::Int(12), IppValue::Int(-3), IppValue::Int(70)]),
        ]);
        let r = from_ipp(&attrs);
        assert_eq!(r.state, State::Stopped);
        assert_eq!(codes(&r), ["noPaper", "doorOpen", "lowToner"]);
        assert_eq!((r.model.as_str(), r.location.as_str(), r.serial.as_str()), ("Brother HL-L6410DN series", "Accounting", "E7Z123456"));
        assert_eq!(r.panel, "Load paper in tray 2");
        assert_eq!(r.supplies[0], Supply { name: "Black Toner".into(), kind: "toner".into(), color: Some("#000000".into()), level: Some(12) });
        assert_eq!((r.supplies[1].kind.as_str(), r.supplies[1].level), ("drum", None));
        assert_eq!((r.supplies[2].kind.as_str(), r.supplies[2].level), ("waste", None));
    }

    #[test]
    fn an_unknown_ipp_error_is_said_as_the_printer_says_it() {
        let r = from_ipp(&ipp(&[
            ("printer-state", vec![IppValue::Int(3)]),
            ("printer-state-reasons", vec![t("spool-area-full-error"), t("other-warning"), t("none")]),
            ("printer-alert-description", vec![t("none"), t("toner-low-warning"), t("Low security")]),
        ]));
        assert_eq!(r.problems, [Problem { code: "alert".into(), severity: Critical, detail: "spool area full".into(), sub: "spool-area-full".into() }]);
        assert_eq!(r.panel, "Low security");
    }

    #[test]
    fn thresholds_name_the_cartridge_and_replace_the_vague_toner_low() {
        let mut r = from_snmp(&laser());
        r.problems.push(problem("lowToner", ""));
        apply_thresholds(&mut r, 15, 5);
        assert_eq!(codes(&r), ["jammed", "supplyLow"]);
        assert_eq!((r.problems[1].severity, r.problems[1].detail.as_str()), (Critical, "Cyan Toner 4%"));
        // At 10 %, only low.
        r.supplies[1].level = Some(10);
        r.problems.retain(|p| p.code != "supplyLow");
        apply_thresholds(&mut r, 15, 5);
        assert_eq!(r.problems[1].severity, Warning);
        // Levels unknown: the printer's own word stands.
        let mut blind = Reading { problems: vec![problem("lowToner", "")], ..Reading::default() };
        apply_thresholds(&mut blind, 15, 5);
        assert_eq!(codes(&blind), ["lowToner"]);
    }

    #[test]
    fn every_code_has_a_label_and_a_severity() {
        for (code, label_text, _) in PROBLEMS {
            assert_eq!(label(code), *label_text);
        }
        assert_eq!(label("something new"), "Printer alert");
        assert_eq!(problem("jammed", "").severity, Critical);
        assert_eq!(problem("lowPaper", "").severity, Warning);
    }
}
