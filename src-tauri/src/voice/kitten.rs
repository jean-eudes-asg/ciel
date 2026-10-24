// Kitten: « OK Ciel »'s answers said out loud on this computer, without the
// internet. KittenML's nano model (15M parameters, Apache-2.0, English only)
// through the sherpa-onnx that already runs Parakeet (speech/mod.rs).
//
// Downloaded on a click, checked and unpacked by pack.rs. An answer in
// another language than English goes to the system voice.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKittenModelConfig, OfflineTtsModelConfig};
use tauri::AppHandle;

use super::pack::{self, Pack};
use crate::speech::{self, ModelState};

pub static KITTEN: Pack = Pack {
    name: "Kitten",
    id: "kitten-nano-en-v0_8-fp32",
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kitten-nano-en-v0_8-fp32.tar.bz2",
    size: 63_815_222,
    sha256: "16092117bfe591ddcd58d078e1454603b8e1caea46f85653b2c2efae76bd883e",
    model: "model.fp32.onnx",
    model_size: 56_768_002,
    needs: &["voices.bin", "tokens.txt", "espeak-ng-data/phontab"],
};

/// Two of its eight voices that speak at an even pace (about 160 words a
/// minute) at speed 1: expr-voice-4-f and expr-voice-3-m.
const FEMALE: i32 = 5;
const MALE: i32 = 2;

pub fn is_present() -> bool {
    KITTEN.is_present()
}

pub fn download_mb() -> u64 {
    KITTEN.download_mb()
}

pub fn state() -> ModelState {
    KITTEN.state()
}

pub fn download(app: AppHandle) {
    KITTEN.download(app);
}

pub fn remove(app: &AppHandle) {
    KITTEN.remove(app, unload);
}

// ── Speaking ──────────────────────────────────────────────────────────────────

fn engine_slot() -> &'static Mutex<Option<Arc<OfflineTts>>> {
    static E: OnceLock<Mutex<Option<Arc<OfflineTts>>>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(None))
}

/// The loaded model, loading it on first use (under a second).
fn engine() -> Option<Arc<OfflineTts>> {
    let mut slot = engine_slot().lock().unwrap();
    if let Some(e) = slot.clone() {
        return Some(e);
    }
    if !is_present() {
        return None;
    }
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kitten: OfflineTtsKittenModelConfig {
                model: Some(KITTEN.path(KITTEN.model)),
                voices: Some(KITTEN.path("voices.bin")),
                tokens: Some(KITTEN.path("tokens.txt")),
                data_dir: Some(KITTEN.path("espeak-ng-data")),
                ..Default::default()
            },
            num_threads: speech::threads(),
            ..Default::default()
        },
        max_num_sentences: 1,
        ..Default::default()
    };
    let started = Instant::now();
    let e = OfflineTts::create(&config).map(Arc::new);
    match &e {
        Some(_) => crate::log::line(format!("Kitten loaded in {:?}", started.elapsed())),
        None => crate::log::line("Kitten could not be loaded"),
    }
    *slot = e.clone();
    e
}

/// Loaded while « OK Ciel » answers with it, freed otherwise.
pub fn sync(wanted: bool) {
    if !wanted {
        unload();
    } else if is_present() && engine_slot().lock().unwrap().is_none() {
        std::thread::spawn(|| {
            let _ = engine();
        });
    }
}

pub fn unload() {
    *engine_slot().lock().unwrap() = None;
}

/// Kitten speaks English only: French (wake.rs) or letters outside ASCII go to
/// the system voice.
pub fn can_say(text: &str) -> bool {
    super::wake::speech_lang(text).is_none() && text.chars().all(|c| c.is_ascii() || !c.is_alphabetic())
}

/// `text` said by Kitten in the voice of `gender`, as a WAV; None and the system voice speaks.
pub fn say(text: &str, gender: &str) -> Option<Vec<u8>> {
    if !can_say(text) {
        return None;
    }
    let tts = engine()?;
    let config = GenerationConfig { sid: if gender == "male" { MALE } else { FEMALE }, ..Default::default() };
    let started = Instant::now();
    let audio = tts.generate_with_config(text, &config, None::<fn(&[f32], f32) -> bool>)?;
    let samples = audio.samples();
    let rate = audio.sample_rate().max(1) as u32;
    crate::log::line(format!(
        "voice: Kitten said {:.1} s in {:?}",
        samples.len() as f64 / rate as f64,
        started.elapsed()
    ));
    (!samples.is_empty()).then(|| pack::wav(samples, rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitten_says_english_and_leaves_the_rest_to_the_system_voice() {
        assert!(can_say("Your build finished, and two tests failed."));
        assert!(can_say("Done."));
        assert!(can_say("It’s 3:45 — want me to rerun them?"));
        assert!(!can_say("Ta compilation est terminée, et deux tests ont échoué."));
        assert!(!can_say("C'est fait pour toi"));
        assert!(!can_say("La compilación terminó."));
        assert!(!can_say("Сборка завершена."));
        assert!(!can_say("构建完成"));
    }
}

#[cfg(test)]
mod with_model {
    /// Downloads the real archive (64 MB) into XDG_DATA_HOME's ciel/Models,
    /// checks it and unpacks it, as the Download button does.
    #[test]
    #[ignore]
    fn downloads_and_unpacks_the_real_archive() {
        let _ = std::fs::remove_dir_all(super::KITTEN.dir());
        let steps = std::sync::atomic::AtomicUsize::new(0);
        tauri::async_runtime::block_on(super::KITTEN.fetch_and_unpack(|_| {
            steps.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }))
        .unwrap();
        assert!(super::is_present());
        assert!(steps.into_inner() > 0);
        assert!(!super::speech::models_dir().join(format!("{}.tar.bz2.part", super::KITTEN.id)).exists());
    }

    /// Needs the voice downloaded (OK Ciel → Kitten → Download), or
    /// XDG_DATA_HOME pointing at a folder holding ciel/Models/<ID>.
    /// CIEL_TEST_KITTEN_OUT keeps the WAVs to listen to.
    #[test]
    #[ignore]
    fn says_an_answer_in_both_voices() {
        for gender in ["female", "male"] {
            let wav = super::say("Your build finished, and two tests failed in the printers module.", gender).expect("the voice");
            let secs = super::pack::wav_seconds(&wav);
            println!("{gender}: {secs:.1} s");
            assert!(secs > 2.0 && secs < 8.0);
            if let Ok(dir) = std::env::var("CIEL_TEST_KITTEN_OUT") {
                std::fs::write(format!("{dir}/kitten-{gender}.wav"), &wav).unwrap();
            }
        }
        assert!(super::say("Ta compilation est terminée.", "female").is_none());
    }
}
