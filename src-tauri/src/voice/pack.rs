// The voices « OK Ciel » can answer in on this computer (kitten.rs,
// kokoro.rs) come as one archive each from sherpa-onnx's releases. Nothing is
// downloaded until the user clicks Download in Ciel → OK Ciel; the archive is
// checked against its size and SHA-256 and unpacked next to Parakeet in the
// Models folder, replacing the old copy only once the new one is complete.

use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::speech::{self, ModelState};

/// One voice's archive and what must be in it.
pub struct Pack {
    /// For the log: "Kitten", "Kokoro".
    pub name: &'static str,
    /// The folder the archive unpacks to, kept under the same name in Models.
    pub id: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub model: &'static str,
    pub model_size: u64,
    /// The other files the voice needs, relative to its folder.
    pub needs: &'static [&'static str],
}

impl Pack {
    pub fn dir(&self) -> PathBuf {
        speech::models_dir().join(self.id)
    }

    pub fn path(&self, name: &str) -> String {
        self.dir().join(name).to_string_lossy().to_string()
    }

    /// The model is there with its size (the archive's hash was checked when it came in).
    pub fn is_present(&self) -> bool {
        let d = self.dir();
        std::fs::metadata(d.join(self.model)).is_ok_and(|m| m.len() == self.model_size) && self.needs.iter().all(|n| d.join(n).exists())
    }

    pub fn download_mb(&self) -> u64 {
        self.size / 1_000_000
    }

    /// What the OK Ciel page shows about the voice.
    pub fn state(&self) -> ModelState {
        let all = downloads().lock().unwrap();
        let d = all.iter().find(|(id, _)| *id == self.id).map(|(_, d)| d);
        match d {
            Some(d) if d.running => ModelState::Downloading { fraction: d.fraction },
            _ if self.is_present() => ModelState::Ready,
            Some(Download { error: Some(error), .. }) => ModelState::Failed { error: error.clone() },
            _ => ModelState::Missing,
        }
    }

    /// Downloads and unpacks the voice, with the progress in `speech-models` events.
    pub fn download(&'static self, app: AppHandle) {
        if !set(self.id, |d| {
            if d.running {
                return false;
            }
            *d = Download { running: true, fraction: 0.0, error: None };
            true
        }) {
            return;
        }
        speech::emit_status(&app);
        tauri::async_runtime::spawn(async move {
            let result = self
                .fetch_and_unpack(|fraction| {
                    set(self.id, |d| d.fraction = fraction);
                    speech::emit_status(&app);
                })
                .await;
            set(self.id, |d| {
                d.running = false;
                d.error = result.as_ref().err().cloned();
            });
            match result {
                Ok(()) => crate::log::line(format!("{} downloaded", self.name)),
                Err(err) => crate::log::line(format!("{} download failed: {err}", self.name)),
            }
            speech::emit_status(&app);
        });
    }

    /// Deletes the voice (OK Ciel → Remove the voice); `unload` frees it first.
    pub fn remove(&self, app: &AppHandle, unload: fn()) {
        unload();
        let _ = std::fs::remove_dir_all(self.dir());
        set(self.id, |d| d.error = None);
        speech::emit_status(app);
    }

    pub(crate) async fn fetch_and_unpack(&self, on_progress: impl Fn(f64)) -> Result<(), String> {
        let models = speech::models_dir();
        std::fs::create_dir_all(&models).map_err(|e| e.to_string())?;
        let part = models.join(format!("{}.tar.bz2.part", self.id));
        let result = match self.fetch(&part, on_progress).await {
            Ok(()) => {
                let (archive, models, id, model) = (part.clone(), models.clone(), self.id, self.model);
                tauri::async_runtime::spawn_blocking(move || unpack(&archive, &models, id, model)).await.map_err(|e| e.to_string())?
            }
            Err(err) => Err(err),
        };
        let _ = std::fs::remove_file(&part);
        result
    }

    async fn fetch(&self, part: &Path, on_progress: impl Fn(f64)) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("Ciel/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        let mut response = client.get(self.url).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("{} — HTTP {}", self.id, response.status().as_u16()));
        }
        let mut out = std::fs::File::create(part).map_err(|e| e.to_string())?;
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        let mut got: u64 = 0;
        let mut last_emit = Instant::now();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            out.write_all(&chunk).map_err(|e| e.to_string())?;
            hash.update(&chunk);
            got += chunk.len() as u64;
            if got > self.size {
                break;
            }
            if last_emit.elapsed() > Duration::from_millis(250) {
                last_emit = Instant::now();
                on_progress(got as f64 / self.size as f64);
            }
        }
        out.flush().map_err(|e| e.to_string())?;
        let hex: String = hash.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        if got != self.size || hex != self.sha256 {
            return Err(format!("{}: the file that came in is not the expected one", self.id));
        }
        Ok(())
    }
}

struct Download {
    running: bool,
    fraction: f64,
    error: Option<String>,
}

fn downloads() -> &'static Mutex<Vec<(&'static str, Download)>> {
    static D: OnceLock<Mutex<Vec<(&'static str, Download)>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Vec::new()))
}

/// Changes the download of `id`, made if it isn't there yet.
fn set<R>(id: &'static str, change: impl FnOnce(&mut Download) -> R) -> R {
    let mut all = downloads().lock().unwrap();
    if !all.iter().any(|(i, _)| *i == id) {
        all.push((id, Download { running: false, fraction: 0.0, error: None }));
    }
    let (_, d) = all.iter_mut().find(|(i, _)| *i == id).unwrap();
    change(d)
}

/// Unpacks `archive` into `models`/`id`, replacing what was there only once the
/// new copy is complete. tar refuses entries that would land outside the folder.
pub(crate) fn unpack(archive: &Path, models: &Path, id: &str, model: &str) -> Result<(), String> {
    let staging = models.join(format!("{id}.unpacking"));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
        tar::Archive::new(bzip2::read::BzDecoder::new(BufReader::new(file))).unpack(&staging).map_err(|e| e.to_string())?;
        let unpacked = staging.join(id);
        if !unpacked.join(model).is_file() {
            return Err(format!("{id}: the archive does not hold the model"));
        }
        let target = models.join(id);
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&unpacked, &target).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// 16-bit mono PCM WAV, what the pill's <audio> plays.
pub fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
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

/// How long a WAV made by `wav` lasts.
pub fn wav_seconds(wav: &[u8]) -> f64 {
    let rate = wav.get(24..28).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    if rate == 0 {
        return 0.0;
    }
    wav.len().saturating_sub(44) as f64 / 2.0 / rate as f64
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((wav_seconds(&wav(&[0.0; 48_000], 24_000)) - 2.0).abs() < 1e-9);
        assert_eq!(wav_seconds(b"short"), 0.0);
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

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ciel-pack-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const ID: &str = "voice-v1";
    const MODEL: &str = "model.onnx";

    #[test]
    fn the_archive_is_unpacked_into_its_folder_and_replaces_the_old_one() {
        let models = temp_dir("unpack");
        let archive = models.join("a.tar.bz2");
        let model = format!("{ID}/{MODEL}");
        let phontab = format!("{ID}/espeak-ng-data/phontab");
        std::fs::write(&archive, tar_bz2(&[(model.as_str(), b"onnx"), (phontab.as_str(), b"ph")])).unwrap();
        std::fs::create_dir_all(models.join(ID)).unwrap();
        std::fs::write(models.join(ID).join("old.txt"), b"old").unwrap();
        unpack(&archive, &models, ID, MODEL).unwrap();
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
        assert!(unpack(&archive, &models, ID, MODEL).is_err());
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
        let _ = unpack(&archive, &models, ID, MODEL);
        assert!(!models.join("escape.tx").exists());
        let _ = std::fs::remove_dir_all(&models);
    }

    #[test]
    fn each_voice_keeps_its_own_download_state() {
        set("pack-a", |d| d.running = true);
        set("pack-b", |d| d.error = Some("no network".into()));
        assert!(set("pack-a", |d| d.running));
        assert!(!set("pack-b", |d| d.running));
        assert_eq!(set("pack-b", |d| d.error.clone()).as_deref(), Some("no network"));
    }
}
