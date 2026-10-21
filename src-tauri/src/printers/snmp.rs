// An SNMP manager, as much as the Printers pill needs: GET, GETNEXT and
// GETBULK over UDP, in v1 and v2c (a community) or v3 (a user, with
// authentication and AES privacy, printers/usm.rs). Each request is tried twice
// before the printer counts as not answering.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;

use super::ber::{self, Malformed, Reader, Value};
use super::usm::{self, Auth};

const GET: u8 = 0xA0;
const GET_NEXT: u8 = 0xA1;
const RESPONSE: u8 = 0xA2;
const GET_BULK: u8 = 0xA5;
const REPORT: u8 = 0xA8;

/// One try waits this long for its answer.
const WAIT: Duration = Duration::from_millis(1500);
const TRIES: usize = 2;
/// A walk stops after this many values: a printer's tables are far smaller.
const MAX_WALK: usize = 400;
const BULK: i64 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    V1,
    V2c,
    V3,
}

impl Version {
    pub fn parse(s: &str) -> Version {
        match s {
            "v1" => Version::V1,
            "v3" => Version::V3,
            _ => Version::V2c,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Credentials {
    pub version: Version,
    /// v1 and v2c.
    pub community: String,
    /// v3.
    pub user: String,
    pub auth: Option<Auth>,
    pub auth_password: String,
    /// AES-128 when set (it needs `auth` too).
    pub privacy: bool,
    pub priv_password: String,
}

impl Credentials {
    #[cfg(test)]
    pub fn community(version: Version, community: &str) -> Credentials {
        Credentials {
            version,
            community: community.to_string(),
            user: String::new(),
            auth: None,
            auth_password: String::new(),
            privacy: false,
            priv_password: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// No answer at all: off, unreachable, SNMP off, or a community it refuses in silence.
    Timeout,
    /// v3: the user or a password is wrong, or the security level isn't allowed.
    Refused(&'static str),
    /// The agent said no to the request itself.
    Agent(i64),
    Malformed,
    Io(String),
}

impl From<Malformed> for Error {
    fn from(_: Malformed) -> Error {
        Error::Malformed
    }
}

pub type VarBinds = Vec<(Vec<u32>, Value)>;

/// What came back for one request.
#[derive(Debug, PartialEq)]
pub struct Pdu {
    pub tag: u8,
    pub request_id: i64,
    pub error_status: i64,
    pub varbinds: VarBinds,
}

// ── Messages ──────────────────────────────────────────────────────────────────

pub fn pdu(tag: u8, request_id: i64, a: i64, b: i64, oids: &[Vec<u32>]) -> Vec<u8> {
    let varbinds: Vec<Vec<u8>> = oids.iter().map(|o| ber::seq(&[ber::oid(o), ber::null()])).collect();
    ber::tlv(tag, &[ber::int(request_id), ber::int(a), ber::int(b), ber::seq(&varbinds)].concat())
}

/// A v1 or v2c message: the version, the community, the PDU.
pub fn community_message(version: Version, community: &str, pdu: Vec<u8>) -> Vec<u8> {
    let v = if version == Version::V1 { 0 } else { 1 };
    ber::seq(&[ber::int(v), ber::octets(community.as_bytes()), pdu])
}

pub fn parse_pdu(tag: u8, content: &[u8]) -> Result<Pdu, Error> {
    let mut r = Reader::new(content);
    let request_id = r.int()?;
    let error_status = r.int()?;
    let _error_index = r.int()?;
    let mut list = r.seq()?;
    let mut varbinds = Vec::new();
    while !list.is_empty() {
        let mut vb = list.seq()?;
        let oid = ber::decode_oid(vb.expect(ber::OID)?)?;
        let (vtag, vcontent) = vb.next()?;
        varbinds.push((oid, Value::read(vtag, vcontent)?));
    }
    Ok(Pdu { tag, request_id, error_status, varbinds })
}

pub fn parse_community_message(bytes: &[u8]) -> Result<Pdu, Error> {
    let mut msg = Reader::new(bytes).seq()?;
    let _version = msg.int()?;
    let _community = msg.octets()?;
    let (tag, content) = msg.next()?;
    parse_pdu(tag, content)
}

// ── v3 ────────────────────────────────────────────────────────────────────────

/// The printer's SNMP engine, learnt by asking with nothing (RFC 3414 §4).
#[derive(Debug, Clone)]
struct Engine {
    id: Vec<u8>,
    boots: u32,
    time: u32,
    learnt: Instant,
    auth_key: Vec<u8>,
    priv_key: Vec<u8>,
}

impl Engine {
    fn time_now(&self) -> u32 {
        self.time.saturating_add(self.learnt.elapsed().as_secs() as u32)
    }
}

/// The v3 message, and where its MAC goes (to be filled once it's all built).
fn v3_message(
    msg_id: i64,
    flags: u8,
    usm: (&[u8], u32, u32, &[u8]),
    mac_len: usize,
    priv_params: &[u8],
    data: Vec<u8>,
) -> (Vec<u8>, usize) {
    let (engine, boots, time, user) = usm;
    let before = [ber::octets(engine), ber::int(boots.into()), ber::int(time.into()), ber::octets(user)].concat();
    let usm_content = [before.clone(), ber::octets(&vec![0; mac_len]), ber::octets(priv_params)].concat();
    let usm_seq = ber::tlv(ber::SEQUENCE, &usm_content);
    let usm_octets = ber::octets(&usm_seq);
    let global = ber::seq(&[ber::int(msg_id), ber::int(65507), ber::octets(&[flags]), ber::int(3)]);
    let prefix = [ber::int(3), global].concat();
    let content = [prefix.clone(), usm_octets.clone(), data].concat();
    let whole = ber::tlv(ber::SEQUENCE, &content);
    let mac_at = (whole.len() - content.len())
        + prefix.len()
        + (usm_octets.len() - usm_seq.len())
        + (usm_seq.len() - usm_content.len())
        + before.len()
        + 2;
    (whole, mac_at)
}

/// What a v3 answer carries, before its PDU is read.
struct V3Answer<'a> {
    msg_id: i64,
    flags: u8,
    engine: &'a [u8],
    boots: u32,
    time: u32,
    mac: &'a [u8],
    salt: &'a [u8],
    /// The scoped PDU, or its cipher text when `flags` says private.
    data: (u8, &'a [u8]),
}

fn parse_v3(bytes: &[u8]) -> Result<V3Answer<'_>, Error> {
    let mut msg = Reader::new(bytes).seq()?;
    if msg.int()? != 3 {
        return Err(Error::Malformed);
    }
    let mut global = msg.seq()?;
    let msg_id = global.int()?;
    let _max = global.int()?;
    let flags = *global.octets()?.first().ok_or(Malformed)?;
    let mut usm = Reader::new(msg.octets()?).seq()?;
    let engine = usm.octets()?;
    let boots = u32::try_from(usm.int()?).unwrap_or(0);
    let time = u32::try_from(usm.int()?).unwrap_or(0);
    let _user = usm.octets()?;
    let mac = usm.octets()?;
    let salt = usm.octets()?;
    let data = msg.next()?;
    Ok(V3Answer { msg_id, flags, engine, boots, time, mac, salt, data })
}

/// The scoped PDU's own PDU.
fn scoped_pdu(scoped: &[u8]) -> Result<Pdu, Error> {
    let mut s = Reader::new(scoped).seq()?;
    let _context_engine = s.octets()?;
    let _context_name = s.octets()?;
    let (tag, content) = s.next()?;
    parse_pdu(tag, content)
}

/// usmStats counters in a Report: why the agent turned the request down.
fn report_reason(pdu: &Pdu) -> Option<&'static str> {
    let oid = &pdu.varbinds.first()?.0;
    let stats = [1, 3, 6, 1, 6, 3, 15, 1, 1];
    if !oid.starts_with(&stats) {
        return None;
    }
    Some(match oid.get(9) {
        Some(1) => "unsupported security level",
        Some(2) => "not in time window",
        Some(3) => "unknown user",
        Some(4) => "unknown engine",
        Some(5) => "wrong password",
        Some(6) => "decryption error",
        _ => "refused",
    })
}

// ── The client ────────────────────────────────────────────────────────────────

pub struct Client {
    socket: UdpSocket,
    creds: Credentials,
    engine: Option<Engine>,
    next_id: i64,
    /// Some agents (Brother's print servers) never answer GETBULK: GETNEXT then.
    bulk: bool,
}

impl Client {
    pub async fn connect(addr: SocketAddr, creds: Credentials) -> Result<Client, Error> {
        let local: SocketAddr = if addr.is_ipv4() { "0.0.0.0:0".parse().unwrap() } else { "[::]:0".parse().unwrap() };
        let socket = UdpSocket::bind(local).await.map_err(|e| Error::Io(e.to_string()))?;
        socket.connect(addr).await.map_err(|e| Error::Io(e.to_string()))?;
        // Request ids start somewhere else each time, so a late answer to an
        // earlier client is never taken for this one's.
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
        let bulk = creds.version != Version::V1;
        Ok(Client { socket, creds, engine: None, next_id: i64::from(seed % 1_000_000_000) + 1, bulk })
    }

    fn request_id(&mut self) -> i64 {
        self.next_id = if self.next_id >= i64::from(i32::MAX) { 1 } else { self.next_id + 1 };
        self.next_id
    }

    /// Whether walks use GETBULK: known from an earlier check, or found out.
    pub fn set_bulk(&mut self, on: bool) {
        self.bulk = on && self.creds.version != Version::V1;
    }

    pub fn bulk(&self) -> bool {
        self.bulk
    }

    pub async fn get(&mut self, oids: &[Vec<u32>]) -> Result<VarBinds, Error> {
        Ok(self.request(GET, 0, 0, oids).await?.varbinds)
    }

    /// Everything under `root`, in order. GETBULK where the version has it.
    pub async fn walk(&mut self, root: &[u32]) -> Result<VarBinds, Error> {
        let mut out: VarBinds = Vec::new();
        let mut cursor = root.to_vec();
        while out.len() < MAX_WALK {
            let pdu = if self.bulk {
                match self.request(GET_BULK, 0, BULK, &[cursor.clone()]).await {
                    Err(Error::Timeout) => {
                        self.bulk = false;
                        continue;
                    }
                    other => other?,
                }
            } else {
                match self.request(GET_NEXT, 0, 0, &[cursor.clone()]).await {
                    // v1 says "no such name" at the end of the agent's tree.
                    Err(Error::Agent(2)) => break,
                    other => other?,
                }
            };
            if pdu.varbinds.is_empty() {
                break;
            }
            let mut done = false;
            for (oid, value) in pdu.varbinds {
                if !oid.starts_with(root) || matches!(value, Value::EndOfMib) || oid <= cursor {
                    done = true;
                    break;
                }
                cursor = oid.clone();
                out.push((oid, value));
            }
            if done {
                break;
            }
        }
        Ok(out)
    }

    async fn request(&mut self, tag: u8, a: i64, b: i64, oids: &[Vec<u32>]) -> Result<Pdu, Error> {
        if self.creds.version != Version::V3 {
            let id = self.request_id();
            let message = community_message(self.creds.version, &self.creds.community, pdu(tag, id, a, b, oids));
            let answer = self.exchange(&message, |bytes| {
                parse_community_message(bytes).ok().filter(|p| p.request_id == id && p.tag == RESPONSE)
            })
            .await?;
            return match answer.error_status {
                0 => Ok(answer),
                e => Err(Error::Agent(e)),
            };
        }
        if self.engine.is_none() {
            self.discover().await?;
        }
        match self.v3_request(tag, a, b, oids).await {
            // Its clock moved (a reboot, a long pause): the Report told us the new one.
            Err(Error::Refused("not in time window")) => self.v3_request(tag, a, b, oids).await,
            other => other,
        }
    }

    /// Sends `message` and waits for an answer `accept` takes, twice at most.
    async fn exchange<T>(&self, message: &[u8], accept: impl Fn(&[u8]) -> Option<T>) -> Result<T, Error> {
        let mut buf = vec![0u8; 65_535];
        for _ in 0..TRIES {
            self.socket.send(message).await.map_err(|e| Error::Io(e.to_string()))?;
            let deadline = tokio::time::Instant::now() + WAIT;
            loop {
                match tokio::time::timeout_at(deadline, self.socket.recv(&mut buf)).await {
                    Err(_) => break,
                    // An ICMP "port unreachable" comes back as an error: nothing listens.
                    Ok(Err(_)) => return Err(Error::Timeout),
                    Ok(Ok(n)) => {
                        if let Some(answer) = accept(&buf[..n]) {
                            return Ok(answer);
                        }
                    }
                }
            }
        }
        Err(Error::Timeout)
    }

    /// Learns the engine's id, boots and time, then the keys localized to it.
    async fn discover(&mut self) -> Result<(), Error> {
        let msg_id = self.request_id();
        let scoped = ber::seq(&[ber::octets(b""), ber::octets(b""), pdu(GET, msg_id, 0, 0, &[])]);
        let (message, _) = v3_message(msg_id, 0x04, (b"", 0, 0, b""), 0, b"", scoped);
        let (id, boots, time) = self
            .exchange(&message, |bytes| {
                let a = parse_v3(bytes).ok()?;
                (a.msg_id == msg_id && !a.engine.is_empty()).then(|| (a.engine.to_vec(), a.boots, a.time))
            })
            .await?;
        let (auth_key, priv_key) = match self.creds.auth {
            Some(auth) => {
                if self.creds.auth_password.len() < 8 {
                    return Err(Error::Refused("password too short"));
                }
                let auth_key = auth.localize(&auth.password_to_key(self.creds.auth_password.as_bytes()), &id);
                let priv_key = if self.creds.privacy {
                    if self.creds.priv_password.len() < 8 {
                        return Err(Error::Refused("password too short"));
                    }
                    auth.localize(&auth.password_to_key(self.creds.priv_password.as_bytes()), &id)
                } else {
                    Vec::new()
                };
                (auth_key, priv_key)
            }
            None => (Vec::new(), Vec::new()),
        };
        self.engine = Some(Engine { id, boots, time, learnt: Instant::now(), auth_key, priv_key });
        Ok(())
    }

    async fn v3_request(&mut self, tag: u8, a: i64, b: i64, oids: &[Vec<u32>]) -> Result<Pdu, Error> {
        let engine = self.engine.clone().ok_or(Error::Timeout)?;
        let auth = self.creds.auth;
        let private = auth.is_some() && self.creds.privacy;
        let flags = 0x04 | if auth.is_some() { 0x01 } else { 0 } | if private { 0x02 } else { 0 };
        let msg_id = self.request_id();
        let time = engine.time_now();
        let scoped = ber::seq(&[ber::octets(&engine.id), ber::octets(b""), pdu(tag, msg_id, a, b, oids)]);
        let salt: [u8; 8] = (msg_id as u64 ^ (u64::from(time) << 32)).to_be_bytes();
        let (data, priv_params) = if private {
            (ber::octets(&usm::aes_cfb(&engine.priv_key, engine.boots, time, &salt, &scoped, true)), salt.to_vec())
        } else {
            (scoped, Vec::new())
        };
        let mac_len = auth.map_or(0, Auth::mac_len);
        let user = self.creds.user.clone();
        let (mut message, mac_at) =
            v3_message(msg_id, flags, (&engine.id, engine.boots, time, user.as_bytes()), mac_len, &priv_params, data);
        if let Some(auth) = auth {
            let mac = auth.mac(&engine.auth_key, &message);
            message[mac_at..mac_at + mac_len].copy_from_slice(&mac);
        }

        let answer = self
            .exchange(&message, |bytes| {
                let a = parse_v3(bytes).ok()?;
                if a.msg_id != msg_id {
                    return None;
                }
                // An answer that says it is authenticated must be.
                if a.flags & 0x01 != 0 {
                    let auth = auth?;
                    let at = a.mac.as_ptr() as usize - bytes.as_ptr() as usize;
                    let mut zeroed = bytes.to_vec();
                    zeroed[at..at + a.mac.len()].fill(0);
                    if auth.mac(&engine.auth_key, &zeroed) != a.mac {
                        return None;
                    }
                }
                let scoped = if a.flags & 0x02 != 0 {
                    let salt: [u8; 8] = a.salt.try_into().ok()?;
                    let (tag, sealed) = a.data;
                    (tag == ber::OCTET_STRING).then_some(())?;
                    usm::aes_cfb(&engine.priv_key, a.boots, a.time, &salt, sealed, false)
                } else {
                    ber::tlv(a.data.0, a.data.1)
                };
                let pdu = scoped_pdu(&scoped).ok()?;
                Some((pdu, a.engine.to_vec(), a.boots, a.time))
            })
            .await?;
        let (pdu, id, boots, time) = answer;
        if pdu.tag == REPORT {
            let reason = report_reason(&pdu).unwrap_or("refused");
            if reason == "not in time window" || reason == "unknown engine" {
                if let Some(e) = self.engine.as_mut() {
                    if !id.is_empty() {
                        e.id = id;
                    }
                    e.boots = boots;
                    e.time = time;
                    e.learnt = Instant::now();
                }
                return Err(Error::Refused("not in time window"));
            }
            return Err(Error::Refused(reason));
        }
        match pdu.error_status {
            0 => Ok(pdu),
            e => Err(Error::Agent(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn a_v2c_get_is_built_byte_for_byte() {
        // What net-snmp's `snmpget -v2c -c public host 1.3.6.1.2.1.1.1.0` sends, request id 1.
        let message = community_message(Version::V2c, "public", pdu(GET, 1, 0, 0, &[ber::parse_oid("1.3.6.1.2.1.1.1.0")]));
        assert_eq!(message, hex("302602010104067075626c6963a019020101020100020100300e300c06082b060102010101000500"));
    }

    #[test]
    fn an_answer_is_read_with_its_values() {
        // A printer answering sysDescr.0 and hrPrinterStatus.1 (idle).
        let answer = ber::seq(&[
            ber::int(1),
            ber::octets(b"public"),
            ber::tlv(
                RESPONSE,
                &[
                    ber::int(77),
                    ber::int(0),
                    ber::int(0),
                    ber::seq(&[
                        ber::seq(&[ber::oid(&ber::parse_oid("1.3.6.1.2.1.1.1.0")), ber::octets(b"Brother NC-8300w")]),
                        ber::seq(&[ber::oid(&ber::parse_oid("1.3.6.1.2.1.25.3.5.1.1.1")), ber::int(3)]),
                        ber::seq(&[ber::oid(&ber::parse_oid("1.3.6.1.2.1.43.5.1.1.17.1")), vec![0x81, 0x00]]),
                    ]),
                ]
                .concat(),
            ),
        ]);
        let pdu = parse_community_message(&answer).unwrap();
        assert_eq!((pdu.tag, pdu.request_id, pdu.error_status), (RESPONSE, 77, 0));
        assert_eq!(pdu.varbinds[0].1.text(), "Brother NC-8300w");
        assert_eq!(pdu.varbinds[1].1, Value::Int(3));
        assert_eq!(pdu.varbinds[2].1, Value::Missing);
        assert!(parse_community_message(&answer[..answer.len() - 3]).is_err());
    }

    #[test]
    fn a_v3_message_leaves_room_for_its_mac_exactly_there() {
        let (message, at) = v3_message(9, 0x05, (b"\x80\x00\x1f\x88\x04engine", 4, 1200, b"ciel"), 12, b"", ber::seq(&[]));
        let a = parse_v3(&message).unwrap();
        assert_eq!(a.mac, &[0u8; 12]);
        assert_eq!(a.mac.as_ptr() as usize - message.as_ptr() as usize, at);
        assert_eq!((a.msg_id, a.flags, a.boots, a.time), (9, 0x05, 4, 1200));
        assert_eq!(a.engine, b"\x80\x00\x1f\x88\x04engine");
    }

    #[test]
    fn reports_say_why_v3_was_refused() {
        let report = |last: u32| Pdu {
            tag: REPORT,
            request_id: 1,
            error_status: 0,
            varbinds: vec![(vec![1, 3, 6, 1, 6, 3, 15, 1, 1, last, 0], Value::Unsigned(1))],
        };
        assert_eq!(report_reason(&report(3)), Some("unknown user"));
        assert_eq!(report_reason(&report(5)), Some("wrong password"));
        assert_eq!(report_reason(&report(2)), Some("not in time window"));
    }

    /// A fake agent on 127.0.0.1 that answers v2c GETBULK from a table, to walk it.
    #[tokio::test]
    async fn a_walk_stops_at_the_end_of_its_subtree() {
        let agent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = agent.local_addr().unwrap();
        let table: Vec<(Vec<u32>, i64)> = vec![
            (ber::parse_oid("1.3.6.1.2.1.43.11.1.1.9.1.1"), 80),
            (ber::parse_oid("1.3.6.1.2.1.43.11.1.1.9.1.2"), 15),
            (ber::parse_oid("1.3.6.1.2.1.43.11.1.1.9.1.3"), 4),
            (ber::parse_oid("1.3.6.1.2.1.43.12.1.1.4.1.1"), 0),
        ];
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((n, from)) = agent.recv_from(&mut buf).await else { return };
                let mut msg = Reader::new(&buf[..n]).seq().unwrap();
                msg.int().unwrap();
                msg.octets().unwrap();
                let (_, content) = msg.next().unwrap();
                let request = parse_pdu(GET_BULK, content).unwrap();
                let cursor = request.varbinds[0].0.clone();
                // Two at a time, to make the walk ask again.
                let rows: Vec<Vec<u8>> = table
                    .iter()
                    .filter(|(o, _)| *o > cursor)
                    .take(2)
                    .map(|(o, v)| ber::seq(&[ber::oid(o), ber::int(*v)]))
                    .collect();
                let rows = if rows.is_empty() {
                    vec![ber::seq(&[ber::oid(&cursor), vec![0x82, 0x00]])]
                } else {
                    rows
                };
                let answer = ber::seq(&[
                    ber::int(1),
                    ber::octets(b"public"),
                    ber::tlv(RESPONSE, &[ber::int(request.request_id), ber::int(0), ber::int(0), ber::seq(&rows)].concat()),
                ]);
                agent.send_to(&answer, from).await.unwrap();
            }
        });
        let mut client = Client::connect(addr, Credentials::community(Version::V2c, "public")).await.unwrap();
        let rows = client.walk(&ber::parse_oid("1.3.6.1.2.1.43.11.1.1.9")).await.unwrap();
        let levels: Vec<i64> = rows.iter().filter_map(|(_, v)| v.as_i64()).collect();
        assert_eq!(levels, [80, 15, 4]);
    }

    #[tokio::test]
    async fn nobody_listening_is_a_timeout_not_a_hang() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = socket.local_addr().unwrap();
        drop(socket);
        let mut client = Client::connect(addr, Credentials::community(Version::V2c, "public")).await.unwrap();
        let started = Instant::now();
        assert_eq!(client.get(&[ber::parse_oid("1.3.6.1.2.1.1.1.0")]).await, Err(Error::Timeout));
        assert!(started.elapsed() < Duration::from_secs(4));
    }
}
