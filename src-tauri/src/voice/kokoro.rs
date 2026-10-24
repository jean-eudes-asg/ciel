// Kokoro: « OK Ciel »'s answers in a natural voice, made on this computer
// without the internet. Kokoro v1.0 (82M parameters, Apache-2.0) through the
// sherpa-onnx that already runs Parakeet: lively English voices, and French
// with its own. Downloaded on a click (pack.rs), loaded only while « OK Ciel »
// answers with it. Full precision: its int8 version took three to four times
// longer on an i7-13700HX (1.3 s against 4 to 7 s for 4 s of speech).

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKokoroModelConfig, OfflineTtsModelConfig};
use tauri::AppHandle;

use super::pack::{self, Pack};
use crate::speech::ModelState;

pub static KOKORO: Pack = Pack {
    name: "Kokoro",
    id: "kokoro-multi-lang-v1_0",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2",
    size: 349_906_910,
    sha256: "c5f7e2d2caf082bc1d20fb70334a61d99d20b484500aad32e7cf84c128ea3298",
    model: "model.onnx",
    model_size: 325_560_556,
    needs: &["voices.bin", "tokens.txt", "lexicon-us-en.txt", "lexicon-zh.txt", "dict", "espeak-ng-data/phontab"],
};

/// The English voices offered, with their speaker ID in the model: the ones
/// Kokoro's own notes rate best, women's then men's. The first is the default.
pub const VOICES: &[(&str, i32)] = &[
    ("af_heart", 3),
    ("af_bella", 2),
    ("af_nicole", 6),
    ("am_michael", 16),
    ("am_fenrir", 14),
    ("am_puck", 18),
];
/// Its French voice, for an answer in French.
const FRENCH: i32 = 30; // ff_siwis

pub fn speaker(voice: &str) -> i32 {
    VOICES.iter().find(|(name, _)| *name == voice).map_or(VOICES[0].1, |(_, id)| *id)
}

pub fn is_present() -> bool {
    KOKORO.is_present()
}

pub fn download_mb() -> u64 {
    KOKORO.download_mb()
}

pub fn state() -> ModelState {
    KOKORO.state()
}

pub fn download(app: AppHandle) {
    KOKORO.download(app);
}

pub fn remove(app: &AppHandle) {
    KOKORO.remove(app, unload);
}

// ── Speaking ──────────────────────────────────────────────────────────────────

/// Kokoro is ten times Kitten's size: half the cores, up to 8 (speech
/// recognition keeps its own 4).
fn threads() -> i32 {
    std::thread::available_parallelism().map(|n| (n.get() / 2).clamp(2, 8) as i32).unwrap_or(4)
}

/// "en" and, once a French answer came, "fr": one engine each.
fn engines() -> &'static Mutex<Vec<(&'static str, Arc<OfflineTts>)>> {
    static E: OnceLock<Mutex<Vec<(&'static str, Arc<OfflineTts>)>>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(Vec::new()))
}

/// The model in `dir` for `lang`: English (with Chinese, which the same
/// dictionaries hold) by its lexicons, French by espeak-ng.
fn load(dir: &Path, model: &str, lang: &'static str, threads: i32) -> Option<OfflineTts> {
    let path = |name: &str| Some(dir.join(name).to_string_lossy().to_string());
    let kokoro = if lang == "fr" {
        OfflineTtsKokoroModelConfig { lang: Some("fr".into()), ..Default::default() }
    } else {
        OfflineTtsKokoroModelConfig {
            lexicon: Some(format!("{},{}", path("lexicon-us-en.txt")?, path("lexicon-zh.txt")?)),
            dict_dir: path("dict"),
            ..Default::default()
        }
    };
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kokoro: OfflineTtsKokoroModelConfig {
                model: path(model),
                voices: path("voices.bin"),
                tokens: path("tokens.txt"),
                data_dir: path("espeak-ng-data"),
                ..kokoro
            },
            num_threads: threads,
            ..Default::default()
        },
        max_num_sentences: 1,
        ..Default::default()
    };
    OfflineTts::create(&config)
}

/// The engine for `lang`, loaded on first use (about a second).
fn engine(lang: &'static str) -> Option<Arc<OfflineTts>> {
    let mut all = engines().lock().unwrap();
    if let Some((_, e)) = all.iter().find(|(l, _)| *l == lang) {
        return Some(e.clone());
    }
    if !is_present() {
        return None;
    }
    let started = Instant::now();
    let e = load(&KOKORO.dir(), KOKORO.model, lang, threads()).map(Arc::new);
    match &e {
        Some(e) => {
            crate::log::line(format!("Kokoro ({lang}) loaded in {:?}", started.elapsed()));
            all.push((lang, e.clone()));
        }
        None => crate::log::line(format!("Kokoro ({lang}) could not be loaded")),
    }
    e
}

/// Loaded while « OK Ciel » answers with it, freed otherwise.
pub fn sync(wanted: bool) {
    if !wanted {
        unload();
    } else if is_present() && engines().lock().unwrap().is_empty() {
        std::thread::spawn(|| {
            let _ = engine("en");
        });
    }
}

pub fn unload() {
    engines().lock().unwrap().clear();
}

/// `text` said by Kokoro in `voice` (its French one for French), as a WAV;
/// None and the system voice speaks.
pub fn say(text: &str, voice: &str) -> Option<Vec<u8>> {
    let french = super::wake::speech_lang(text).is_some();
    let (lang, sid) = if french { ("fr", FRENCH) } else { ("en", speaker(voice)) };
    let tts = engine(lang)?;
    generate(&tts, text, sid)
}

fn generate(tts: &OfflineTts, text: &str, sid: i32) -> Option<Vec<u8>> {
    let started = Instant::now();
    let audio = tts.generate_with_config(text, &GenerationConfig { sid, ..Default::default() }, None::<fn(&[f32], f32) -> bool>)?;
    let samples = audio.samples();
    let rate = audio.sample_rate().max(1) as u32;
    crate::log::line(format!("voice: Kokoro said {:.1} s in {:?}", samples.len() as f64 / rate as f64, started.elapsed()));
    (!samples.is_empty()).then(|| pack::wav(samples, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_voice_is_found_by_name_and_an_unknown_one_is_heart() {
        assert_eq!(speaker("am_michael"), 16);
        assert_eq!(speaker("af_heart"), 3);
        assert_eq!(speaker("nobody"), 3);
        assert_eq!(speaker(""), 3);
        assert!(VOICES.iter().all(|(n, _)| n.starts_with("af_") || n.starts_with("am_")));
    }
}

#[cfg(test)]
mod with_model {
    /// CIEL_TEST_KOKORO: a folder holding the unpacked archive. Says an English
    /// answer in each voice and a French one, and times them; CIEL_TEST_KOKORO_OUT
    /// keeps the WAVs to listen to.
    #[test]
    #[ignore]
    fn says_english_and_french_in_time() {
        let dir = std::path::PathBuf::from(std::env::var("CIEL_TEST_KOKORO").expect("CIEL_TEST_KOKORO"));
        let out = std::env::var("CIEL_TEST_KOKORO_OUT").ok();
        let model = std::env::var("CIEL_TEST_KOKORO_MODEL").unwrap_or_else(|_| super::KOKORO.model.into());
        // CIEL_TEST_KOKORO_THREADS: how fast it gets with more cores.
        for threads in std::env::var("CIEL_TEST_KOKORO_THREADS").ok().map(|t| t.split(',').filter_map(|n| n.parse().ok()).collect::<Vec<i32>>()).unwrap_or_default() {
            let en = super::load(&dir, &model, "en", threads).expect("the English engine");
            let _ = super::generate(&en, "Warming up.", 3);
            let started = std::time::Instant::now();
            let wav = super::generate(&en, "It's a quarter to six. Your next meeting is the design review at six thirty.", 3).expect("audio");
            println!("{threads} threads: {:.1} s of speech in {:?}", super::pack::wav_seconds(&wav), started.elapsed());
        }
        let started = std::time::Instant::now();
        let en = super::load(&dir, &model, "en", super::threads()).expect("the English engine");
        println!("en loaded in {:?}", started.elapsed());
        for (name, sid) in super::VOICES {
            let started = std::time::Instant::now();
            let wav = super::generate(&en, "It's a quarter to six. Your next meeting is the design review at six thirty.", *sid).expect("audio");
            let secs = super::pack::wav_seconds(&wav);
            println!("{name}: {secs:.1} s of speech in {:?}", started.elapsed());
            assert!(secs > 2.0 && secs < 12.0);
            if let Some(out) = &out {
                std::fs::write(format!("{out}/kokoro-{name}.wav"), &wav).unwrap();
            }
        }
        let started = std::time::Instant::now();
        let fr = super::load(&dir, &model, "fr", super::threads()).expect("the French engine");
        let wav = super::generate(&fr, "Il est six heures moins le quart. Ta prochaine réunion est à six heures et demie.", super::FRENCH).expect("audio");
        println!("fr: {:.1} s of speech in {:?} (with loading)", super::pack::wav_seconds(&wav), started.elapsed());
        if let Some(out) = &out {
            std::fs::write(format!("{out}/kokoro-ff_siwis.wav"), &wav).unwrap();
        }
    }
}
