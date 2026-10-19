// Gmail, the pill `integration_gmail`: new mail shows in the island as a
// notification (notify.rs; Open opens it in Gmail), and the pill shows how many
// are unread and the latest of them.
//
// Over IMAP with an app password (Google account → Security → 2-Step
// Verification → App passwords), kept in the keychain with the address: no
// Google sign-in, and nothing else of the account is reached. Gmail pushes:
// IMAP IDLE waits for the server to say something, and Ciel looks again only
// then, or every 25 minutes as RFC 2177 asks. Only mail that arrives while
// Ciel watches is notified; what was already unread is only counted.
//
// The log says how the connection is, never who wrote or what about.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use async_imap::imap_proto::Address;
use async_imap::types::{Fetch, Flag};
use futures_util::TryStreamExt;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::integrations::{IntegrationEvent, IntegrationUpdate};
use crate::notify::{self, Notification, Target};
use crate::{log, secrets};

pub const PILL_ID: &str = "integration_gmail";
pub const ADDRESS_KEY: &str = "gmail-address";
pub const PASSWORD_KEY: &str = "gmail-app-password";

const HOST: &str = "imap.gmail.com";
const PORT: u16 = 993;
/// IDLE is renewed this often at the latest (RFC 2177: under 29 minutes).
const IDLE_FOR: Duration = Duration::from_secs(25 * 60);
/// At most this many notifications for one burst of mail.
const MAX_NEW: usize = 5;
/// The pill's latest unread mails.
const LATEST: usize = 3;
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

// ── Mail headers ──────────────────────────────────────────────────────────────

/// RFC 2047's encoded words (`=?UTF-8?B?…?=`, `=?ISO-8859-1?Q?…?=`) decoded;
/// the space between two of them goes, as the RFC says; the rest is UTF-8.
pub fn decode_words(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::new();
    let mut rest: &str = &text;
    let mut after_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, from) = rest.split_at(start);
        let decoded = parse_word(from);
        let Some((word, used)) = decoded else {
            out.push_str(before);
            out.push_str("=?");
            rest = &from[2..];
            after_word = false;
            continue;
        };
        if !(after_word && before.trim().is_empty()) {
            out.push_str(before);
        }
        out.push_str(&word);
        rest = &from[used..];
        after_word = true;
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One encoded word at the start of `s`: its text, and how much of `s` it took.
fn parse_word(s: &str) -> Option<(String, usize)> {
    let body = s.strip_prefix("=?")?;
    let mut parts = body.splitn(3, '?');
    let charset = parts.next()?;
    let encoding = parts.next()?;
    let rest = parts.next()?;
    let end = rest.find("?=")?;
    let payload = &rest[..end];
    let used = 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
    let bytes = match encoding.to_ascii_uppercase().as_str() {
        "B" => {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()?
        }
        "Q" => decode_q(payload),
        _ => return None,
    };
    // "UTF-8*en": a language after the charset.
    let label = charset.split('*').next().unwrap_or(charset);
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(&bytes);
    Some((text.into_owned(), used))
}

fn decode_q(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b' '),
            // =XX: a byte in hex; anything else after = stays as it is.
            b'=' if i + 2 < b.len() => {
                let byte = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match byte {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'='),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Who a mail is from: their name, else their address.
pub fn sender(address: Option<&Address<'_>>) -> String {
    let Some(a) = address else { return String::new() };
    let name = a.name.as_deref().map(decode_words).unwrap_or_default();
    if !name.is_empty() {
        return name;
    }
    let mailbox = a.mailbox.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
    let host = a.host.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
    if host.is_empty() { mailbox.into_owned() } else { format!("{mailbox}@{host}") }
}

/// The mail in Gmail, by Gmail's own id; the inbox without one.
pub fn mail_url(gmail_id: Option<u64>) -> String {
    match gmail_id {
        Some(id) => format!("https://mail.google.com/mail/u/0/#all/{id:x}"),
        None => "https://mail.google.com/mail/u/0/#inbox".into(),
    }
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

/// From a fetched mail: who it's from, its subject, and where it opens.
fn read_mail(f: &Fetch) -> (String, String, String) {
    let env = f.envelope();
    let from = sender(env.and_then(|e| e.from.as_ref()).and_then(|v| v.first()));
    let subject = env.and_then(|e| e.subject.as_deref()).map(decode_words).unwrap_or_default();
    (clip(&from, 80), clip(&subject, 200), mail_url(f.gmail_msg_id().copied()))
}

fn unread(f: &Fetch) -> bool {
    !f.flags().any(|flag| matches!(flag, Flag::Seen))
}

// ── New arrivals ──────────────────────────────────────────────────────────────

/// Which mails are new: UIDs only grow, so what is at or above the first one
/// not yet seen is new. It starts at the inbox's UIDNEXT when Ciel connects,
/// so the mail already there is never notified.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrivals {
    next: u32,
}

impl Arrivals {
    pub fn new(uid_next: u32) -> Self {
        Arrivals { next: uid_next.max(1) }
    }

    pub fn next(&self) -> u32 {
        self.next
    }

    /// The new UIDs among `uids` (the latest few at most, oldest first); they count as seen.
    pub fn take(&mut self, uids: impl IntoIterator<Item = u32>) -> Vec<u32> {
        let mut new: Vec<u32> = uids.into_iter().filter(|u| *u >= self.next).collect();
        new.sort_unstable();
        new.dedup();
        if let Some(last) = new.last() {
            self.next = last + 1;
        }
        let skip = new.len().saturating_sub(MAX_NEW);
        new.split_off(skip)
    }
}

// ── The watcher ───────────────────────────────────────────────────────────────

struct Watch {
    generation: u64,
    stop: Arc<Notify>,
}

static WATCH: LazyLock<Mutex<Option<Watch>>> = LazyLock::new(|| Mutex::new(None));
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Watches Gmail when it should — the pill declared, the address and the
/// password in the keychain, Ciel not paused — and stops otherwise. `restart`:
/// the credentials changed, so a watcher already running starts again.
pub fn sync(app: &AppHandle, active_integrations: &[String], restart: bool) {
    let want = active_integrations.iter().any(|id| id == PILL_ID)
        && !crate::integrations::PAUSED.load(Ordering::Relaxed)
        && secrets::present(ADDRESS_KEY)
        && secrets::present(PASSWORD_KEY);
    let mut watch = WATCH.lock().unwrap();
    if watch.is_some() && (!want || restart) {
        if let Some(w) = watch.take() {
            w.stop.notify_one();
        }
    }
    if want && watch.is_none() {
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        let stop = Arc::new(Notify::new());
        *watch = Some(Watch { generation, stop: stop.clone() });
        tauri::async_runtime::spawn(run(app.clone(), generation, stop));
    }
}

fn current(generation: u64) -> bool {
    WATCH.lock().unwrap().as_ref().is_some_and(|w| w.generation == generation)
}

enum Failure {
    /// The address or the app password is refused: no use trying again until they change.
    Login,
    Network(String),
}

fn net(e: impl std::fmt::Display) -> Failure {
    Failure::Network(e.to_string())
}

fn report(app: &AppHandle, data: Value, error: Option<String>, event: Option<IntegrationEvent>) {
    let _ = app.emit_to(crate::island::WINDOW_LABEL, "integration", IntegrationUpdate { id: PILL_ID, data, error, event });
}

async fn run(app: AppHandle, generation: u64, stop: Arc<Notify>) {
    let mut backoff = Duration::from_secs(5);
    while current(generation) {
        match watch(&app, generation, &stop).await {
            Ok(()) => break,
            Err(Failure::Login) => {
                log::line("gmail: the address or the app password was refused");
                report(&app, json!({}), Some(crate::i18n::t("Wrong address or app password")), None);
                // Until they change (sync restarts us) or the pill goes.
                stop.notified().await;
                break;
            }
            Err(Failure::Network(e)) => {
                log::line(format!("gmail: no connection ({}), again in {} s", e.lines().next().unwrap_or(""), backoff.as_secs()));
                report(&app, json!({}), Some(crate::i18n::tf("No connection: {error}", &[("error", &e)])), None);
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = stop.notified() => break,
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
    // The pill is off or reset: it shows nothing more from this watcher.
    if !current(generation) {
        report(&app, json!({}), None, None);
    }
}

type Tls = tokio_rustls::client::TlsStream<tokio::net::TcpStream>;

async fn connect() -> Result<Tls, Failure> {
    use tokio_rustls::rustls;
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(net)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = tokio::time::timeout(TIMEOUT, tokio::net::TcpStream::connect((HOST, PORT)))
        .await
        .map_err(net)?
        .map_err(net)?;
    let name = rustls::pki_types::ServerName::try_from(HOST).map_err(net)?;
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    tokio::time::timeout(TIMEOUT, connector.connect(name, tcp)).await.map_err(net)?.map_err(net)
}

/// One connection, until it fails or Ciel stops it (Ok).
async fn watch(app: &AppHandle, generation: u64, stop: &Notify) -> Result<(), Failure> {
    let (Some(address), Some(password)) = (secrets::get(ADDRESS_KEY), secrets::get(PASSWORD_KEY)) else {
        return Ok(());
    };
    // Google shows app passwords in groups of four: the spaces aren't part of it.
    let password: String = password.chars().filter(|c| !c.is_whitespace()).collect();

    let mut client = async_imap::Client::new(connect().await?);
    // The server's greeting comes first.
    tokio::time::timeout(TIMEOUT, client.read_response()).await.map_err(net)?.map_err(net)?;
    let mut session = match tokio::time::timeout(TIMEOUT, client.login(address.trim(), &password)).await.map_err(net)? {
        Ok(s) => s,
        Err((async_imap::error::Error::No(_), _)) => return Err(Failure::Login),
        Err((e, _)) => return Err(net(e)),
    };
    let inbox = session.select("INBOX").await.map_err(net)?;
    let mut arrivals = Arrivals::new(inbox.uid_next.unwrap_or(1));
    log::line("gmail: connected");

    loop {
        look(app, &mut session, &mut arrivals).await?;
        if !current(generation) {
            let _ = session.logout().await;
            return Ok(());
        }
        // Until Gmail says something, IDLE_FOR at most, or Ciel stops it.
        let mut idle = session.idle();
        idle.init().await.map_err(net)?;
        let stopped = {
            let (wait, _interrupt) = idle.wait_with_timeout(IDLE_FOR);
            tokio::select! {
                r = tokio::time::timeout(IDLE_FOR, wait) => {
                    if let Ok(Err(e)) = r {
                        return Err(net(e));
                    }
                    false
                }
                _ = stop.notified() => true,
            }
        };
        session = idle.done().await.map_err(net)?;
        if stopped || !current(generation) {
            let _ = session.logout().await;
            return Ok(());
        }
    }
}

/// New mail to tell about, then the unread for the pill.
async fn look(app: &AppHandle, session: &mut async_imap::Session<Tls>, arrivals: &mut Arrivals) -> Result<(), Failure> {
    let found = session.uid_search(format!("UID {}:*", arrivals.next())).await.map_err(net)?;
    let new = arrivals.take(found);
    let mut told = 0;
    if !new.is_empty() {
        let set = new.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let fetched: Vec<Fetch> =
            session.uid_fetch(&set, "(UID FLAGS ENVELOPE X-GM-MSGID)").await.map_err(net)?.try_collect().await.map_err(net)?;
        for f in fetched.iter().filter(|f| unread(f)) {
            let (from, subject, url) = read_mail(f);
            let n = Notification {
                id: notify::next_id("gmail"),
                source: "gmail".into(),
                app: "Gmail".into(),
                title: if from.is_empty() { subject.clone() } else { from },
                body: subject,
                icon: None,
                at: now_ms(),
                tag: None,
            };
            notify::show(app, n, Target::Url(url));
            told += 1;
        }
    }

    let unseen: HashSet<u32> = session.uid_search("UNSEEN").await.map_err(net)?;
    let mut latest: Vec<u32> = unseen.iter().copied().collect();
    latest.sort_unstable_by(|a, b| b.cmp(a));
    latest.truncate(LATEST);
    let mut mails: Vec<(u32, Value)> = Vec::new();
    if !latest.is_empty() {
        let set = latest.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let fetched: Vec<Fetch> =
            session.uid_fetch(&set, "(UID ENVELOPE X-GM-MSGID)").await.map_err(net)?.try_collect().await.map_err(net)?;
        for f in &fetched {
            let (from, subject, url) = read_mail(f);
            mails.push((f.uid.unwrap_or(0), json!({"from": from, "subject": subject, "url": url})));
        }
        mails.sort_by(|a, b| b.0.cmp(&a.0));
    }
    let event = (told > 0).then(|| IntegrationEvent {
        success: true,
        label: crate::i18n::t("New mail"),
        detail: None,
    });
    let mails: Vec<Value> = mails.into_iter().map(|(_, m)| m).collect();
    report(app, json!({"unread": unseen.len(), "mails": mails}), None, event);
    Ok(())
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn encoded_words_decode_in_their_charset() {
        assert_eq!(decode_words(b"=?UTF-8?B?Q2Fmw6kgw6AgMTRo?="), "Café à 14h");
        assert_eq!(decode_words(b"=?ISO-8859-1?Q?R=E9union_d=27=E9quipe?="), "Réunion d'équipe");
        assert_eq!(decode_words(b"=?utf-8?q?Hello_World?="), "Hello World");
        // Two encoded words: the space between them goes; plain text keeps its own.
        assert_eq!(decode_words(b"=?UTF-8?Q?Bon?= =?UTF-8?Q?jour?= le monde"), "Bonjour le monde");
        assert_eq!(decode_words(b"Plain subject"), "Plain subject");
        assert_eq!(decode_words("Déjà UTF-8".as_bytes()), "Déjà UTF-8");
        assert_eq!(decode_words(b"=?KOI8-R?B?8NLJ18XU?="), "Привет");
        // Not an encoded word after all: left as it is.
        assert_eq!(decode_words(b"x =? y"), "x =? y");
    }

    #[test]
    fn the_sender_is_their_name_else_their_address() {
        let named = Address {
            name: Some(Cow::Borrowed(b"=?UTF-8?Q?L=C3=A9a?= Martin")),
            adl: None,
            mailbox: Some(Cow::Borrowed(b"lea")),
            host: Some(Cow::Borrowed(b"example.com")),
        };
        assert_eq!(sender(Some(&named)), "Léa Martin");
        let bare = Address { name: None, adl: None, mailbox: Some(Cow::Borrowed(b"noreply")), host: Some(Cow::Borrowed(b"github.com")) };
        assert_eq!(sender(Some(&bare)), "noreply@github.com");
        assert_eq!(sender(None), "");
    }

    #[test]
    fn a_mail_opens_by_gmails_own_id() {
        assert_eq!(mail_url(Some(1_234_567_890_123)), "https://mail.google.com/mail/u/0/#all/11f71fb04cb");
        assert_eq!(mail_url(None), "https://mail.google.com/mail/u/0/#inbox");
    }

    #[test]
    fn only_mail_arriving_while_watching_is_new() {
        // The inbox at connection: UIDNEXT 120, so 1…119 were there already.
        let mut a = Arrivals::new(120);
        // UID 120:* with nothing new answers the last UID there is.
        assert_eq!(a.take([119]), Vec::<u32>::new());
        assert_eq!(a.take([120, 121]), vec![120, 121]);
        assert_eq!(a.next(), 122);
        // Seen once, never again.
        assert_eq!(a.take([121]), Vec::<u32>::new());
        // A burst: the latest few only, all counted as seen.
        let burst: Vec<u32> = (122..140).collect();
        assert_eq!(a.take(burst), (135..140).collect::<Vec<_>>());
        assert_eq!(a.next(), 140);
    }

    #[test]
    fn long_text_is_clipped_on_a_character() {
        assert_eq!(clip("Réunion", 3), "Réu…");
        assert_eq!(clip("short", 10), "short");
    }
}
