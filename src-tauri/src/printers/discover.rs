// Finding printers, only when the user clicks: the ones this computer already
// prints to (CUPS on Linux, the print spooler on Windows), the ones that
// announce themselves on the network (mDNS, Bonjour), and, on its own button,
// a sweep of a range of addresses asking each one over SNMP whether it's a
// printer. Nothing is added: the user picks.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde::Serialize;
use tokio::net::UdpSocket;

use super::ber::{self, Value};
use super::snmp::{self, Version};

/// A sweep covers this many addresses at most (a /22).
pub const MAX_SWEEP: usize = 1024;
const MDNS: (Ipv4Addr, u16) = (Ipv4Addr::new(224, 0, 0, 251), 5353);
const SERVICES: [&str; 4] = ["_ipp._tcp.local", "_ipps._tcp.local", "_printer._tcp.local", "_pdl-datastream._tcp.local"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub host: String,
    pub name: String,
    pub model: String,
    pub location: String,
    /// "system", "mdns", "snmp": where it was seen.
    pub sources: Vec<String>,
}

/// Puts what several ways found about one address together.
pub fn merge(lists: Vec<Vec<Found>>) -> Vec<Found> {
    let mut by_host: BTreeMap<String, Found> = BTreeMap::new();
    for f in lists.into_iter().flatten() {
        match by_host.get_mut(&f.host) {
            Some(known) => {
                for (mine, theirs) in [(&mut known.name, &f.name), (&mut known.model, &f.model), (&mut known.location, &f.location)] {
                    if mine.is_empty() {
                        mine.clone_from(theirs);
                    }
                }
                for s in f.sources {
                    if !known.sources.contains(&s) {
                        known.sources.push(s);
                    }
                }
            }
            None => {
                by_host.insert(f.host.clone(), f);
            }
        }
    }
    let mut out: Vec<Found> = by_host.into_values().collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.host.cmp(&b.host)));
    out
}

// ── This computer's printers ──────────────────────────────────────────────────

/// The host in a printer's device URI: ipp://, ipps://, http(s)://,
/// socket:// and lpd:// have one; usb://, dnssd://, implicitclass:// don't.
pub fn uri_host(uri: &str) -> Option<String> {
    let (scheme, rest) = uri.split_once("://")?;
    if !["ipp", "ipps", "http", "https", "socket", "lpd"].contains(&scheme) {
        return None;
    }
    let authority = rest.split(['/', '?']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next()?.to_string()
    } else {
        authority.split(':').next()?.to_string()
    };
    (!host.is_empty() && host != "localhost" && host != "127.0.0.1").then_some(host)
}

/// `lpstat -v`: "device for Canon_MF650C: ipp://10.0.2.40/ipp/print".
#[cfg_attr(windows, allow(dead_code))]
pub fn parse_lpstat(output: &str) -> Vec<Found> {
    output
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("device for ")?;
            let (name, uri) = rest.split_once(": ")?;
            Some(Found {
                host: uri_host(uri.trim())?,
                name: name.replace('_', " "),
                model: String::new(),
                location: String::new(),
                sources: vec!["system".into()],
            })
        })
        .collect()
}

/// What the PowerShell below prints: name, address and location, a tab between.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_windows(output: &str) -> Vec<Found> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.trim_end_matches('\r').split('\t');
            let name = parts.next()?.trim().to_string();
            let host = parts.next()?.trim().to_string();
            let location = parts.next().unwrap_or("").trim().to_string();
            (!host.is_empty() && !name.is_empty()).then(|| Found {
                host,
                name,
                model: String::new(),
                location,
                sources: vec!["system".into()],
            })
        })
        .collect()
}

pub async fn system() -> Vec<Found> {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(target_os = "linux")]
        {
            let mut cmd = std::process::Command::new("lpstat");
            cmd.arg("-v").env("LC_ALL", "C");
            match crate::platform::no_console(&mut cmd).output() {
                Ok(out) => parse_lpstat(&String::from_utf8_lossy(&out.stdout)),
                Err(_) => Vec::new(),
            }
        }
        #[cfg(windows)]
        {
            let script = "Get-Printer | ForEach-Object { $p = Get-PrinterPort -Name $_.PortName -ErrorAction SilentlyContinue; \
                          if ($p.PrinterHostAddress) { \"$($_.Name)`t$($p.PrinterHostAddress)`t$($_.Location)\" } }";
            let mut cmd = std::process::Command::new("powershell.exe");
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            match crate::platform::no_console(&mut cmd).output() {
                Ok(out) => parse_windows(&String::from_utf8_lossy(&out.stdout)),
                Err(_) => Vec::new(),
            }
        }
    })
    .await
    .unwrap_or_default()
}

// ── mDNS ──────────────────────────────────────────────────────────────────────

const PTR: u16 = 12;
const TXT: u16 = 16;
const SRV: u16 = 33;
const A: u16 = 1;

/// A one-shot query from an ordinary port: responders answer it straight back
/// (RFC 6762 §6.7), so nothing has to listen on 5353.
pub fn query(id: u16, names: &[&str], qtype: u16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&(names.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    for name in names {
        for label in name.trim_end_matches('.').split('.') {
            out.push(label.len() as u8);
            out.extend_from_slice(label.as_bytes());
        }
        out.push(0);
        out.extend_from_slice(&qtype.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Ptr(String),
    Srv { port: u16, target: String },
    Txt(Vec<String>),
    A(Ipv4Addr),
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub name: String,
    pub data: Data,
}

/// A name, following compression pointers (a few, never in a loop).
fn read_name(msg: &[u8], at: &mut usize) -> Option<String> {
    let mut labels: Vec<String> = Vec::new();
    let mut pos = *at;
    let mut jumped = false;
    for _ in 0..64 {
        let len = *msg.get(pos)? as usize;
        if len == 0 {
            if !jumped {
                *at = pos + 1;
            }
            return Some(labels.join("."));
        }
        if len & 0xC0 == 0xC0 {
            let target = ((len & 0x3F) << 8) | *msg.get(pos + 1)? as usize;
            if !jumped {
                *at = pos + 2;
            }
            jumped = true;
            pos = target;
            continue;
        }
        let label = msg.get(pos + 1..pos + 1 + len)?;
        labels.push(String::from_utf8_lossy(label).into_owned());
        pos += 1 + len;
    }
    None
}

pub fn parse_response(msg: &[u8]) -> Option<Vec<Record>> {
    let count = |i: usize| -> Option<usize> { Some(u16::from_be_bytes([*msg.get(i)?, *msg.get(i + 1)?]) as usize) };
    let (questions, records) = (count(4)?, count(6)? + count(8)? + count(10)?);
    let mut at = 12;
    for _ in 0..questions {
        read_name(msg, &mut at)?;
        at += 4;
    }
    let mut out = Vec::new();
    for _ in 0..records {
        let name = read_name(msg, &mut at)?;
        let rtype = u16::from_be_bytes([*msg.get(at)?, *msg.get(at + 1)?]);
        let len = u16::from_be_bytes([*msg.get(at + 8)?, *msg.get(at + 9)?]) as usize;
        let start = at + 10;
        let rdata = msg.get(start..start + len)?;
        let data = match rtype {
            PTR => {
                let mut p = start;
                Data::Ptr(read_name(msg, &mut p)?)
            }
            SRV if len >= 7 => {
                let mut p = start + 6;
                Data::Srv { port: u16::from_be_bytes([rdata[4], rdata[5]]), target: read_name(msg, &mut p)? }
            }
            TXT => {
                let mut strings = Vec::new();
                let mut p = 0;
                while p < rdata.len() {
                    let l = rdata[p] as usize;
                    strings.push(String::from_utf8_lossy(rdata.get(p + 1..p + 1 + l)?).into_owned());
                    p += 1 + l;
                }
                Data::Txt(strings)
            }
            A if len == 4 => Data::A(Ipv4Addr::new(rdata[0], rdata[1], rdata[2], rdata[3])),
            _ => Data::Other,
        };
        out.push(Record { name, data });
        at = start + len;
    }
    Some(out)
}

/// The printers among the records: each announced instance, its address
/// (from its SRV target's A record), and what its TXT says.
pub fn printers_from(records: &[Record]) -> Vec<Found> {
    let lower = |s: &str| s.to_lowercase();
    let mut instances: Vec<String> = Vec::new();
    for r in records {
        if let Data::Ptr(target) = &r.data {
            if SERVICES.iter().any(|s| lower(&r.name) == *s) && !instances.contains(target) {
                instances.push(target.clone());
            }
        }
    }
    let address = |host: &str| {
        records.iter().find_map(|r| match &r.data {
            Data::A(ip) if lower(&r.name) == lower(host) => Some(*ip),
            _ => None,
        })
    };
    let mut out = Vec::new();
    for instance in instances {
        let target = records.iter().find_map(|r| match &r.data {
            Data::Srv { target, .. } if r.name == instance => Some(target.clone()),
            _ => None,
        });
        let txt: Vec<String> = records
            .iter()
            .filter(|r| r.name == instance)
            .flat_map(|r| match &r.data {
                Data::Txt(s) => s.clone(),
                _ => Vec::new(),
            })
            .collect();
        let value = |key: &str| {
            txt.iter().find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                k.eq_ignore_ascii_case(key).then(|| v.trim().to_string())
            })
        };
        let Some(ip) = target.as_deref().and_then(address) else { continue };
        let name = instance.split("._").next().unwrap_or(&instance).to_string();
        out.push(Found {
            host: ip.to_string(),
            model: value("ty").or_else(|| value("product").map(|p| p.trim_matches(['(', ')']).to_string())).unwrap_or_default(),
            location: value("note").unwrap_or_default(),
            name,
            sources: vec!["mdns".into()],
        });
    }
    out
}

/// SRV targets that came without their address.
fn missing_addresses(records: &[Record]) -> Vec<String> {
    let mut out = Vec::new();
    for r in records {
        if let Data::Srv { target, .. } = &r.data {
            let known = records.iter().any(|a| matches!(a.data, Data::A(_)) && a.name.eq_ignore_ascii_case(target));
            if !known && !out.contains(target) {
                out.push(target.clone());
            }
        }
    }
    out
}

async fn ask(socket: &UdpSocket, message: &[u8], wait: Duration, records: &mut Vec<Record>) {
    if socket.send_to(message, MDNS).await.is_err() {
        return;
    }
    let deadline = tokio::time::Instant::now() + wait;
    let mut buf = vec![0u8; 9000];
    while let Ok(Ok((n, _))) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
        if let Some(found) = parse_response(&buf[..n]) {
            records.extend(found);
        }
    }
}

pub async fn mdns() -> Vec<Found> {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await else { return Vec::new() };
    let mut records = Vec::new();
    ask(&socket, &query(0x4349, &SERVICES, PTR), Duration::from_secs(3), &mut records).await;
    let missing = missing_addresses(&records);
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(String::as_str).take(32).collect();
        ask(&socket, &query(0x4350, &names, A), Duration::from_secs(1), &mut records).await;
    }
    printers_from(&records)
}

// ── SNMP sweep ────────────────────────────────────────────────────────────────

/// "10.0.2.0/24" (from /22 to /30), "10.0.2.10-10.0.2.60" or "10.0.2.10-60".
/// None when it isn't one, or covers more than MAX_SWEEP addresses.
pub fn parse_range(s: &str) -> Option<Vec<Ipv4Addr>> {
    let s = s.trim();
    if let Some((net, bits)) = s.split_once('/') {
        let bits: u32 = bits.trim().parse().ok()?;
        if !(22..=30).contains(&bits) {
            return None;
        }
        let base = u32::from(net.trim().parse::<Ipv4Addr>().ok()?) & (u32::MAX << (32 - bits));
        let size = 1u32 << (32 - bits);
        // Neither the network's own address nor its broadcast.
        return Some((1..size - 1).map(|i| Ipv4Addr::from(base + i)).collect());
    }
    let (from, to) = s.split_once('-')?;
    let from: Ipv4Addr = from.trim().parse().ok()?;
    let to: Ipv4Addr = match to.trim().parse::<Ipv4Addr>() {
        Ok(ip) => ip,
        Err(_) => {
            let last: u8 = to.trim().parse().ok()?;
            let o = from.octets();
            Ipv4Addr::new(o[0], o[1], o[2], last)
        }
    };
    let (a, b) = (u32::from(from), u32::from(to));
    if b < a || (b - a) as usize >= MAX_SWEEP {
        return None;
    }
    Some((a..=b).map(Ipv4Addr::from).collect())
}

/// This computer's /24, as a sweep's default.
pub fn default_range() -> Option<String> {
    match crate::alerts::lan_ip()? {
        std::net::IpAddr::V4(ip) => {
            let o = ip.octets();
            Some(format!("{}.{}.{}.0/24", o[0], o[1], o[2]))
        }
        std::net::IpAddr::V6(_) => None,
    }
}

const PRINTER_MIB: &str = "1.3.6.1.2.1.43";

/// What one address answered: a printer when the object after the Printer
/// MIB's root is in it. sysDescr, sysName and sysLocation say the rest.
pub fn sweep_answer(host: Ipv4Addr, varbinds: &[(Vec<u32>, Value)]) -> Option<Found> {
    let printer_root = ber::parse_oid(PRINTER_MIB);
    let is_printer = varbinds.get(3).is_some_and(|(oid, value)| oid.starts_with(&printer_root) && value.present());
    if !is_printer {
        return None;
    }
    let text = |i: usize| varbinds.get(i).map(|(_, v)| v.text()).unwrap_or_default();
    let descr = text(0).lines().next().unwrap_or("").trim().to_string();
    let name = text(1);
    Some(Found {
        host: host.to_string(),
        name: if name.is_empty() { descr.clone() } else { name },
        model: descr,
        location: text(2),
        sources: vec!["snmp".into()],
    })
}

/// Asks every address at once, a GETNEXT each, and keeps the printers.
/// v3 has no one-packet question, so a sweep speaks v2c (v1 if that's the setting).
pub async fn sweep(hosts: &[Ipv4Addr], version: Version, community: &str) -> Vec<Found> {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await else { return Vec::new() };
    let version = if version == Version::V1 { Version::V1 } else { Version::V2c };
    let oids: Vec<Vec<u32>> = ["1.3.6.1.2.1.1.1", "1.3.6.1.2.1.1.5", "1.3.6.1.2.1.1.6", PRINTER_MIB].iter().map(|o| ber::parse_oid(o)).collect();
    let mut found: BTreeMap<u32, Found> = BTreeMap::new();
    let mut answer = vec![0u8; 4096];
    for (i, host) in hosts.iter().enumerate() {
        let message = snmp::community_message(version, community, snmp::pdu(0xA1, i as i64 + 1, 0, 0, &oids));
        let _ = socket.send_to(&message, (*host, 161)).await;
        // Sixteen at a time, reading what came back meanwhile.
        if i % 16 == 15 {
            while let Ok(Ok((n, from))) = tokio::time::timeout(Duration::from_millis(5), socket.recv_from(&mut answer)).await {
                take_answer(&answer[..n], from, hosts, &mut found);
            }
        }
    }
    let deadline = tokio::time::Instant::now() + Duration::from_millis(2500);
    while let Ok(Ok((n, from))) = tokio::time::timeout_at(deadline, socket.recv_from(&mut answer)).await {
        take_answer(&answer[..n], from, hosts, &mut found);
    }
    found.into_values().collect()
}

/// An answer to the sweep: kept when it comes from the address it was asked of, and is a printer.
fn take_answer(bytes: &[u8], from: SocketAddr, hosts: &[Ipv4Addr], found: &mut BTreeMap<u32, Found>) {
    let Ok(pdu) = snmp::parse_community_message(bytes) else { return };
    let Some(host) = usize::try_from(pdu.request_id - 1).ok().and_then(|i| hosts.get(i)) else { return };
    if from.ip() != std::net::IpAddr::V4(*host) {
        return;
    }
    if let Some(f) = sweep_answer(*host, &pdu.varbinds) {
        found.insert(u32::from(*host), f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_uris_give_their_host() {
        assert_eq!(uri_host("ipp://10.0.2.40/ipp/print"), Some("10.0.2.40".into()));
        assert_eq!(uri_host("ipps://BRN30055C.local:443/ipp/print"), Some("BRN30055C.local".into()));
        assert_eq!(uri_host("socket://10.0.3.12:9100"), Some("10.0.3.12".into()));
        assert_eq!(uri_host("lpd://admin@10.0.3.13/queue"), Some("10.0.3.13".into()));
        assert_eq!(uri_host("ipp://[fe80::1]:631/ipp"), Some("fe80::1".into()));
        assert_eq!(uri_host("implicitclass://Canon_MF650C_Series/"), None);
        assert_eq!(uri_host("usb://Brother/HL-L6410DN?serial=1"), None);
        assert_eq!(uri_host("ipp://localhost:631/printers/pdf"), None);
    }

    #[test]
    fn lpstat_and_windows_lists_are_read() {
        let lpstat = "device for Canon_MF650C: ipp://10.0.2.40/ipp/print\n\
                      device for Brother_HL_L6410DN_series: implicitclass://Brother_HL_L6410DN_series/\n\
                      device for Kyocera: socket://10.0.3.12:9100\n";
        let found = parse_lpstat(lpstat);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].name.as_str(), found[0].host.as_str()), ("Canon MF650C", "10.0.2.40"));
        let windows = "HP 2F\t10.0.2.41\tSecond floor\r\nMicrosoft Print to PDF\t\t\r\n";
        let found = parse_windows(windows);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].host.as_str(), found[0].location.as_str()), ("10.0.2.41", "Second floor"));
    }

    /// An answer as a printer's mDNS responder sends it, names compressed.
    fn bonjour_answer() -> Vec<u8> {
        let mut m = vec![0x43, 0x49, 0x84, 0x00, 0, 0, 0, 1, 0, 0, 0, 3];
        // PTR _ipp._tcp.local → Canon MF650C._ipp._tcp.local
        let service_at = m.len();
        for l in ["_ipp", "_tcp", "local"] {
            m.push(l.len() as u8);
            m.extend_from_slice(l.as_bytes());
        }
        m.push(0);
        m.extend_from_slice(&[0, 12, 0, 1, 0, 0, 0, 120]);
        let instance_label = b"Canon MF650C";
        let rdata_len = 1 + instance_label.len() + 2;
        m.extend_from_slice(&(rdata_len as u16).to_be_bytes());
        let instance_at = m.len();
        m.push(instance_label.len() as u8);
        m.extend_from_slice(instance_label);
        m.extend_from_slice(&[0xC0, service_at as u8]);
        // SRV instance → CANON1.local:631
        m.extend_from_slice(&[0xC0, instance_at as u8, 0, 33, 0x80, 1, 0, 0, 0, 120]);
        let target_at_offset = m.len() + 2 + 6;
        let srv: Vec<u8> = [vec![0, 0, 0, 0, 2, 119], vec![6], b"CANON1".to_vec(), vec![0xC0, (service_at + 10) as u8]].concat();
        m.extend_from_slice(&(srv.len() as u16).to_be_bytes());
        m.extend_from_slice(&srv);
        // TXT instance: ty=, note=
        m.extend_from_slice(&[0xC0, instance_at as u8, 0, 16, 0x80, 1, 0, 0, 0, 120]);
        let txt: Vec<u8> = [b"\x13ty=Canon MF650C Ser".to_vec(), b"\x0anote=2F IT".to_vec()].concat();
        m.extend_from_slice(&(txt.len() as u16).to_be_bytes());
        m.extend_from_slice(&txt);
        // A CANON1.local → 10.0.2.40
        m.extend_from_slice(&[0xC0, target_at_offset as u8, 0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4, 10, 0, 2, 40]);
        m
    }

    #[test]
    fn a_bonjour_answer_gives_the_printer_its_address_and_place() {
        let records = parse_response(&bonjour_answer()).unwrap();
        assert_eq!(records[0], Record { name: "_ipp._tcp.local".into(), data: Data::Ptr("Canon MF650C._ipp._tcp.local".into()) });
        assert_eq!(records[1].data, Data::Srv { port: 631, target: "CANON1.local".into() });
        let found = printers_from(&records);
        assert_eq!(found, [Found {
            host: "10.0.2.40".into(),
            name: "Canon MF650C".into(),
            model: "Canon MF650C Ser".into(),
            location: "2F IT".into(),
            sources: vec!["mdns".into()],
        }]);
        assert!(missing_addresses(&records).is_empty());
        assert_eq!(parse_response(&bonjour_answer()[..40]), None, "cut short");
    }

    #[test]
    fn a_compression_loop_is_refused() {
        let mut m = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        m.extend_from_slice(&[0xC0, 12]);
        assert_eq!(parse_response(&m), None);
    }

    #[test]
    fn the_query_asks_each_service() {
        let q = query(7, &["_ipp._tcp.local"], PTR);
        assert_eq!(&q[..12], [0, 7, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[12..], b"\x04_ipp\x04_tcp\x05local\x00\x00\x0c\x00\x01");
    }

    #[test]
    fn ranges_in_the_ways_people_write_them() {
        let net = parse_range("10.0.2.0/24").unwrap();
        assert_eq!((net.len(), net[0], net[253]), (254, Ipv4Addr::new(10, 0, 2, 1), Ipv4Addr::new(10, 0, 2, 254)));
        assert_eq!(parse_range("10.0.2.77/24").unwrap()[0], Ipv4Addr::new(10, 0, 2, 1), "any address in it");
        assert_eq!(parse_range("10.0.0.0/22").unwrap().len(), 1022);
        assert_eq!(parse_range("10.0.0.0/16"), None, "too big");
        assert_eq!(parse_range("10.0.2.10-10.0.2.20").unwrap().len(), 11);
        assert_eq!(parse_range("10.0.2.10-20").unwrap().last(), Some(&Ipv4Addr::new(10, 0, 2, 20)));
        assert_eq!(parse_range("10.0.2.20-10"), None);
        assert_eq!(parse_range("printers"), None);
    }

    #[test]
    fn a_sweep_answer_is_a_printer_only_with_the_printer_mib() {
        let ip = Ipv4Addr::new(10, 0, 2, 40);
        let printer = vec![
            (ber::parse_oid("1.3.6.1.2.1.1.1.0"), Value::Octets(b"KYOCERA Document Solutions Printing System".to_vec())),
            (ber::parse_oid("1.3.6.1.2.1.1.5.0"), Value::Octets(b"KM3040".to_vec())),
            (ber::parse_oid("1.3.6.1.2.1.1.6.0"), Value::Octets(b"Reception".to_vec())),
            (ber::parse_oid("1.3.6.1.2.1.43.5.1.1.1.1"), Value::Int(1)),
        ];
        let f = sweep_answer(ip, &printer).unwrap();
        assert_eq!((f.name.as_str(), f.location.as_str()), ("KM3040", "Reception"));
        let mut switch = printer.clone();
        switch[3] = (ber::parse_oid("1.3.6.1.2.1.47.1.1.1.1.2.1"), Value::Octets(b"chassis".to_vec()));
        assert_eq!(sweep_answer(ip, &switch), None);
    }

    #[test]
    fn what_several_ways_found_is_put_together() {
        let a = Found { host: "10.0.2.40".into(), name: "Canon MF650C".into(), model: String::new(), location: String::new(), sources: vec!["system".into()] };
        let b = Found { host: "10.0.2.40".into(), name: "Canon".into(), model: "MF650C".into(), location: "2F".into(), sources: vec!["mdns".into()] };
        let c = Found { host: "10.0.2.12".into(), name: "brother".into(), model: String::new(), location: String::new(), sources: vec!["snmp".into()] };
        let merged = merge(vec![vec![a], vec![b, c]]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "brother");
        assert_eq!((merged[1].name.as_str(), merged[1].model.as_str(), merged[1].location.as_str()), ("Canon MF650C", "MF650C", "2F"));
        assert_eq!(merged[1].sources, ["system", "mdns"]);
    }
}
