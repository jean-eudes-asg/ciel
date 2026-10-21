// BER (X.690), as far as SNMP needs it: lengths, integers, octet strings,
// object identifiers, sequences, and SNMP's application types on the way back.

pub const INTEGER: u8 = 0x02;
pub const OCTET_STRING: u8 = 0x04;
pub const NULL: u8 = 0x05;
pub const OID: u8 = 0x06;
pub const SEQUENCE: u8 = 0x30;
const IP_ADDRESS: u8 = 0x40;
const COUNTER32: u8 = 0x41;
const GAUGE32: u8 = 0x42;
const TIMETICKS: u8 = 0x43;
const COUNTER64: u8 = 0x46;
const NO_SUCH_OBJECT: u8 = 0x80;
const NO_SUCH_INSTANCE: u8 = 0x81;
const END_OF_MIB_VIEW: u8 = 0x82;

/// Bytes that don't read as BER.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed;

pub type Result<T> = std::result::Result<T, Malformed>;

/// Tag, length and contents.
pub fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 4);
    out.push(tag);
    let len = content.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes = len.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        out.push(0x80 | (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
    out.extend_from_slice(content);
    out
}

/// The shortest two's complement.
pub fn int(v: i64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let mut start = 0;
    while start < 7 {
        let (a, b) = (bytes[start], bytes[start + 1]);
        if (a == 0 && b & 0x80 == 0) || (a == 0xFF && b & 0x80 != 0) {
            start += 1;
        } else {
            break;
        }
    }
    tlv(INTEGER, &bytes[start..])
}

pub fn octets(v: &[u8]) -> Vec<u8> {
    tlv(OCTET_STRING, v)
}

pub fn null() -> Vec<u8> {
    vec![NULL, 0]
}

pub fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
    tlv(SEQUENCE, &parts.concat())
}

pub fn oid(o: &[u32]) -> Vec<u8> {
    let mut content = Vec::new();
    if o.len() >= 2 {
        base128(&mut content, o[0] * 40 + o[1]);
    }
    for &n in o.iter().skip(2) {
        base128(&mut content, n);
    }
    tlv(OID, &content)
}

fn base128(out: &mut Vec<u8>, n: u32) {
    let mut digits = [0u8; 5];
    let mut i = digits.len();
    let mut v = n;
    loop {
        i -= 1;
        digits[i] = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            break;
        }
    }
    let last = digits.len() - 1;
    for d in &mut digits[i..last] {
        *d |= 0x80;
    }
    out.extend_from_slice(&digits[i..]);
}

/// "1.3.6.1.2.1.1.1.0" → [1, 3, 6, 1, 2, 1, 1, 1, 0].
pub fn parse_oid(s: &str) -> Vec<u32> {
    s.trim_start_matches('.').split('.').filter_map(|n| n.parse().ok()).collect()
}

pub fn decode_int(c: &[u8]) -> Result<i64> {
    if c.is_empty() || c.len() > 9 {
        return Err(Malformed);
    }
    let mut v: i128 = if c[0] & 0x80 != 0 { -1 } else { 0 };
    for b in c {
        v = (v << 8) | i128::from(*b);
    }
    i64::try_from(v).map_err(|_| Malformed)
}

fn decode_uint(c: &[u8]) -> Result<u64> {
    if c.is_empty() || c.len() > 9 {
        return Err(Malformed);
    }
    Ok(c.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b)))
}

pub fn decode_oid(c: &[u8]) -> Result<Vec<u32>> {
    let mut out = Vec::new();
    let mut v: u32 = 0;
    for (i, b) in c.iter().enumerate() {
        v = v.checked_mul(128).ok_or(Malformed)? | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            if out.is_empty() {
                let first = (v / 40).min(2);
                out.push(first);
                out.push(v - first * 40);
            } else {
                out.push(v);
            }
            v = 0;
        } else if i == c.len() - 1 {
            return Err(Malformed);
        }
    }
    Ok(out)
}

/// One element after another out of a buffer.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// The next element: its tag and its contents.
    pub fn next(&mut self) -> Result<(u8, &'a [u8])> {
        let tag = *self.data.get(self.pos).ok_or(Malformed)?;
        let mut p = self.pos + 1;
        let first = *self.data.get(p).ok_or(Malformed)?;
        p += 1;
        let len = if first & 0x80 == 0 {
            usize::from(first)
        } else {
            let n = usize::from(first & 0x7F);
            if n == 0 || n > 4 {
                return Err(Malformed);
            }
            let mut len = 0usize;
            for _ in 0..n {
                len = (len << 8) | usize::from(*self.data.get(p).ok_or(Malformed)?);
                p += 1;
            }
            len
        };
        let end = p.checked_add(len).filter(|e| *e <= self.data.len()).ok_or(Malformed)?;
        self.pos = end;
        Ok((tag, &self.data[p..end]))
    }

    pub fn expect(&mut self, tag: u8) -> Result<&'a [u8]> {
        match self.next()? {
            (t, content) if t == tag => Ok(content),
            _ => Err(Malformed),
        }
    }

    pub fn int(&mut self) -> Result<i64> {
        decode_int(self.expect(INTEGER)?)
    }

    pub fn octets(&mut self) -> Result<&'a [u8]> {
        self.expect(OCTET_STRING)
    }

    pub fn seq(&mut self) -> Result<Reader<'a>> {
        Ok(Reader::new(self.expect(SEQUENCE)?))
    }
}

/// A value an agent answers with.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Octets(Vec<u8>),
    Oid(Vec<u32>),
    Ip([u8; 4]),
    /// Counter32, Gauge32, TimeTicks and Counter64.
    Unsigned(u64),
    Null,
    /// noSuchObject or noSuchInstance: the agent doesn't have it.
    Missing,
    EndOfMib,
    Other,
}

impl Value {
    pub fn read(tag: u8, c: &[u8]) -> Result<Value> {
        Ok(match tag {
            INTEGER => Value::Int(decode_int(c)?),
            OCTET_STRING => Value::Octets(c.to_vec()),
            OID => Value::Oid(decode_oid(c)?),
            NULL => Value::Null,
            IP_ADDRESS if c.len() == 4 => Value::Ip([c[0], c[1], c[2], c[3]]),
            COUNTER32 | GAUGE32 | TIMETICKS | COUNTER64 => Value::Unsigned(decode_uint(c)?),
            NO_SUCH_OBJECT | NO_SUCH_INSTANCE => Value::Missing,
            END_OF_MIB_VIEW => Value::EndOfMib,
            _ => Value::Other,
        })
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Unsigned(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// Text, without the NULs and padding some printers leave in.
    pub fn text(&self) -> String {
        match self {
            Value::Octets(b) => {
                let s = String::from_utf8(b.clone()).unwrap_or_else(|_| b.iter().map(|&c| char::from(c)).collect());
                s.replace('\0', "").trim().to_string()
            }
            Value::Int(v) => v.to_string(),
            Value::Unsigned(v) => v.to_string(),
            _ => String::new(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Value::Octets(b) => b,
            _ => &[],
        }
    }

    /// Something there: not missing, not the end of the agent's tree.
    pub fn present(&self) -> bool {
        !matches!(self, Value::Missing | Value::EndOfMib | Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_take_their_shortest_form() {
        assert_eq!(int(0), [0x02, 0x01, 0x00]);
        assert_eq!(int(127), [0x02, 0x01, 0x7F]);
        assert_eq!(int(128), [0x02, 0x02, 0x00, 0x80]);
        assert_eq!(int(-1), [0x02, 0x01, 0xFF]);
        assert_eq!(int(-129), [0x02, 0x02, 0xFF, 0x7F]);
        assert_eq!(int(65507), [0x02, 0x03, 0x00, 0xFF, 0xE3]);
        for v in [0, 1, -1, 127, 128, 255, 256, -128, -129, 65535, 2_147_483_647, -2_147_483_648, i64::MAX, i64::MIN] {
            assert_eq!(Reader::new(&int(v)).int(), Ok(v), "{v}");
        }
    }

    #[test]
    fn long_lengths_use_more_bytes() {
        let content = vec![7u8; 300];
        let encoded = octets(&content);
        assert_eq!(&encoded[..4], [0x04, 0x82, 0x01, 0x2C]);
        assert_eq!(Reader::new(&encoded).octets().unwrap(), &content[..]);
    }

    #[test]
    fn oids_go_and_come_back() {
        let sys_descr = parse_oid("1.3.6.1.2.1.1.1.0");
        assert_eq!(oid(&sys_descr), [0x06, 0x08, 0x2B, 0x06, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00]);
        let big = parse_oid(".1.3.6.1.4.1.2435.2.3.9.4.2.1.5.5.8.0");
        let encoded = oid(&big);
        assert_eq!(decode_oid(&encoded[2..]), Ok(big));
        // 2435 → two digits: 0x93 0x03.
        assert!(encoded.windows(2).any(|w| w == [0x93, 0x03]));
        assert_eq!(decode_oid(&[0x2B, 0x86]), Err(Malformed), "cut in the middle of a number");
    }

    #[test]
    fn values_are_read_by_their_tag() {
        assert_eq!(Value::read(0x41, &[0x01, 0x00]), Ok(Value::Unsigned(256)));
        assert_eq!(Value::read(0x46, &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]), Ok(Value::Unsigned(u64::MAX)));
        assert_eq!(Value::read(0x40, &[10, 0, 2, 40]), Ok(Value::Ip([10, 0, 2, 40])));
        assert_eq!(Value::read(0x81, &[]), Ok(Value::Missing));
        assert_eq!(Value::read(0x82, &[]), Ok(Value::EndOfMib));
        assert_eq!(Value::Octets(b"HL-L6410DN\0\0 ".to_vec()).text(), "HL-L6410DN");
        assert_eq!(Value::Octets(vec![0xE9, b't', 0xE9]).text(), "été", "Latin-1 when it isn't UTF-8");
    }

    #[test]
    fn a_reader_refuses_what_runs_past_the_end() {
        assert_eq!(Reader::new(&[0x04, 0x05, 1, 2]).next(), Err(Malformed));
        assert_eq!(Reader::new(&[0x04, 0x85, 0, 0, 0, 0, 1]).next(), Err(Malformed));
        assert_eq!(Reader::new(&[0x04]).next(), Err(Malformed));
        let mut r = Reader::new(&[0x02, 0x01, 0x05, 0x04, 0x00]);
        assert_eq!(r.int(), Ok(5));
        assert_eq!(r.octets(), Ok(&[][..]));
        assert!(r.is_empty());
    }
}
