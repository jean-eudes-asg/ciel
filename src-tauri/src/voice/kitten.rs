// Kitten: « OK Ciel »'s answers said out loud on this computer, without the
// internet. KittenML's nano model (15M parameters, Apache-2.0, English only)
// through the sherpa-onnx that already runs Parakeet (speech/mod.rs).
//
// Nothing is downloaded until the user clicks Download in Ciel → OK Ciel. The
// model comes as one archive from sherpa-onnx's releases, is checked against
// its SHA-256 and unpacked next to Parakeet in the Models folder. An answer in
// another language than English goes to the system voice.

use std::io::{BufReader, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKittenModelConfig, OfflineTtsModelConfig};
use tauri::AppHandle;

use crate::speech::{self, ModelState};

const ARCHIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kitten-nano-en-v0_8-fp32.tar.bz2";
const ARCHIVE_SIZE: u64 = 63_815_222;
const ARCHIVE_SHA256: &str = "16092117bfe591ddcd58d078e1454603b8e1caea46f85653b2c2efae76bd883e";
/// The folder the archive unpacks to, kept under the same name in Models.
const ID: &str = "kitten-nano-en-v0_8-fp32";
const MODEL: &str = "model.fp32.onnx";
const MODEL_SIZE: u64 = 56_768_002;

/// Two of its eight voices that speak at an even pace (about 160 words a
/// minute) at speed 1: expr-voice-4-f and expr-voice-3-m.
const FEMALE: i32 = 5;
const MALE: i32 = 2;

fn dir() -> std::path::PathBuf {
    speech::models_dir().join(ID)
}

fn path(name: &str) -> String {
    dir().join(name).to_string_lossy().to_string()
}

/// The model is there with its size (the archive's hash was checked when it came in).
pub fn is_present() -> bool {
    let d = dir();
    std::fs::metadata(d.join(MODEL)).is_ok_and(|m| m.len() == MODEL_SIZE)
        && d.join("voices.bin").is_file()
        && d.join("tokens.txt").is_file()
        && d.join("espeak-ng-data").join("phontab").is_file()
}

pub fn download_mb() -> u64 {
    ARCHIVE_SIZE / 1_000_000
}

// ── Download ──────────────────────────────────────────────────────────────────

struct Download {
    running: bool,
    fraction: f64,
    error: Option<String>,
}

fn download_state() -> &'static Mutex<Download> {
    static D: OnceLock<Mutex<Download>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Download { running: false, fraction: 0.0, error: None }))
}

/// What the OK Ciel page shows about the voice.
pub fn state() -> ModelState {
    let d = download_state().lock().unwrap();
    if d.running {
        ModelState::Downloading { fraction: d.fraction }
    } else if is_present() {
        ModelState::Ready
    } else if let Some(error) = &d.error {
        ModelState::Failed { error: error.clone() }
    } else {
        ModelState::Missing
    }
}

/// Downloads and unpacks the voice, with the progress in `speech-models` events.
pub fn download(app: AppHandle) {
    {
        let mut d = download_state().lock().unwrap();
        if d.running {
            return;
        }
        d.running = true;
        d.fraction = 0.0;
        d.error = None;
    }
    speech::emit_status(&app);
    tauri::async_runtime::spawn(async move {
        let result = fetch_and_unpack(|fraction| {
            download_state().lock().unwrap().fraction = fraction;
            speech::emit_status(&app);
        })
        .await;
        {
            let mut d = download_state().lock().unwrap();
            d.running = false;
            if let Err(err) = &result {
                d.error = Some(err.clone());
            }
        }
        match result {
            Ok(()) => crate::log::line("Kitten downloaded"),
            Err(err) => crate::log::line(format!("Kitten download failed: {err}")),
        }
        speech::emit_status(&app);
    });
}

async fn fetch_and_unpack(on_progress: impl Fn(f64)) -> Result<(), String> {
    let models = speech::models_dir();
    std::fs::create_dir_all(&models).map_err(|e| e.to_string())?;
    let part = models.join(format!("{ID}.tar.bz2.part"));
    let fetched = fetch(&part, on_progress).await;
    let result = match fetched {
        Ok(()) => {
            let (archive, models) = (part.clone(), models.clone());
            tauri::async_runtime::spawn_blocking(move || unpack(&archive, &models)).await.map_err(|e| e.to_string())?
        }
        Err(err) => Err(err),
    };
    let _ = std::fs::remove_file(&part);
    result
}

async fn fetch(part: &Path, on_progress: impl Fn(f64)) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!("Ciel/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client.get(ARCHIVE_URL).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("{ID} — HTTP {}", response.status().as_u16()));
    }
    let mut out = std::fs::File::create(part).map_err(|e| e.to_string())?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut got: u64 = 0;
    let mut last_emit = Instant::now();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        out.write_all(&chunk).map_err(|e| e.to_string())?;
        hash.update(&chunk);
        got += chunk.len() as u64;
        if got > ARCHIVE_SIZE {
            break;
        }
        if last_emit.elapsed() > Duration::from_millis(250) {
            last_emit = Instant::now();
            on_progress(got as f64 / ARCHIVE_SIZE as f64);
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    let hex: String = hash.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if got != ARCHIVE_SIZE || hex != ARCHIVE_SHA256 {
        return Err(format!("{ID}: the file that came in is not the expected one"));
    }
    Ok(())
}

/// Unpacks `archive` into `models`/ID, replacing what was there only once the
/// new copy is complete. tar refuses entries that would land outside the folder.
fn unpack(archive: &Path, models: &Path) -> Result<(), String> {
    let staging = models.join(format!("{ID}.unpacking"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
        tar::Archive::new(bzip2::read::BzDecoder::new(BufReader::new(file))).unpack(&staging).map_err(|e| e.to_string())?;
        let unpacked = staging.join(ID);
        if !unpacked.join(MODEL).is_file() {
            return Err(format!("{ID}: the archive does not hold the model"));
        }
        let target = models.join(ID);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&unpacked, &target).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Deletes the voice (OK Ciel → Remove the voice).
pub fn remove(app: &AppHandle) {
    unload();
    let _ = std::fs::remove_dir_all(dir());
    download_state().lock().unwrap().error = None;
    speech::emit_status(app);
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
                model: Some(path(MODEL)),
                voices: Some(path("voices.bin")),
                tokens: Some(path("tokens.txt")),
                data_dir: Some(path("espeak-ng-data")),
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
    (!samples.is_empty()).then(|| wav(samples, rate))
}

/// 16-bit mono PCM WAV, what the pill's <audio> plays.
fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
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

    #[test]
    fn the_wav_has_a_proper_header_and_clamped_samples() {
        let w = wav(&[0.0, 1.0, -1.0, 2.0, -2.0], 24_000);
        assert_eq!(w.len(), 44 + 10);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 10);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 48_000);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 10);
        let s: Vec<i16> = w[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(s, [0, 32767, -32767, 32767, -32767]);
    }

    fn tar_bz2(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast()));
        for (name, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *body).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ciel-kitten-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_archive_is_unpacked_into_its_folder_and_replaces_the_old_one() {
        let models = temp_dir("unpack");
        let archive = models.join("a.tar.bz2");
        let model = format!("{ID}/{MODEL}");
        let phontab = format!("{ID}/espeak-ng-data/phontab");
        std::fs::write(&archive, tar_bz2(&[(model.as_str(), b"onnx"), (phontab.as_str(), b"ph")])).unwrap();
        std::fs::create_dir_all(models.join(ID)).unwrap();
        std::fs::write(models.join(ID).join("old.txt"), b"old").unwrap();
        unpack(&archive, &models).unwrap();
        assert_eq!(std::fs::read(models.join(ID).join(MODEL)).unwrap(), b"onnx");
        assert!(models.join(ID).join("espeak-ng-data/phontab").is_file());
        assert!(!models.join(ID).join("old.txt").exists());
        assert!(!models.join(format!("{ID}.unpacking")).exists());
        let _ = std::fs::remove_dir_all(&models);
    }

    #[test]
    fn an_archive_without_the_model_leaves_the_installed_voice_alone() {
        let models = temp_dir("bad");
        let archive = models.join("a.tar.bz2");
        std::fs::write(&archive, tar_bz2(&[("something-else/readme.txt", b"hi")])).unwrap();
        std::fs::create_dir_all(models.join(ID)).unwrap();
        std::fs::write(models.join(ID).join(MODEL), b"kept").unwrap();
        assert!(unpack(&archive, &models).is_err());
        assert_eq!(std::fs::read(models.join(ID).join(MODEL)).unwrap(), b"kept");
        assert!(!models.join(format!("{ID}.unpacking")).exists());
        let _ = std::fs::remove_dir_all(&models);
    }

    #[test]
    fn an_entry_that_climbs_out_of_the_folder_is_not_written_there() {
        let models = temp_dir("climb");
        let archive = models.join("a.tar.bz2");
        // tar::Builder refuses "..", so the header's name is written by hand.
        let mut header = tar::Header::new_gnu();
        header.as_gnu_mut().unwrap().name[..12].copy_from_slice(b"../escape.tx");
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        let mut builder = tar::Builder::new(bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast()));
        builder.append(&header, &b"x"[..]).unwrap();
        std::fs::write(&archive, builder.into_inner().unwrap().finish().unwrap()).unwrap();
        let _ = unpack(&archive, &models);
        assert!(!models.join("escape.tx").exists());
        let _ = std::fs::remove_dir_all(&models);
    }
}

#[cfg(test)]
mod with_model {
    /// Downloads the real archive (64 MB) into XDG_DATA_HOME's ciel/Models,
    /// checks it and unpacks it, as the Download button does.
    #[test]
    #[ignore]
    fn downloads_and_unpacks_the_real_archive() {
        let _ = std::fs::remove_dir_all(super::dir());
        let steps = std::sync::atomic::AtomicUsize::new(0);
        tauri::async_runtime::block_on(super::fetch_and_unpack(|_| {
            steps.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }))
        .unwrap();
        assert!(super::is_present());
        assert!(steps.into_inner() > 0);
        assert!(!super::speech::models_dir().join(format!("{}.tar.bz2.part", super::ID)).exists());
    }

    /// Needs the voice downloaded (OK Ciel → Kitten → Download), or
    /// XDG_DATA_HOME pointing at a folder holding ciel/Models/<ID>.
    /// CIEL_TEST_KITTEN_OUT keeps the WAVs to listen to.
    #[test]
    #[ignore]
    fn says_an_answer_in_both_voices() {
        for gender in ["female", "male"] {
            let wav = super::say("Your build finished, and two tests failed in the printers module.", gender).expect("the voice");
            let rate = u32::from_le_bytes(wav[24..28].try_into().unwrap());
            let secs = (wav.len() - 44) as f64 / 2.0 / rate as f64;
            println!("{gender}: {secs:.1} s at {rate} Hz");
            assert!(secs > 2.0 && secs < 8.0);
            if let Ok(dir) = std::env::var("CIEL_TEST_KITTEN_OUT") {
                std::fs::write(format!("{dir}/kitten-{gender}.wav"), &wav).unwrap();
            }
        }
        assert!(super::say("Ta compilation est terminée.", "female").is_none());
    }
}
