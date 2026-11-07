// The words of the song playing (the music pill's card, and a line in the
// folded island), from LRCLIB (lrclib.net): free, no key, lyrics timed to
// the song where it has them. Off until the user turns it on (Tools →
// Music, or Settings); then only the song's title, artist, album and length
// are sent, once a song, and the answer is kept in memory while Ciel runs.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use reqwest::Url;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

const USER_AGENT: &str = concat!("Ciel/", env!("CARGO_PKG_VERSION"), " (+https://github.com/jean-eudes-asg/ciel)");
const KEPT: usize = 200;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    /// LRC: "[01:02.30] a line", one a line.
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub instrumental: bool,
}

static CACHE: LazyLock<Mutex<HashMap<String, Option<Lyrics>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(title: &str, artist: &str) -> String {
    format!("{}\u{1f}{}", title.trim().to_lowercase(), artist.trim().to_lowercase())
}

pub fn from_json(v: &Value) -> Option<Lyrics> {
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let lyrics = Lyrics {
        synced: text("syncedLyrics"),
        plain: text("plainLyrics"),
        instrumental: v.get("instrumental").and_then(Value::as_bool).unwrap_or(false),
    };
    (lyrics.synced.is_some() || lyrics.plain.is_some() || lyrics.instrumental).then_some(lyrics)
}

async fn ask(title: &str, artist: &str, album: &str, duration: f64) -> Result<Option<Lyrics>, String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    // The exact song first…
    let mut url = Url::parse("https://lrclib.net/api/get").map_err(|e| e.to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("track_name", title).append_pair("artist_name", artist);
        if !album.is_empty() {
            q.append_pair("album_name", album);
        }
        if duration > 0.0 {
            q.append_pair("duration", &format!("{}", duration.round() as u64));
        }
    }
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if response.status().is_success() {
        let body = crate::net::read_capped(response, 512 * 1024).await?;
        let v: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
        if let Some(l) = from_json(&v) {
            return Ok(Some(l));
        }
    }
    // …else a search, the best match with timed words, else any.
    let mut url = Url::parse("https://lrclib.net/api/search").map_err(|e| e.to_string())?;
    url.query_pairs_mut().append_pair("track_name", title).append_pair("artist_name", artist);
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body = crate::net::read_capped(response, 2 * 1024 * 1024).await?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
    let list = v.as_array().cloned().unwrap_or_default();
    let found: Vec<Lyrics> = list.iter().filter_map(from_json).collect();
    Ok(found.iter().find(|l| l.synced.is_some()).or(found.first()).cloned())
}

/// The words of a song, once the user has turned lyrics on.
#[tauri::command]
pub async fn lyrics_get(app: AppHandle, title: String, artist: String, album: String, duration: f64) -> Option<Lyrics> {
    let on = app.state::<crate::Shared>().settings.lock().unwrap().tools.lyrics;
    if !on || title.trim().is_empty() {
        return None;
    }
    let k = key(&title, &artist);
    if let Some(kept) = CACHE.lock().unwrap().get(&k) {
        return kept.clone();
    }
    match ask(&title, &artist, &album, duration).await {
        Ok(found) => {
            let mut cache = CACHE.lock().unwrap();
            if cache.len() >= KEPT {
                cache.clear();
            }
            cache.insert(k, found.clone());
            found
        }
        Err(err) => {
            // Not kept: a network hiccup may be gone next time.
            crate::log::line(format!("lyrics: {err}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_answer_with_words_or_none_reads_as_such() {
        let l = from_json(&json!({"syncedLyrics": "[00:01.00] Hello", "plainLyrics": "Hello", "instrumental": false})).unwrap();
        assert_eq!(l.synced.as_deref(), Some("[00:01.00] Hello"));
        assert!(from_json(&json!({"instrumental": true})).unwrap().instrumental);
        assert_eq!(from_json(&json!({"syncedLyrics": null, "plainLyrics": "  "})), None);
    }

    #[test]
    fn songs_are_kept_whatever_their_case() {
        assert_eq!(key(" Song ", "Artist"), key("song", "ARTIST "));
    }
}
