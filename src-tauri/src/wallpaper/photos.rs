// Free photos for the wallpapers: NASA's Image and Video Library for "space"
// (public domain), Wikimedia Commons for the other themes (CC0, public domain,
// CC BY and CC BY-SA only). The network is used only when the user clicks
// "Get photos": one request at a time, a second apart, with a User-Agent that
// says who is asking (Wikimedia's policy). Photos are cached in
// `<data dir>/wallpaper/photos/<theme>/`, with credits.json saying who made
// each one, under which licence, and where it came from.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::{t, tf};

pub const THEMES: [&str; 7] = ["clear", "cloudy", "golden", "sunset", "night", "space", "storm"];

const USER_AGENT: &str =
    concat!("Ciel/", env!("CARGO_PKG_VERSION"), " (desktop wallpaper photos; https://github.com/jean-eudes-asg/ciel)");
/// Landscape only, and sharp enough for a large screen.
const MIN_WIDTH: u32 = 2560;
const MIN_RATIO: f64 = 1.3;
/// Commons makes thumbnails at standard widths; 3840 is the largest.
const THUMB_WIDTH: u32 = 3840;
/// A search or collection answer, and a picture.
const MAX_ANSWER: usize = 8 * 1024 * 1024;
const MAX_PICTURE: usize = 40 * 1024 * 1024;
/// Between two requests.
const PAUSE: Duration = Duration::from_millis(1000);
/// At most this many new photos per click.
const MAX_PER_FETCH: u32 = 12;

fn queries(theme: &str) -> Option<&'static [&'static str]> {
    Some(match theme {
        "clear" => &["blue sky clouds"],
        "cloudy" => &["cumulus clouds sky", "overcast sky"],
        "golden" => &["golden hour sky"],
        "sunset" => &["sunset sky"],
        "night" => &["night sky stars milky way"],
        "storm" => &["storm clouds sky"],
        "space" => &["nebula", "galaxy", "Hubble", "Webb", "earth from space"],
        _ => return None,
    })
}

fn root() -> PathBuf {
    super::dir().join("photos")
}

fn unknown_theme(theme: &str) -> String {
    tf("Unknown theme: {theme}", &[("theme", theme)])
}

/// The theme's cache folder, for a theme Ciel knows (never a path).
fn theme_dir(theme: &str) -> Result<PathBuf, String> {
    THEMES.contains(&theme).then(|| root().join(theme)).ok_or_else(|| unknown_theme(theme))
}

// ── Credits ───────────────────────────────────────────────────────────────────

/// One line of credits.json.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credit {
    /// The file's name in the theme's folder.
    pub file: String,
    /// "nasa:<nasa_id>" or "commons:<pageid>": never downloaded twice.
    pub id: String,
    pub title: String,
    pub author: String,
    pub license: String,
    pub source_url: String,
    pub width: u32,
    pub height: u32,
}

/// What `wallpaper_photos` returns.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Photo {
    pub path: String,
    pub title: String,
    pub author: String,
    pub license: String,
    pub source_url: String,
    pub width: u32,
    pub height: u32,
}

fn load_credits(folder: &Path) -> Vec<Credit> {
    std::fs::read(folder.join("credits.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_credits(folder: &Path, credits: &[Credit]) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(credits).map_err(std::io::Error::other)?;
    super::write_atomic(&folder.join("credits.json"), &json)
}

/// The photos already cached for `theme`, those whose file is still there.
pub fn cached(theme: &str) -> Result<Vec<Photo>, String> {
    let folder = theme_dir(theme)?;
    Ok(load_credits(&folder)
        .into_iter()
        .filter_map(|c| {
            let path = folder.join(&c.file);
            path.is_file().then(|| Photo {
                path: path.to_string_lossy().into_owned(),
                title: c.title,
                author: c.author,
                license: c.license,
                source_url: c.source_url,
                width: c.width,
                height: c.height,
            })
        })
        .collect())
}

/// A cached photo's bytes; anything outside the photo cache is refused.
pub fn bytes(path: &str) -> Result<Vec<u8>, String> {
    bytes_in(&root(), path)
}

fn bytes_in(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let refuse = || t("That file isn't one of Ciel's photos.");
    let root = std::fs::canonicalize(root).map_err(|_| refuse())?;
    let file = std::fs::canonicalize(path).map_err(|_| refuse())?;
    let picture = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| matches!(e.as_str(), "jpg" | "jpeg" | "png"));
    if !file.starts_with(&root) || !file.is_file() || !picture {
        return Err(refuse());
    }
    std::fs::read(&file).map_err(|e| tf("Ciel couldn't read that photo: {error}", &[("error", &e.to_string())]))
}

// ── Reading the answers ───────────────────────────────────────────────────────

/// A photo found in a search, before it is downloaded.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub title: String,
    pub author: String,
    pub license: String,
    pub source_url: String,
    /// Commons: the picture itself. NASA: the item's collection.json.
    pub url: String,
    /// The size of what `url` gives, when the answer says (checked again
    /// from the picture's own header once downloaded).
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn landscape(width: u32, height: u32) -> bool {
    height > 0 && width >= MIN_WIDTH && width as f64 / height as f64 >= MIN_RATIO
}

/// Only free licences that allow any use: CC0, public domain, CC BY and CC
/// BY-SA (any version or port). Not NC, ND, GFDL-only, the Free Art License…
pub fn license_ok(short: &str) -> bool {
    let lower = short.trim().to_lowercase();
    let tokens: Vec<&str> =
        lower.split(|c: char| !c.is_ascii_alphanumeric()).filter(|t| !t.is_empty()).collect();
    if tokens.iter().any(|t| *t == "nc" || *t == "nd") {
        return false;
    }
    match tokens.as_slice() {
        ["cc0", ..] | ["public", "domain", ..] | ["pd", ..] => true,
        ["cc", "by", rest @ ..] => rest.iter().all(|t| {
            *t == "sa" || t.chars().all(|c| c.is_ascii_digit()) || (t.len() <= 3 && t.chars().all(|c| c.is_ascii_alphabetic()))
        }),
        _ => false,
    }
}

/// Commons's HTML fields (Artist) as plain text.
pub fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest[..rest.len().min(10)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn meta<'a>(info: &'a Value, key: &str) -> Option<&'a str> {
    info.pointer(&format!("/extmetadata/{key}/value")).and_then(Value::as_str)
}

/// The photos a Commons search answer offers that may be used: a free
/// licence, JPEG or PNG, landscape and large enough. In search order.
pub fn commons_candidates(answer: &Value) -> Vec<Candidate> {
    let mut pages: Vec<&Value> = answer.pointer("/query/pages").and_then(Value::as_array).into_iter().flatten().collect();
    pages.sort_by_key(|p| p.get("index").and_then(Value::as_u64).unwrap_or(u64::MAX));
    pages
        .into_iter()
        .filter_map(|page| {
            let info = page.pointer("/imageinfo/0")?;
            let mime = info.get("mime").and_then(Value::as_str)?;
            if mime != "image/jpeg" && mime != "image/png" {
                return None;
            }
            let num = |k: &str| info.get(k).and_then(Value::as_u64).map(|n| n as u32);
            let (width, height) = (num("width")?, num("height")?);
            let license = meta(info, "LicenseShortName")?.trim().to_string();
            if !landscape(width, height) || !license_ok(&license) {
                return None;
            }
            // Past 3840 px the standard thumbnail is plenty, and far lighter.
            let thumb = info.get("thumburl").and_then(Value::as_str).zip(num("thumbwidth")).zip(num("thumbheight"));
            let (url, width, height) = match thumb {
                Some(((url, w), h)) if width > THUMB_WIDTH && landscape(w, h) => (url, w, h),
                _ => (info.get("url").and_then(Value::as_str)?, width, height),
            };
            let file_title = page.get("title").and_then(Value::as_str).unwrap_or_default();
            let file_title = file_title.strip_prefix("File:").unwrap_or(file_title);
            let file_title = file_title.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(file_title);
            let title = meta(info, "ObjectName").map(plain_text).filter(|t| !t.is_empty()).unwrap_or_else(|| file_title.to_string());
            Some(Candidate {
                id: format!("commons:{}", page.get("pageid").and_then(Value::as_u64)?),
                title,
                author: meta(info, "Artist").map(plain_text).unwrap_or_default(),
                license,
                source_url: info.get("descriptionurl").and_then(Value::as_str).unwrap_or_default().to_string(),
                url: url.to_string(),
                width: Some(width),
                height: Some(height),
            })
        })
        .collect()
}

fn https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

fn url_part(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The images a NASA search answer offers that may suit: landscape and large
/// enough by the original's size, or — when the answer doesn't give it — by
/// the proportions of its largest preview (the size is then checked once the
/// picture is downloaded).
pub fn nasa_candidates(answer: &Value) -> Vec<Candidate> {
    let items = answer.pointer("/collection/items").and_then(Value::as_array).into_iter().flatten();
    items
        .filter_map(|item| {
            let data = item.pointer("/data/0")?;
            if data.get("media_type").and_then(Value::as_str) != Some("image") {
                return None;
            }
            let id = data.get("nasa_id").and_then(Value::as_str)?;
            let links: Vec<&Value> = item.get("links").and_then(Value::as_array).into_iter().flatten().collect();
            let size = |l: &Value| Some((l.get("width")?.as_u64()? as u32, l.get("height")?.as_u64()? as u32));
            let canonical = links.iter().find(|l| l.get("rel").and_then(Value::as_str) == Some("canonical"));
            let (width, height) = match canonical.and_then(|l| size(l)) {
                Some((w, h)) => {
                    if !landscape(w, h) {
                        return None;
                    }
                    (Some(w), Some(h))
                }
                None => {
                    let (w, h) = links.iter().filter_map(|l| size(l)).max_by_key(|(w, _)| *w)?;
                    if h == 0 || (w as f64 / h as f64) < MIN_RATIO {
                        return None;
                    }
                    (None, None)
                }
            };
            let text = |k: &str| data.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
            Some(Candidate {
                id: format!("nasa:{id}"),
                title: text("title").unwrap_or(id).to_string(),
                author: text("photographer").or_else(|| text("secondary_creator")).unwrap_or("NASA").to_string(),
                license: "Public domain (NASA)".to_string(),
                source_url: format!("https://images.nasa.gov/details/{}", url_part(id)),
                url: https(item.get("href").and_then(Value::as_str)?),
                width,
                height,
            })
        })
        .collect()
}

/// The largest JPEG or PNG listed in an item's collection.json.
pub fn nasa_largest(collection: &Value) -> Option<String> {
    let files: Vec<&str> = collection.as_array()?.iter().filter_map(Value::as_str).collect();
    for size in ["~orig", "~large", "~medium"] {
        for ext in [".jpg", ".jpeg", ".png"] {
            if let Some(f) = files.iter().find(|f| f.to_ascii_lowercase().ends_with(&format!("{size}{ext}"))) {
                return Some(https(f));
            }
        }
    }
    None
}

/// Width, height and extension from a JPEG's or PNG's header.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32, &'static str)> {
    if super::is_png(bytes) && bytes.len() >= 24 && &bytes[12..16] == b"IHDR" {
        let be32 = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        return Some((be32(16), be32(20), "png"));
    }
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let be16 = |i: usize| u16::from_be_bytes([bytes[i], bytes[i + 1]]) as u32;
    let mut i = 2;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD9).contains(&marker) {
            i += 2;
            continue;
        }
        let len = be16(i + 2) as usize;
        let sof = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if sof {
            if i + 9 > bytes.len() {
                return None;
            }
            return Some((be16(i + 7), be16(i + 5), "jpg"));
        }
        i += 2 + len;
    }
    None
}

// ── Fetching ──────────────────────────────────────────────────────────────────

static FETCHING: AtomicBool = AtomicBool::new(false);

struct Busy;
impl Drop for Busy {
    fn drop(&mut self) {
        FETCHING.store(false, Ordering::SeqCst);
    }
}

/// Keeps requests a second apart.
struct Throttle(Option<Instant>);

impl Throttle {
    async fn wait(&mut self) {
        if let Some(last) = self.0 {
            let since = last.elapsed();
            if since < PAUSE {
                tokio::time::sleep(PAUSE - since).await;
            }
        }
        self.0 = Some(Instant::now());
    }
}

/// Why a candidate was not kept: not what we want (skipped quietly), or the
/// network failed (said if nothing at all could be fetched).
enum Miss {
    Skip,
    Net(String),
}

async fn get(client: &reqwest::Client, gate: &mut Throttle, url: &str, limit: usize) -> Result<Vec<u8>, Miss> {
    gate.wait().await;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| Miss::Net(tf("Network error: {error}", &[("error", &e.without_url().to_string())])))?;
    let status = response.status();
    if !status.is_success() {
        return Err(Miss::Net(tf("Network error: {error}", &[("error", &status.to_string())])));
    }
    crate::net::read_capped(response, limit).await.map_err(|_| Miss::Skip)
}

async fn search(client: &reqwest::Client, gate: &mut Throttle, theme: &str, query: &str, round: u64) -> Result<Vec<Candidate>, Miss> {
    let (url, nasa) = if theme == "space" {
        let page = (1 + round % 3).to_string();
        let params = [("q", query), ("media_type", "image"), ("page_size", "100"), ("page", page.as_str())];
        (Url::parse_with_params("https://images-api.nasa.gov/search", &params), true)
    } else {
        let search = format!("{query} fileres:>3000");
        let offset = ((round % 4) * 30).to_string();
        let params = [
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("generator", "search"),
            ("gsrsearch", search.as_str()),
            ("gsrnamespace", "6"),
            ("gsrlimit", "30"),
            ("gsroffset", offset.as_str()),
            ("prop", "imageinfo"),
            ("iiprop", "url|size|mime|extmetadata"),
            ("iiurlwidth", "3840"),
            ("iiextmetadatafilter", "Artist|LicenseShortName|ObjectName"),
        ];
        (Url::parse_with_params("https://commons.wikimedia.org/w/api.php", &params), false)
    };
    let url = url.map_err(|e| Miss::Net(e.to_string()))?;
    let body = get(client, gate, url.as_str(), MAX_ANSWER).await?;
    let answer: Value = serde_json::from_slice(&body).map_err(|_| Miss::Skip)?;
    Ok(if nasa { nasa_candidates(&answer) } else { commons_candidates(&answer) })
}

/// The picture for `candidate`, checked from its own header.
async fn download(client: &reqwest::Client, gate: &mut Throttle, candidate: &Candidate) -> Result<(Vec<u8>, u32, u32, &'static str), Miss> {
    let url = if candidate.id.starts_with("nasa:") {
        let body = get(client, gate, &candidate.url, MAX_ANSWER).await?;
        let collection: Value = serde_json::from_slice(&body).map_err(|_| Miss::Skip)?;
        nasa_largest(&collection).ok_or(Miss::Skip)?
    } else {
        candidate.url.clone()
    };
    let bytes = get(client, gate, &url, MAX_PICTURE).await?;
    let (width, height, ext) = image_size(&bytes).ok_or(Miss::Skip)?;
    if !landscape(width, height) {
        return Err(Miss::Skip);
    }
    Ok((bytes, width, height, ext))
}

/// A file name for a photo id: `nasa:PIA14417` → `nasa-PIA14417`.
fn file_stem(id: &str) -> String {
    let mut stem: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    stem.truncate(80);
    stem
}

fn seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7)
}

/// Downloads up to `count` photos for `theme` that are not cached yet, and
/// says how many came. Searches a few pages of the theme's queries, starting
/// somewhere different each time.
pub async fn fetch(theme: &str, count: u32) -> Result<u32, String> {
    let queries = queries(theme).ok_or_else(|| unknown_theme(theme))?;
    let folder = theme_dir(theme)?;
    let want = count.min(MAX_PER_FETCH);
    if want == 0 {
        return Ok(0);
    }
    if FETCHING.swap(true, Ordering::SeqCst) {
        return Err(t("Ciel is already getting photos."));
    }
    let _busy = Busy;
    crate::platform::ensure_private_dir(&folder).map_err(super::not_written)?;
    let mut credits = load_credits(&folder);
    let mut known: HashSet<String> = credits.iter().map(|c| c.id.clone()).collect();
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| tf("Network error: {error}", &[("error", &e.to_string())]))?;
    let mut gate = Throttle(None);
    let start = seed();
    let (mut added, mut tries, mut failure) = (0u32, 0u32, None);
    'rounds: for round in 0..3u64 {
        for q in 0..queries.len() {
            let query = queries[(start as usize + q) % queries.len()];
            let candidates = match search(&client, &mut gate, theme, query, start.wrapping_add(round)).await {
                Ok(c) => c,
                Err(Miss::Net(e)) => {
                    failure = Some(e);
                    continue;
                }
                Err(Miss::Skip) => continue,
            };
            let fresh: Vec<Candidate> = candidates.into_iter().filter(|c| !known.contains(&c.id)).collect();
            for candidate in fresh {
                if added >= want || tries >= want * 4 {
                    break 'rounds;
                }
                tries += 1;
                match download(&client, &mut gate, &candidate).await {
                    Ok((bytes, width, height, ext)) => {
                        let file = format!("{}.{ext}", file_stem(&candidate.id));
                        super::write_atomic(&folder.join(&file), &bytes).map_err(super::not_written)?;
                        known.insert(candidate.id.clone());
                        credits.push(Credit {
                            file,
                            id: candidate.id,
                            title: candidate.title,
                            author: candidate.author,
                            license: candidate.license,
                            source_url: candidate.source_url,
                            width,
                            height,
                        });
                        save_credits(&folder, &credits).map_err(super::not_written)?;
                        added += 1;
                    }
                    Err(Miss::Net(e)) => failure = Some(e),
                    Err(Miss::Skip) => {}
                }
            }
        }
        if added >= want {
            break;
        }
    }
    crate::log::line(format!("wallpaper photos: {added} new for {theme} ({tries} tried)"));
    match failure {
        Some(error) if added == 0 => Err(error),
        _ => Ok(added),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/wallpaper/fixtures").join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn only_licences_that_allow_any_use_are_taken() {
        for ok in ["CC0", "Public domain", "PD-self", "CC BY 4.0", "CC BY-SA 4.0", "CC BY-SA 3.0 us", "CC BY 2.0", "cc-by-sa-2.5-nl", "CC BY-SA 3.0 igo"] {
            assert!(license_ok(ok), "{ok}");
        }
        for no in ["FAL", "GFDL", "CC BY-NC 4.0", "CC BY-NC-SA 2.0", "CC BY-ND 3.0", "Copyrighted free use", "Attribution", "", "CC BY-SA 4.0 or GFDL"] {
            assert!(!license_ok(no), "{no}");
        }
    }

    #[test]
    fn commons_answers_keep_free_landscape_photos_with_their_credits() {
        // Recorded answer for "sunset sky": a Free Art License photo, a square
        // one, and four that may be used.
        let found = commons_candidates(&fixture("commons-sunset.json"));
        let ids: Vec<&str> = found.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["commons:54469472", "commons:92581386", "commons:156858817", "commons:89619356"]);
        let first = &found[0];
        assert_eq!(first.title, "Cumulonimbus sunset panorama, Albury NSW Australia");
        assert_eq!(first.author, "Thennicke");
        assert_eq!(first.license, "CC BY-SA 4.0");
        assert_eq!(first.source_url, "https://commons.wikimedia.org/wiki/File:Cumulonimbus_sunset_panorama,_Albury_NSW_Australia.jpg");
        // 6134 px wide: the 3840 px thumbnail is downloaded instead.
        assert!(first.url.contains("/3840px-"), "{}", first.url);
        assert_eq!((first.width, first.height), (Some(3840), Some(1920)));
        let cc0 = found.iter().find(|c| c.license == "CC0").unwrap();
        assert_eq!(cc0.author, "W.carter");
        assert!(found.iter().all(|c| c.license != "FAL"));

        // "night sky": public domain and a US port of CC BY-SA in, a portrait one out.
        let night = commons_candidates(&fixture("commons-night.json"));
        let licenses: Vec<&str> = night.iter().map(|c| c.license.as_str()).collect();
        assert_eq!(licenses, ["Public domain", "CC0", "CC BY-SA 3.0 us"]);
        assert_eq!(night[0].author, "NASA/JPL-Caltech/UCLA");
        assert!(night.iter().all(|c| c.width.unwrap() >= MIN_WIDTH));
    }

    #[test]
    fn nasa_answers_keep_large_landscape_images() {
        let found = nasa_candidates(&fixture("nasa-search.json"));
        let ids: Vec<&str> = found.iter().map(|c| c.id.as_str()).collect();
        // A portrait one, a small one, and a square-ish one without sizes are left out.
        assert_eq!(ids, ["nasa:PIA14417", "nasa:PIA13014", "nasa:GSFC_20171208_Archive_e001955"]);
        let dumbbell = &found[0];
        assert_eq!(dumbbell.title, "Weighing in on the Dumbbell Nebula");
        assert_eq!(dumbbell.author, "NASA/JPL-Caltech/Harvard-Smithsonian CfA");
        assert_eq!(dumbbell.license, "Public domain (NASA)");
        assert_eq!(dumbbell.source_url, "https://images.nasa.gov/details/PIA14417");
        assert_eq!(dumbbell.url, "https://images-assets.nasa.gov/image/PIA14417/collection.json");
        assert_eq!((dumbbell.width, dumbbell.height), (Some(2800), Some(1400)));
        // No size for the original: taken on its previews' proportions, checked later.
        let lagoon = &found[2];
        assert_eq!((lagoon.width, lagoon.height), (None, None));
        assert_eq!(lagoon.author, "NASA");
    }

    #[test]
    fn the_largest_picture_of_a_nasa_item_is_the_original() {
        assert_eq!(
            nasa_largest(&fixture("nasa-collection.json")).as_deref(),
            Some("https://images-assets.nasa.gov/image/PIA14417/PIA14417~orig.jpg")
        );
        let tiff_only = serde_json::json!(["http://x/a~orig.tif", "http://x/a~large.jpg", "http://x/metadata.json"]);
        assert_eq!(nasa_largest(&tiff_only).as_deref(), Some("https://x/a~large.jpg"));
        assert_eq!(nasa_largest(&serde_json::json!({})), None);
    }

    #[test]
    fn credits_come_out_as_plain_text() {
        assert_eq!(
            plain_text(r#"<a href="//commons.wikimedia.org/wiki/User:Alchemist-hp" title="User:Alchemist-hp">Alchemist-hp</a> (<a rel="nofollow" class="external text" href="http://www.pse-mendelejew.de">www.pse-mendelejew.de</a>)"#),
            "Alchemist-hp (www.pse-mendelejew.de)"
        );
        assert_eq!(plain_text("Tom &amp; Jerry&#39;s &lt;sky&gt;\n  photo"), "Tom & Jerry's <sky> photo");
        assert_eq!(plain_text("A &unknown; B & C"), "A &unknown; B & C");
        assert_eq!(plain_text("&#x00E9;t&#233;"), "été");
    }

    #[test]
    fn picture_sizes_come_from_their_own_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&3840u32.to_be_bytes());
        png.extend_from_slice(&2160u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((3840, 2160, "png")));
        // SOI, an APP0 segment, then SOF0 with height 1440 and width 2560.
        let jpeg = [
            0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x05, 0xA0, 0x0A, 0x00,
        ];
        assert_eq!(image_size(&jpeg), Some((2560, 1440, "jpg")));
        assert_eq!(image_size(b"GIF89a"), None);
        assert_eq!(image_size(&[0xFF, 0xD8, 0x00]), None);
    }

    #[test]
    fn photo_bytes_come_only_from_the_photo_cache() {
        let base = std::env::temp_dir().join(format!("ciel-wallpaper-photos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("photos");
        std::fs::create_dir_all(root.join("space")).unwrap();
        let inside = root.join("space").join("nasa-PIA14417.jpg");
        std::fs::write(&inside, b"jpeg bytes").unwrap();
        let outside = base.join("secret.jpg");
        std::fs::write(&outside, b"not a photo").unwrap();
        std::fs::write(root.join("space").join("credits.json"), b"[]").unwrap();

        assert_eq!(bytes_in(&root, &inside.to_string_lossy()).unwrap(), b"jpeg bytes");
        let sneaky = root.join("space").join("..").join("..").join("secret.jpg");
        for refused in [
            outside.to_string_lossy().into_owned(),
            sneaky.to_string_lossy().into_owned(),
            root.join("space").join("credits.json").to_string_lossy().into_owned(),
            root.join("space").join("missing.jpg").to_string_lossy().into_owned(),
            root.to_string_lossy().into_owned(),
            "nasa-PIA14417.jpg".to_string(),
        ] {
            assert!(bytes_in(&root, &refused).is_err(), "{refused}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_theme_is_a_name_never_a_path() {
        assert!(theme_dir("space").is_ok());
        for bad in ["../space", "space/..", "", "Space", "C:\\x"] {
            assert!(theme_dir(bad).is_err(), "{bad}");
        }
        assert_eq!(file_stem("nasa:PIA14417"), "nasa-PIA14417");
        assert_eq!(file_stem("commons:54469472"), "commons-54469472");
        assert_eq!(file_stem("nasa:../x y"), "nasa----x-y");
    }

    #[test]
    fn credits_survive_a_save_and_photos_list_only_what_is_there() {
        let folder = std::env::temp_dir().join(format!("ciel-wallpaper-credits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let credit = Credit {
            file: "nasa-PIA14417.jpg".into(),
            id: "nasa:PIA14417".into(),
            title: "Weighing in on the Dumbbell Nebula".into(),
            author: "NASA/JPL-Caltech/Harvard-Smithsonian CfA".into(),
            license: "Public domain (NASA)".into(),
            source_url: "https://images.nasa.gov/details/PIA14417".into(),
            width: 2800,
            height: 1400,
        };
        save_credits(&folder, std::slice::from_ref(&credit)).unwrap();
        assert_eq!(load_credits(&folder), [credit.clone()]);
        let written: Value = serde_json::from_slice(&std::fs::read(folder.join("credits.json")).unwrap()).unwrap();
        assert_eq!(written[0]["sourceUrl"], "https://images.nasa.gov/details/PIA14417");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
