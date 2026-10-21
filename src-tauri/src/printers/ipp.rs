// IPP (RFC 8010, RFC 8011), one operation: Get-Printer-Attributes, which every
// IPP Everywhere and AirPrint printer answers over HTTP on port 631. It says
// the printer's state, why (printer-state-reasons), and its supply levels.

use std::collections::BTreeMap;
use std::time::Duration;

const GET_PRINTER_ATTRIBUTES: u16 = 0x000B;
const OPERATION: u8 = 0x01;
const PRINTER: u8 = 0x04;
const END: u8 = 0x03;
const INTEGER: u8 = 0x21;
const BOOLEAN: u8 = 0x22;
const ENUM: u8 = 0x23;
const URI: u8 = 0x45;
const KEYWORD: u8 = 0x44;
const CHARSET: u8 = 0x47;
const LANGUAGE: u8 = 0x48;
const BEGIN_COLLECTION: u8 = 0x34;
const TEXT_WITH_LANGUAGE: u8 = 0x35;
const NAME_WITH_LANGUAGE: u8 = 0x36;
const END_COLLECTION: u8 = 0x37;

/// Where printers answer, in the order they are tried.
pub const PATHS: [&str; 3] = ["/ipp/print", "/ipp", "/"];
const TIMEOUT: Duration = Duration::from_secs(5);

/// What the Printers pill asks for.
const WANTED: [&str; 12] = [
    "printer-state",
    "printer-state-reasons",
    "printer-state-message",
    "printer-make-and-model",
    "printer-location",
    "printer-info",
    "printer-device-id",
    "marker-names",
    "marker-levels",
    "marker-colors",
    "marker-types",
    "printer-alert-description",
];

#[derive(Debug, Clone, PartialEq)]
pub enum IppValue {
    Int(i64),
    Bool(bool),
    Text(String),
    Other,
}

impl IppValue {
    pub fn int(&self) -> Option<i64> {
        match self {
            IppValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            IppValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// The printer's attributes: each name with all its values.
pub type Attributes = BTreeMap<String, Vec<IppValue>>;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Nothing answered on the port.
    Unreachable(String),
    /// Something answered, but not IPP, or the printer refused.
    Refused(String),
    Malformed,
}

fn attribute(out: &mut Vec<u8>, tag: u8, name: &str, value: &[u8]) {
    out.push(tag);
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend_from_slice(value);
}

/// The request for `uri`'s attributes.
pub fn request(uri: &str, request_id: u32) -> Vec<u8> {
    let mut out = vec![0x02, 0x00];
    out.extend_from_slice(&GET_PRINTER_ATTRIBUTES.to_be_bytes());
    out.extend_from_slice(&request_id.to_be_bytes());
    out.push(OPERATION);
    attribute(&mut out, CHARSET, "attributes-charset", b"utf-8");
    attribute(&mut out, LANGUAGE, "attributes-natural-language", b"en");
    attribute(&mut out, URI, "printer-uri", uri.as_bytes());
    for (i, name) in WANTED.iter().enumerate() {
        // The other values of a list come without a name.
        attribute(&mut out, KEYWORD, if i == 0 { "requested-attributes" } else { "" }, name.as_bytes());
    }
    out.push(END);
    out
}

/// The status code and the printer group's attributes. Collections are
/// skipped; their members never land among the printer's own.
pub fn parse(body: &[u8]) -> Result<(u16, Attributes), Error> {
    let take = |at: usize, n: usize| body.get(at..at + n).ok_or(Error::Malformed);
    let status = u16::from_be_bytes(take(2, 2)?.try_into().unwrap());
    let mut at = 8;
    let mut group = 0u8;
    let mut depth = 0usize;
    let mut last: Option<String> = None;
    let mut attrs = Attributes::new();
    loop {
        let tag = *body.get(at).ok_or(Error::Malformed)?;
        at += 1;
        if tag == END {
            break;
        }
        if tag < 0x10 {
            group = tag;
            continue;
        }
        let name_len = usize::from(u16::from_be_bytes(take(at, 2)?.try_into().unwrap()));
        let name = String::from_utf8_lossy(take(at + 2, name_len)?).into_owned();
        at += 2 + name_len;
        let value_len = usize::from(u16::from_be_bytes(take(at, 2)?.try_into().unwrap()));
        let raw = take(at + 2, value_len)?;
        at += 2 + value_len;
        match tag {
            BEGIN_COLLECTION => {
                depth += 1;
                if depth == 1 && !name.is_empty() {
                    last = Some(name);
                }
                continue;
            }
            END_COLLECTION => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ if depth > 0 => continue,
            _ => {}
        }
        if group != PRINTER {
            continue;
        }
        let value = match tag {
            INTEGER | ENUM if raw.len() == 4 => IppValue::Int(i64::from(i32::from_be_bytes(raw.try_into().unwrap()))),
            BOOLEAN if raw.len() == 1 => IppValue::Bool(raw[0] != 0),
            0x41..=0x49 => IppValue::Text(String::from_utf8_lossy(raw).into_owned()),
            // textWithLanguage, nameWithLanguage: the language, then the text, each with its length.
            TEXT_WITH_LANGUAGE | NAME_WITH_LANGUAGE => with_language(raw).map_or(IppValue::Other, IppValue::Text),
            _ => IppValue::Other,
        };
        if !name.is_empty() {
            last = Some(name);
        }
        if let Some(name) = &last {
            attrs.entry(name.clone()).or_default().push(value);
        }
    }
    Ok((status, attrs))
}

fn with_language(raw: &[u8]) -> Option<String> {
    let lang = usize::from(u16::from_be_bytes(raw.get(..2)?.try_into().ok()?));
    let at = 2 + lang;
    let len = usize::from(u16::from_be_bytes(raw.get(at..at + 2)?.try_into().ok()?));
    Some(String::from_utf8_lossy(raw.get(at + 2..at + 2 + len)?).into_owned())
}

fn url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() }
}

/// The printer's attributes, from the first path that answers in IPP. `path`,
/// when known from an earlier check, is tried first.
pub async fn printer_attributes(host: &str, known: Option<&str>) -> Result<(String, Attributes), Error> {
    let http = reqwest::Client::builder()
        .timeout(TIMEOUT)
        // ipps on a LAN printer means its own self-signed certificate.
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| Error::Unreachable(e.to_string()))?;
    let host = url_host(host);
    let mut candidates: Vec<String> = known.map(|k| vec![k.to_string()]).unwrap_or_default();
    for path in PATHS {
        candidates.push(format!("http://{host}:631{path}"));
    }
    candidates.push(format!("https://{host}:443/ipp/print"));
    candidates.dedup();
    let mut last = Error::Unreachable(String::new());
    let mut no_631 = false;
    for (i, url) in candidates.iter().enumerate() {
        let on_631 = url.contains(":631/");
        // Nothing listens on 631: its other paths won't change that.
        if on_631 && no_631 {
            continue;
        }
        let uri = url.replacen("http://", "ipp://", 1).replacen("https://", "ipps://", 1);
        let sent = http
            .post(url)
            .header("Content-Type", "application/ipp")
            .body(request(&uri, i as u32 + 1))
            .send()
            .await;
        let response = match sent {
            Ok(r) => r,
            Err(e) => {
                no_631 |= on_631 && (e.is_connect() || e.is_timeout());
                last = Error::Unreachable(e.to_string());
                continue;
            }
        };
        if !response.status().is_success() {
            last = Error::Refused(format!("HTTP {}", response.status().as_u16()));
            continue;
        }
        let Ok(body) = response.bytes().await else { continue };
        match parse(&body) {
            Ok((status, attrs)) if status < 0x0100 && !attrs.is_empty() => return Ok((url.clone(), attrs)),
            Ok((status, _)) => last = Error::Refused(format!("IPP status 0x{status:04x}")),
            Err(e) => last = e,
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer as a printer sends it: the operation group, then the printer's.
    fn answer() -> Vec<u8> {
        let mut out = vec![0x02, 0x00, 0x00, 0x00, 0, 0, 0, 7, OPERATION];
        attribute(&mut out, CHARSET, "attributes-charset", b"utf-8");
        out.push(PRINTER);
        attribute(&mut out, ENUM, "printer-state", &5i32.to_be_bytes());
        attribute(&mut out, KEYWORD, "printer-state-reasons", b"media-jam-error");
        attribute(&mut out, KEYWORD, "", b"toner-low-report");
        attribute(&mut out, 0x41, "printer-make-and-model", b"Canon MF650C Series");
        attribute(&mut out, 0x42, "marker-names", b"Black Toner");
        attribute(&mut out, 0x42, "", b"Cyan Toner");
        attribute(&mut out, INTEGER, "marker-levels", &80i32.to_be_bytes());
        attribute(&mut out, INTEGER, "", &(-3i32).to_be_bytes());
        // A collection, whose members must not mix with the printer's own.
        attribute(&mut out, BEGIN_COLLECTION, "printer-input-tray-col", b"");
        attribute(&mut out, 0x4A, "", b"printer-state");
        attribute(&mut out, ENUM, "", &3i32.to_be_bytes());
        attribute(&mut out, END_COLLECTION, "", b"");
        attribute(&mut out, BOOLEAN, "printer-is-accepting-jobs", &[1]);
        // What a Brother set to Turkish says: "tr", then "Uykuda" (asleep).
        attribute(&mut out, TEXT_WITH_LANGUAGE, "printer-state-message", b"\x00\x02tr\x00\x06Uykuda");
        out.push(END);
        out
    }

    #[test]
    fn the_printer_group_is_read_with_its_lists() {
        let (status, attrs) = parse(&answer()).unwrap();
        assert_eq!(status, 0);
        assert_eq!(attrs["printer-state"], [IppValue::Int(5)]);
        assert_eq!(
            attrs["printer-state-reasons"],
            [IppValue::Text("media-jam-error".into()), IppValue::Text("toner-low-report".into())]
        );
        assert_eq!(attrs["marker-levels"], [IppValue::Int(80), IppValue::Int(-3)]);
        assert_eq!(attrs["printer-is-accepting-jobs"], [IppValue::Bool(true)]);
        assert_eq!(attrs["printer-state-message"], [IppValue::Text("Uykuda".into())]);
        assert!(!attrs.contains_key("attributes-charset"), "the operation group is not the printer's");
        assert!(!attrs.contains_key("printer-input-tray-col"));
    }

    #[test]
    fn a_cut_answer_is_malformed_not_a_panic() {
        let full = answer();
        for cut in [3, 9, 20, full.len() - 1] {
            assert_eq!(parse(&full[..cut]).unwrap_err(), Error::Malformed, "cut at {cut}");
        }
    }

    #[test]
    fn the_request_names_the_printer_and_what_it_wants() {
        let r = request("ipp://10.0.2.40:631/ipp/print", 3);
        assert_eq!(&r[..8], [0x02, 0x00, 0x00, 0x0B, 0, 0, 0, 3]);
        assert_eq!(*r.last().unwrap(), END);
        let text = String::from_utf8_lossy(&r);
        assert!(text.contains("ipp://10.0.2.40:631/ipp/print"));
        assert!(text.contains("requested-attributes"));
        assert!(text.contains("marker-levels"));
    }

    #[test]
    fn ipv6_hosts_get_brackets() {
        assert_eq!(url_host("fe80::1"), "[fe80::1]");
        assert_eq!(url_host("10.0.2.40"), "10.0.2.40");
        assert_eq!(url_host("printer.local"), "printer.local");
    }
}
