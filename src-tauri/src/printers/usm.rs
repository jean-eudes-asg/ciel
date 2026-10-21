// SNMPv3's User-based Security Model (RFC 3414, RFC 3826, RFC 7860), the parts
// a manager needs: a password turned into a key and localized to the printer's
// engine, the message's HMAC, and AES-128 in CFB mode for privacy.
//
// SHA-1 and SHA-256 come from ring; MD5, which ring doesn't have and many
// printers still default to, is RFC 1321 written out below.

use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
use ring::{digest, hmac};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    Md5,
    Sha1,
    Sha256,
}

impl Auth {
    /// "md5", "sha" or "sha1", "sha256"; anything else is no authentication.
    pub fn parse(s: &str) -> Option<Auth> {
        match s.to_ascii_lowercase().as_str() {
            "md5" => Some(Auth::Md5),
            "sha" | "sha1" | "sha-1" => Some(Auth::Sha1),
            "sha256" | "sha-256" => Some(Auth::Sha256),
            _ => None,
        }
    }

    /// How much of the HMAC goes in the message: 96 bits, or 192 for SHA-256.
    pub fn mac_len(self) -> usize {
        match self {
            Auth::Md5 | Auth::Sha1 => 12,
            Auth::Sha256 => 24,
        }
    }

    fn hash(self, parts: &[&[u8]]) -> Vec<u8> {
        match self {
            Auth::Md5 => {
                let mut m = Md5::new();
                for p in parts {
                    m.update(p);
                }
                m.finish().to_vec()
            }
            Auth::Sha1 | Auth::Sha256 => {
                let alg = if self == Auth::Sha1 { &digest::SHA1_FOR_LEGACY_USE_ONLY } else { &digest::SHA256 };
                let mut c = digest::Context::new(alg);
                for p in parts {
                    c.update(p);
                }
                c.finish().as_ref().to_vec()
            }
        }
    }

    /// RFC 3414 A.2: the password repeated over a megabyte, hashed.
    pub fn password_to_key(self, password: &[u8]) -> Vec<u8> {
        const TOTAL: usize = 1_048_576;
        let mut block = [0u8; 64];
        let mut chunks: Vec<[u8; 64]> = Vec::with_capacity(TOTAL / 64);
        let mut index = 0;
        for _ in 0..TOTAL / 64 {
            for b in &mut block {
                *b = password[index % password.len()];
                index += 1;
            }
            chunks.push(block);
        }
        let parts: Vec<&[u8]> = chunks.iter().map(|c| &c[..]).collect();
        self.hash(&parts)
    }

    /// The key for one engine: H(key ‖ engineID ‖ key).
    pub fn localize(self, key: &[u8], engine_id: &[u8]) -> Vec<u8> {
        self.hash(&[key, engine_id, key])
    }

    /// The truncated HMAC that goes in msgAuthenticationParameters.
    pub fn mac(self, key: &[u8], message: &[u8]) -> Vec<u8> {
        let full = match self {
            Auth::Md5 => hmac_md5(key, message).to_vec(),
            Auth::Sha1 => hmac::sign(&hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key), message).as_ref().to_vec(),
            Auth::Sha256 => hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), message).as_ref().to_vec(),
        };
        full[..self.mac_len()].to_vec()
    }
}

/// RFC 3826: AES-128 in CFB mode, the IV made of the engine's boots and time and an 8-byte salt.
pub fn aes_cfb(key: &[u8], boots: u32, time: u32, salt: &[u8; 8], data: &[u8], encrypt: bool) -> Vec<u8> {
    let mut iv = [0u8; 16];
    iv[..4].copy_from_slice(&boots.to_be_bytes());
    iv[4..8].copy_from_slice(&time.to_be_bytes());
    iv[8..].copy_from_slice(salt);
    cfb128(&key[..16], iv, data, encrypt)
}

fn cfb128(key: &[u8], iv: [u8; 16], data: &[u8], encrypt: bool) -> Vec<u8> {
    let cipher = aes::Aes128::new(GenericArray::from_slice(key));
    let mut feedback = iv;
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut stream = GenericArray::clone_from_slice(&feedback);
        cipher.encrypt_block(&mut stream);
        let start = out.len();
        out.extend(chunk.iter().zip(stream.iter()).map(|(d, s)| d ^ s));
        // The cipher text is what feeds the next block, whichever way we go.
        let fed = if encrypt { &out[start..] } else { chunk };
        feedback[..fed.len()].copy_from_slice(fed);
    }
    out
}

// ── MD5 (RFC 1321) ────────────────────────────────────────────────────────────

struct Md5 {
    state: [u32; 4],
    buffer: Vec<u8>,
    length: u64,
}

const S: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
    21,
];

/// floor(|sin(i + 1)| × 2³²).
fn k(i: usize) -> u32 {
    ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32
}

impl Md5 {
    fn new() -> Md5 {
        Md5 { state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476], buffer: Vec::with_capacity(64), length: 0 }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.length += data.len() as u64;
        if !self.buffer.is_empty() {
            let take = (64 - self.buffer.len()).min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buffer.len() == 64 {
                let block: [u8; 64] = self.buffer[..].try_into().unwrap();
                self.block(&block);
                self.buffer.clear();
            }
        }
        let mut chunks = data.chunks_exact(64);
        for block in &mut chunks {
            self.block(block.try_into().unwrap());
        }
        self.buffer.extend_from_slice(chunks.remainder());
    }

    fn finish(mut self) -> [u8; 16] {
        let bits = self.length.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        let used = (self.length % 64) as usize;
        let zeros = if used < 56 { 55 - used } else { 119 - used };
        pad.extend(std::iter::repeat_n(0u8, zeros));
        pad.extend_from_slice(&bits.to_le_bytes());
        let length = self.length;
        self.update(&pad);
        self.length = length;
        let mut out = [0u8; 16];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        out
    }

    fn block(&mut self, block: &[u8; 64]) {
        let m: Vec<u32> = block.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect();
        let [mut a, mut b, mut c, mut d] = self.state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a.wrapping_add(f).wrapping_add(k(i)).wrapping_add(m[g]).rotate_left(S[i]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(rotated);
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    }
}

fn md5(parts: &[&[u8]]) -> [u8; 16] {
    let mut m = Md5::new();
    for p in parts {
        m.update(p);
    }
    m.finish()
}

fn hmac_md5(key: &[u8], message: &[u8]) -> [u8; 16] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..16].copy_from_slice(&md5(&[key]));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5C).collect();
    let inner = md5(&[&ipad, message]);
    md5(&[&opad, &inner])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn md5_matches_rfc_1321() {
        assert_eq!(hex(&md5(&[b""])), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex(&md5(&[b"abc"])), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex(&md5(&[b"message ", b"digest"])), "f96b697d7cb7938d525a2f31aaf161d0", "in two pieces");
        let long = b"1234567890".repeat(8);
        assert_eq!(hex(&md5(&[&long])), "57edf4a22be3c955ac49da2e2107b67a");
        assert_eq!(hex(&hmac_md5(b"key", b"The quick brown fox jumps over the lazy dog")), "80070713463e7749b90c2dc24911e275");
    }

    #[test]
    fn keys_match_rfc_3414_and_7860() {
        let engine = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2];
        let cases = [
            (Auth::Md5, "9faf3283884e92834ebc9847d8edd963", "526f5eed9fcce26f8964c2930787d82b"),
            (Auth::Sha1, "9fb5cc0381497b3793528939ff788d5d79145211", "6695febc9288e36282235fc7151f128497b38f3f"),
            (
                Auth::Sha256,
                "ab51014d1e077f6017df2b12bee5f5aa72993177e9bb569c4dff5a4ca0b4afac",
                "8982e0e549e866db361a6b625d84cccc11162d453ee8ce3a6445c2d6776f0f8b",
            ),
        ];
        for (auth, ku, kul) in cases {
            let key = auth.password_to_key(b"maplesyrup");
            assert_eq!(hex(&key), ku, "{auth:?}");
            assert_eq!(hex(&auth.localize(&key, &engine)), kul, "{auth:?}");
        }
    }

    #[test]
    fn aes_cfb_matches_nist_and_comes_back() {
        // NIST SP 800-38A, F.3.13: CFB128-AES128.
        let key: Vec<u8> = (0..16).map(|i| [0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c][i]).collect();
        let iv: [u8; 16] = std::array::from_fn(|i| i as u8);
        let plain = [0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a, 0xae, 0x2d];
        let cipher = cfb128(&key, iv, &plain, true);
        assert_eq!(hex(&cipher[..16]), "3b3fd92eb72dad20333449f8e83cfb4a");
        assert_eq!(cfb128(&key, iv, &cipher, false), plain);

        let salt = [1, 2, 3, 4, 5, 6, 7, 8];
        let scoped = b"a scoped PDU, not a multiple of sixteen bytes long";
        let sealed = aes_cfb(&key, 3, 1234, &salt, scoped, true);
        assert_ne!(&sealed[..], &scoped[..]);
        assert_eq!(aes_cfb(&key, 3, 1234, &salt, &sealed, false), scoped);
    }

    #[test]
    fn the_mac_is_cut_to_its_protocols_length() {
        assert_eq!(Auth::Md5.mac(&[1; 16], b"m").len(), 12);
        assert_eq!(Auth::Sha1.mac(&[1; 20], b"m").len(), 12);
        assert_eq!(Auth::Sha256.mac(&[1; 32], b"m").len(), 24);
        assert_eq!(Auth::parse("SHA"), Some(Auth::Sha1));
        assert_eq!(Auth::parse("none"), None);
    }
}
