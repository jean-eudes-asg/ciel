// « OK Ciel »: hearing the wake phrase in what Parakeet wrote, and telling
// speech from silence on the microphone. Nothing here touches the OS.

/// Lower case, accents gone, everything but letters and digits a space.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        let base = match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'ç' => 'c',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ÿ' | 'ý' => 'y',
            'ñ' => 'n',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        out.push(base);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How Parakeet may write "OK".
fn is_ok(word: &str) -> bool {
    matches!(word, "ok" | "okay" | "oke" | "okey" | "hey")
}

/// How Parakeet may write "Ciel" (said the French way, "see-ell"), in one word.
fn is_ciel(word: &str) -> bool {
    matches!(
        word,
        "ciel" | "ciels" | "cielle" | "ciell" | "siel" | "sielle" | "siell" | "seel" | "seal" | "ceil" | "cial"
    )
}

/// …or in two ("see el", "sea elle"…). Said as one syllable, "syel", it
/// comes out as "see yeah", "C yeah" or "see ya".
fn is_ciel_pair(a: &str, b: &str) -> bool {
    matches!(
        (a, b),
        ("see" | "sea", "el" | "elle" | "yeah" | "ya")
            | ("si", "elle" | "yeah")
            | ("c", "el" | "yeah" | "ya")
    )
}

/// …and as "see you", which is also how a conversation ends: only when it
/// opens what was said, never in the middle of a sentence.
fn is_ciel_pair_at_start(a: &str, b: &str) -> bool {
    (a, b) == ("see", "you")
}

/// The words as `normalize` sees them, each with where it ends in `text`:
/// "Yeah.Is" is two of them, as "O.K." is.
fn tokens(text: &str) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut current = String::new();
    for (at, c) in text.char_indices() {
        let piece = normalize(&c.to_string());
        if piece.is_empty() {
            if !current.is_empty() {
                out.push((std::mem::take(&mut current), at));
            }
        } else {
            current.push_str(&piece);
        }
    }
    if !current.is_empty() {
        out.push((current, text.len()));
    }
    out
}

/// A word as it sounds rather than as it is spelled, near enough for a name:
/// "Raphaël", "Rafael" and "Raffael" are all "rafael".
fn sound(word: &str) -> String {
    let w = word.replace("ph", "f").replace("ck", "k").replace("qu", "k").replace('y', "i").replace('h', "");
    let mut out = String::with_capacity(w.len());
    for c in w.chars() {
        if !out.ends_with(c) {
            out.push(c);
        }
    }
    out
}

/// When what was said opens with `name` (after an "OK" or a "Hey", maybe),
/// where the name ends in `text`. A name said in passing doesn't count.
fn after_name(words: &[(String, usize)], name: &str) -> Option<usize> {
    let name: Vec<String> = normalize(name).split(' ').filter(|w| !w.is_empty()).map(sound).collect();
    if name.is_empty() {
        return None;
    }
    let opens_at = |start: usize| {
        let said = words.get(start..start + name.len())?;
        said.iter().zip(&name).all(|((w, _), n)| sound(w) == *n).then(|| said[said.len() - 1].1)
    };
    opens_at(0).or_else(|| words.first().filter(|(w, _)| is_ok(w)).and_then(|_| opens_at(1)))
}

/// When `text` holds the wake phrase, « OK Ciel » or the other `name` Ciel
/// answers to, what was said after it (maybe nothing). The words keep their
/// case and punctuation, as Parakeet wrote them.
pub fn after_wake(text: &str, name: &str) -> Option<String> {
    let words = tokens(text);
    let word = |i: usize| words.get(i).map(|(w, _)| w.as_str());
    let after = |end: usize| text[end..].trim_start_matches([',', '.', '!', '?', ';', ':', ' ']).trim().to_string();
    if let Some(end) = after_name(&words, name) {
        return Some(after(end));
    }
    for i in 0..words.len() {
        // "OK", "Okay", or "O.K." in two pieces.
        let ok_end = if is_ok(&words[i].0) {
            Some(i + 1)
        } else if word(i) == Some("o") && word(i + 1) == Some("k") {
            Some(i + 2)
        } else {
            None
        };
        if let Some(j) = ok_end {
            if let Some(w) = word(j) {
                if is_ciel(w) {
                    return Some(after(words[j].1));
                }
                if let Some(next) = word(j + 1) {
                    if is_ciel_pair(w, next) || (i == 0 && is_ciel_pair_at_start(w, next)) {
                        return Some(after(words[j + 1].1));
                    }
                }
            }
        }
        // "okciel" in one piece.
        if let Some(rest) = words[i].0.strip_prefix("okay").or_else(|| words[i].0.strip_prefix("ok")) {
            if is_ciel(rest) {
                return Some(after(words[i].1));
            }
        }
    }
    None
}

/// Speech or silence, 30 ms at a time, against a noise floor that follows the
/// room. A stretch of speech is handed out once followed by `hangover` frames
/// of silence, or when it gets too long.
pub struct Vad {
    floor: f32,
    in_speech: bool,
    speech_frames: usize,
    silent_frames: usize,
    frame: Vec<f32>,
    /// The current stretch, with a little audio from before it began.
    current: Vec<f32>,
    preroll: std::collections::VecDeque<f32>,
    /// Silence that ends a stretch, in frames.
    pub hangover: usize,
}

pub const FRAME: usize = 480; // 30 ms at 16 kHz
const PREROLL: usize = 16_000 * 3 / 10;
const MAX_SEGMENT: usize = 16_000 * 12;

impl Default for Vad {
    fn default() -> Self {
        Self {
            floor: 0.004,
            in_speech: false,
            speech_frames: 0,
            silent_frames: 0,
            frame: Vec::with_capacity(FRAME),
            current: Vec::new(),
            preroll: std::collections::VecDeque::with_capacity(PREROLL),
            hangover: 25,
        }
    }
}

impl Vad {
    /// Someone is speaking now: a request isn't over yet.
    pub fn in_speech(&self) -> bool {
        self.in_speech
    }

    /// Feeds 16 kHz samples; the stretches of speech that ended.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        let mut done = Vec::new();
        for &s in samples {
            self.frame.push(s);
            if self.frame.len() < FRAME {
                continue;
            }
            let rms = (self.frame.iter().map(|x| x * x).sum::<f32>() / FRAME as f32).sqrt();
            let speech = rms > (self.floor * 3.0).max(0.012);
            if !speech && !self.in_speech {
                self.floor = self.floor * 0.97 + rms * 0.03;
            }
            if self.in_speech {
                self.current.extend_from_slice(&self.frame);
                if speech {
                    self.silent_frames = 0;
                } else {
                    self.silent_frames += 1;
                }
                if self.silent_frames >= self.hangover || self.current.len() >= MAX_SEGMENT {
                    done.push(std::mem::take(&mut self.current));
                    self.in_speech = false;
                    self.speech_frames = 0;
                    self.silent_frames = 0;
                }
            } else if speech {
                self.speech_frames += 1;
                if self.speech_frames >= 3 {
                    self.in_speech = true;
                    self.current = self.preroll.drain(..).collect();
                    self.current.extend_from_slice(&self.frame);
                }
            } else {
                self.speech_frames = 0;
            }
            if !self.in_speech {
                for &x in &self.frame {
                    if self.preroll.len() == PREROLL {
                        self.preroll.pop_front();
                    }
                    self.preroll.push_back(x);
                }
            }
            self.frame.clear();
        }
        done
    }

    /// Forgets what is being heard (Ciel speaks, or dictation starts).
    pub fn reset(&mut self) {
        self.in_speech = false;
        self.speech_frames = 0;
        self.silent_frames = 0;
        self.current.clear();
        self.preroll.clear();
        self.frame.clear();
    }
}

/// "fr-FR" for a French answer, else None (the system's language).
pub fn speech_lang(text: &str) -> Option<&'static str> {
    let n = normalize(text);
    let words: Vec<&str> = n.split(' ').collect();
    let french = ["le", "la", "les", "est", "tu", "vous", "je", "un", "une", "des", "pour", "pas", "c", "et", "sur", "avec", "ton", "ta"];
    let count = words.iter().filter(|w| french.contains(w)).count();
    (count * 6 >= words.len().max(1) && count >= 1).then_some("fr-FR")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wake_phrase_however_parakeet_writes_it() {
        assert_eq!(after_wake("OK Ciel, play some music.", "").as_deref(), Some("play some music."));
        assert_eq!(after_wake("Okay, ciel! Quelle heure est-il ?", "").as_deref(), Some("Quelle heure est-il ?"));
        assert_eq!(after_wake("O.K. Seal what's the weather", "").as_deref(), Some("what's the weather"));
        assert_eq!(after_wake("ok see el ajoute Stripe", "").as_deref(), Some("ajoute Stripe"));
        assert_eq!(after_wake("Okay, Sielle, play some music", "").as_deref(), Some("play some music"));
        assert_eq!(after_wake("okciel quelle heure", "").as_deref(), Some("quelle heure"));
        assert_eq!(after_wake("OK Ciel.", "").as_deref(), Some(""));
        assert_eq!(after_wake("Hey ciel, ça va ?", "").as_deref(), Some("ça va ?"));
        assert_eq!(after_wake("on a vu le ciel bleu", ""), None);
        assert_eq!(after_wake("ok si el código falla", ""), None);
        assert_eq!(after_wake("ok let's go", ""), None);
        assert_eq!(normalize("Ça VA, Élodie ?"), "ca va elodie");
    }

    #[test]
    fn ciel_said_in_one_syllable() {
        // What Parakeet wrote for « OK Ciel, what time is it? » said "syel".
        assert_eq!(after_wake("Okay, see you, what time is it?", "").as_deref(), Some("what time is it?"));
        assert_eq!(after_wake("Okay, C Yeah.Is it", "").as_deref(), Some("Is it"));
        assert_eq!(after_wake("OK see yeah, play some music", "").as_deref(), Some("play some music"));
        assert_eq!(after_wake("Okay. See ya.", "").as_deref(), Some(""));
        // "See you" ends conversations: only at the very start.
        assert_eq!(after_wake("Thanks, okay, see you tomorrow", ""), None);
        assert_eq!(after_wake("see you", ""), None);
        assert_eq!(after_wake("okay you see", ""), None);
    }

    #[test]
    fn the_other_name_wakes_ciel_when_said_first() {
        let name = "Raphael";
        assert_eq!(after_wake("Raphael, what time is it?", name).as_deref(), Some("what time is it?"));
        assert_eq!(after_wake("Rafael what time is it", name).as_deref(), Some("what time is it"));
        assert_eq!(after_wake("Hey Raphaël! Play some music.", name).as_deref(), Some("Play some music."));
        assert_eq!(after_wake("Raphael.", name).as_deref(), Some(""));
        // In passing, or with no name set: no.
        assert_eq!(after_wake("I saw Raphael yesterday", name), None);
        assert_eq!(after_wake("Raphael, what time is it?", ""), None);
        // « OK Ciel » still works beside it.
        assert_eq!(after_wake("OK Ciel, play some music", name).as_deref(), Some("play some music"));
        // A name of two words.
        assert_eq!(after_wake("Hey Nimbus, the weather?", "Hey Nimbus").as_deref(), Some("the weather?"));
        assert_eq!(after_wake("Nimbus, the weather?", "Hey Nimbus"), None);
    }

    #[test]
    fn speech_is_cut_after_a_pause() {
        let mut vad = Vad::default();
        let silence = vec![0.0f32; 16_000];
        let tone: Vec<f32> = (0..16_000).map(|i| (i as f32 * 0.1).sin() * 0.3).collect();
        assert!(vad.feed(&silence).is_empty());
        assert!(vad.feed(&tone).is_empty());
        assert!(vad.in_speech());
        let out = vad.feed(&silence);
        assert_eq!(out.len(), 1);
        // The tone, a pre-roll before it and the hangover after it.
        assert!(out[0].len() > 16_000 && out[0].len() <= 16_000 + 25 * FRAME + PREROLL + 2 * FRAME);
        assert!(!vad.in_speech());
    }

    #[test]
    fn french_answers_are_spoken_in_french() {
        assert_eq!(speech_lang("C'est fait, j'ai ajouté Stripe pour toi."), Some("fr-FR"));
        assert_eq!(speech_lang("Done, Stripe is in the island."), None);
    }
}
