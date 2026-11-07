// The weather (Tools → Weather, the Today card, the sky): Open-Meteo's free
// forecast, no key and no account. Nothing is asked until the user picks a
// place, and then only Open-Meteo is: the place's coordinates, every 30
// minutes while one is set, or when the island asks for fresher.
//
// Rain warnings (off by default): when the next hour's quarter-hours say rain
// is about to start where it is dry now, a card says when, once per shower.
// Temperatures come in °C and wind in km/h; the island converts.

use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::notify::{self, Notification, Target};

const EVENT: &str = "weather";
const FRESH_MS: f64 = 30.0 * 60_000.0;
const TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = concat!("Ciel/", env!("CARGO_PKG_VERSION"), " (+https://github.com/jean-eudes-asg/ciel)");
/// Wet enough to call it rain, in mm per quarter-hour.
const RAIN_MM: f64 = 0.2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    /// "Auvergne-Rhône-Alpes, France".
    pub detail: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Now {
    pub temp: f64,
    pub feels: f64,
    pub code: i64,
    pub is_day: bool,
    pub wind: f64,
    pub humidity: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Hour {
    /// Unix ms.
    pub t: f64,
    pub temp: f64,
    pub code: i64,
    /// Chance of rain, in %.
    pub pop: f64,
    pub is_day: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    /// Unix ms of the day's start, where the place is.
    pub t: f64,
    pub code: i64,
    pub max: f64,
    pub min: f64,
    pub pop: f64,
    pub sunrise: f64,
    pub sunset: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Weather {
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    /// Unix ms: when Open-Meteo answered.
    pub at: f64,
    pub now: Now,
    pub hours: Vec<Hour>,
    pub days: Vec<Day>,
    /// Rain starts then (Unix ms), within the next two hours, where it is dry now.
    pub rain_at: Option<f64>,
    /// It rains now; it stops then, if it does within two hours.
    pub raining: bool,
    pub rain_until: Option<f64>,
}

struct Cache {
    weather: Option<Weather>,
    /// The shower already announced: its start.
    warned: Option<f64>,
}

static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(|| Mutex::new(Cache { weather: None, warned: None }));
static LOOP: OnceLock<()> = OnceLock::new();

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(10))
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())
}

async fn get_json(url: Url) -> Result<Value, String> {
    let response = client()?.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let body = crate::net::read_capped(response, 2 * 1024 * 1024).await?;
    serde_json::from_slice(&body).map_err(|e| e.to_string())
}

// ── Reading Open-Meteo's answers ──────────────────────────────────────────────

pub fn parse_places(v: &Value) -> Vec<Place> {
    let Some(list) = v.get("results").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|r| {
            let name = r.get("name")?.as_str()?.to_string();
            let lat = r.get("latitude")?.as_f64()?;
            let lon = r.get("longitude")?.as_f64()?;
            let parts: Vec<&str> = ["admin1", "country"].iter().filter_map(|k| r.get(*k).and_then(Value::as_str)).collect();
            Some(Place { name, detail: parts.join(", "), lat, lon })
        })
        .collect()
}

fn nums(v: &Value, block: &str, key: &str) -> Vec<f64> {
    v.get(block)
        .and_then(|b| b.get(key))
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default()
}

fn at(list: &[f64], i: usize) -> f64 {
    list.get(i).copied().filter(|x| x.is_finite()).unwrap_or(0.0)
}

/// The forecast, with `timeformat=unixtime`; `now` (Unix ms) cuts the past hours.
pub fn parse_forecast(v: &Value, place: &str, lat: f64, lon: f64, now: f64) -> Option<Weather> {
    let current = v.get("current")?;
    let num = |k: &str| current.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let now_block = Now {
        temp: num("temperature_2m"),
        feels: num("apparent_temperature"),
        code: num("weather_code") as i64,
        is_day: num("is_day") >= 1.0,
        wind: num("wind_speed_10m"),
        humidity: num("relative_humidity_2m"),
    };
    let (ht, htemp, hcode, hpop, hday) = (
        nums(v, "hourly", "time"),
        nums(v, "hourly", "temperature_2m"),
        nums(v, "hourly", "weather_code"),
        nums(v, "hourly", "precipitation_probability"),
        nums(v, "hourly", "is_day"),
    );
    let hours = (0..ht.len())
        .map(|i| Hour { t: at(&ht, i) * 1000.0, temp: at(&htemp, i), code: at(&hcode, i) as i64, pop: at(&hpop, i), is_day: at(&hday, i) >= 1.0 })
        // The hour that has begun stays, so "now" has its column.
        .filter(|h| h.t + 3_600_000.0 > now)
        .take(24)
        .collect();
    let (dt, dcode, dmax, dmin, dpop, rise, set) = (
        nums(v, "daily", "time"),
        nums(v, "daily", "weather_code"),
        nums(v, "daily", "temperature_2m_max"),
        nums(v, "daily", "temperature_2m_min"),
        nums(v, "daily", "precipitation_probability_max"),
        nums(v, "daily", "sunrise"),
        nums(v, "daily", "sunset"),
    );
    let days = (0..dt.len())
        .map(|i| Day {
            t: at(&dt, i) * 1000.0,
            code: at(&dcode, i) as i64,
            max: at(&dmax, i),
            min: at(&dmin, i),
            pop: at(&dpop, i),
            sunrise: at(&rise, i) * 1000.0,
            sunset: at(&set, i) * 1000.0,
        })
        .collect();
    let (qt, qmm) = (nums(v, "minutely_15", "time"), nums(v, "minutely_15", "precipitation"));
    let quarters: Vec<(f64, f64)> = (0..qt.len()).map(|i| (at(&qt, i) * 1000.0, at(&qmm, i))).collect();
    let (raining, rain_at, rain_until) = rain(&quarters, now);
    Some(Weather {
        place: place.to_string(),
        lat,
        lon,
        at: now,
        now: now_block,
        hours,
        days,
        rain_at,
        raining,
        rain_until,
    })
}

/// From the quarter-hours (start in Unix ms, mm): whether it rains now, when
/// rain starts within two hours if it doesn't, and when it stops if it does.
pub fn rain(quarters: &[(f64, f64)], now: f64) -> (bool, Option<f64>, Option<f64>) {
    let ahead: Vec<&(f64, f64)> = quarters.iter().filter(|(t, _)| *t + 15.0 * 60_000.0 > now && *t < now + 2.0 * 3_600_000.0).collect();
    let Some(first) = ahead.first() else { return (false, None, None) };
    let raining = first.1 >= RAIN_MM;
    if raining {
        let until = ahead.iter().find(|(_, mm)| *mm < RAIN_MM).map(|(t, _)| *t);
        (true, None, until)
    } else {
        let start = ahead.iter().find(|(_, mm)| *mm >= RAIN_MM).map(|(t, _)| t.max(now));
        (false, start, None)
    }
}

// ── Asking ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn weather_search(query: String, language: String) -> Result<Vec<Place>, String> {
    let query = query.trim();
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let mut url = Url::parse("https://geocoding-api.open-meteo.com/v1/search").map_err(|e| e.to_string())?;
    let lang: String = language.chars().take(2).filter(char::is_ascii_alphabetic).collect();
    url.query_pairs_mut()
        .append_pair("name", query)
        .append_pair("count", "6")
        .append_pair("format", "json")
        .append_pair("language", if lang.is_empty() { "en" } else { &lang });
    Ok(parse_places(&get_json(url).await?))
}

async fn fetch(place: &str, lat: f64, lon: f64) -> Result<Weather, String> {
    let mut url = Url::parse("https://api.open-meteo.com/v1/forecast").map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("latitude", &format!("{lat:.4}"))
        .append_pair("longitude", &format!("{lon:.4}"))
        .append_pair("current", "temperature_2m,apparent_temperature,weather_code,is_day,wind_speed_10m,relative_humidity_2m")
        .append_pair("hourly", "temperature_2m,weather_code,precipitation_probability,is_day")
        .append_pair("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset")
        .append_pair("minutely_15", "precipitation")
        .append_pair("forecast_minutely_15", "12")
        .append_pair("forecast_hours", "26")
        .append_pair("forecast_days", "7")
        .append_pair("timezone", "auto")
        .append_pair("timeformat", "unixtime");
    let v = get_json(url).await?;
    parse_forecast(&v, place, lat, lon, now_ms()).ok_or_else(|| "no forecast".to_string())
}

fn place_of(app: &AppHandle) -> Option<(String, f64, f64, bool)> {
    let shared = app.state::<crate::Shared>();
    let s = shared.settings.lock().unwrap();
    let t = &s.tools;
    (!t.weather_place.is_empty()).then(|| (t.weather_place.clone(), t.weather_lat, t.weather_lon, t.rain_warnings))
}

/// Asks Open-Meteo unless what is kept is fresh (and for the same place).
async fn refresh(app: &AppHandle, force: bool) -> Option<Weather> {
    let (place, lat, lon, warnings) = place_of(app)?;
    let kept = CACHE.lock().unwrap().weather.clone();
    if let Some(w) = kept.as_ref() {
        let same = w.place == place && (w.lat - lat).abs() < 1e-6 && (w.lon - lon).abs() < 1e-6;
        if same && !force && now_ms() - w.at < FRESH_MS {
            return kept;
        }
    }
    match fetch(&place, lat, lon).await {
        Ok(w) => {
            CACHE.lock().unwrap().weather = Some(w.clone());
            let _ = app.emit(EVENT, &w);
            if warnings {
                warn_of_rain(app, &w);
            }
            Some(w)
        }
        Err(err) => {
            crate::log::line(format!("weather: {err}"));
            kept
        }
    }
}

fn warn_of_rain(app: &AppHandle, w: &Weather) {
    let Some(start) = w.rain_at else { return };
    // Within the hour, and not the shower already announced.
    if start - now_ms() > 3_600_000.0 {
        return;
    }
    {
        let mut cache = CACHE.lock().unwrap();
        if cache.warned.is_some_and(|t| (t - start).abs() < 45.0 * 60_000.0) {
            return;
        }
        cache.warned = Some(start);
    }
    use crate::i18n::{t, tf};
    let minutes = (((start - now_ms()) / 60_000.0).round().max(0.0) as u64).to_string();
    notify::show(
        app,
        Notification {
            id: notify::next_id("weather"),
            source: "weather".into(),
            app: t("Weather"),
            title: t("Rain is coming"),
            body: tf("In about {minutes} min in {place}.", &[("minutes", &minutes), ("place", &w.place)]),
            icon: None,
            at: now_ms(),
            tag: Some("weather-rain".into()),
            buttons: Vec::new(),
            until: None,
            time: None,
        },
        Target::None,
    );
}

/// The weather, from what is kept or asked again (`force`: the user pressed refresh).
#[tauri::command]
pub async fn weather_now(app: AppHandle, force: bool) -> Option<Weather> {
    refresh(&app, force).await
}

/// The place changed (settings saved): what was kept for another one goes.
pub fn settings_saved(app: &AppHandle, place: &str) {
    let stale = CACHE.lock().unwrap().weather.as_ref().is_some_and(|w| w.place != place);
    if stale || place.is_empty() {
        let mut cache = CACHE.lock().unwrap();
        cache.weather = None;
        cache.warned = None;
    }
    if !place.is_empty() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            refresh(&app, false).await;
        });
    }
}

/// Every 30 minutes, while a place is set: one small request.
pub fn start(app: &AppHandle) {
    if LOOP.set(()).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app, false).await;
            // Quicker while rain warnings watch an hour that may change.
            let warnings = place_of(&app).is_some_and(|p| p.3);
            tokio::time::sleep(Duration::from_secs(if warnings { 15 * 60 } else { 30 * 60 })).await;
        }
    });
}

/// The weather kept, if any.
pub fn kept() -> Option<Weather> {
    CACHE.lock().unwrap().weather.clone()
}

/// WMO codes in words, for the brain (it says them in the user's language).
fn words(code: i64) -> &'static str {
    match code {
        0 | 1 => "clear",
        2 => "partly cloudy",
        3 => "cloudy",
        45 | 48 => "fog",
        51..=57 => "drizzle",
        61 | 80 => "light rain",
        63 | 81 => "rain",
        65 | 82 | 66 | 67 => "heavy rain",
        71..=77 | 85 | 86 => "snow",
        95..=99 => "thunderstorm",
        _ => "cloudy",
    }
}

/// The brain's `weather` tool: the forecast already kept for the place the
/// user picked. Nothing more is asked of anyone.
pub fn for_brain() -> String {
    let Some(w) = kept() else {
        return "No weather here: the user hasn't picked a place in the island's Tools → Weather. Say they can.".into();
    };
    let hm = |ms: f64| {
        chrono::DateTime::from_timestamp_millis(ms as i64)
            .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_default()
    };
    let hours: Vec<Value> = w.hours.iter().take(12).map(|h| serde_json::json!({ "at": hm(h.t), "c": h.temp.round(), "sky": words(h.code), "rainChance": h.pop })).collect();
    let days: Vec<Value> = w.days.iter().take(7).map(|d| serde_json::json!({
        "date": chrono::DateTime::from_timestamp_millis(d.t as i64 + 12 * 3_600_000).map(|x| x.format("%a %Y-%m-%d").to_string()).unwrap_or_default(),
        "sky": words(d.code), "maxC": d.max.round(), "minC": d.min.round(), "rainChance": d.pop,
    })).collect();
    serde_json::json!({
        "place": w.place,
        "now": { "c": w.now.temp.round(), "feelsC": w.now.feels.round(), "sky": words(w.now.code), "windKmh": w.now.wind.round(), "humidity": w.now.humidity },
        "rainStartsAt": w.rain_at.map(hm),
        "raining": w.raining,
        "rainStopsAt": w.rain_until.map(hm),
        "nextHours": hours,
        "days": days,
        "note": "Temperatures in Celsius; say them in the unit the user's country uses.",
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn places_read_with_their_region_and_country() {
        let v = json!({"results": [
            {"name": "Lyon", "latitude": 45.75, "longitude": 4.85, "admin1": "Auvergne-Rhône-Alpes", "country": "France"},
            {"name": "Lyons", "latitude": 43.1, "longitude": -77.0, "country": "United States"},
            {"name": "broken"}
        ]});
        let places = parse_places(&v);
        assert_eq!(places.len(), 2);
        assert_eq!(places[0].detail, "Auvergne-Rhône-Alpes, France");
        assert_eq!(places[1].detail, "United States");
        assert!(parse_places(&json!({})).is_empty());
    }

    #[test]
    fn rain_starts_or_stops_within_two_hours() {
        let q = |mm: &[f64]| mm.iter().enumerate().map(|(i, m)| (i as f64 * 900_000.0, *m)).collect::<Vec<_>>();
        assert_eq!(rain(&q(&[0.0, 0.0, 0.5, 1.0]), 0.0), (false, Some(1_800_000.0), None));
        assert_eq!(rain(&q(&[0.6, 0.4, 0.0]), 0.0), (true, None, Some(1_800_000.0)));
        assert_eq!(rain(&q(&[0.0, 0.1, 0.0]), 0.0), (false, None, None));
        // Rain already started in the quarter we're in: it rains now.
        assert_eq!(rain(&q(&[0.0, 0.5, 0.5]), 1_000_000.0), (true, None, None));
    }

    #[test]
    fn a_forecast_reads_into_now_hours_and_days() {
        let v = json!({
            "current": {"temperature_2m": 12.3, "apparent_temperature": 10.1, "weather_code": 61, "is_day": 1,
                        "wind_speed_10m": 14.0, "relative_humidity_2m": 80},
            "hourly": {"time": [0, 3600, 7200], "temperature_2m": [11, 12, 13], "weather_code": [3, 61, 61],
                       "precipitation_probability": [10, 80, 90], "is_day": [1, 1, 0]},
            "daily": {"time": [0], "weather_code": [61], "temperature_2m_max": [14], "temperature_2m_min": [7],
                      "precipitation_probability_max": [90], "sunrise": [25200], "sunset": [61200]},
            "minutely_15": {"time": [3600, 4500], "precipitation": [0.0, 0.4]}
        });
        let w = parse_forecast(&v, "Lyon", 45.75, 4.85, 3_700_000.0).unwrap();
        assert_eq!(w.now.code, 61);
        assert!(w.now.is_day);
        // The hour that has begun is the first one kept.
        assert_eq!(w.hours.len(), 2);
        assert_eq!(w.hours[0].t, 3_600_000.0);
        assert_eq!(w.days[0].sunset, 61_200_000.0);
        assert_eq!(w.rain_at, Some(4_500_000.0));
    }
}
