// The company folder (Settings → Island → Chat): what the answer key reads for
// the passages that fit the question. When a call starts to be noted, Ciel
// reads the folder into memory; at each question it looks again, and a file
// added or changed since is read again (one removed goes), so the passages are
// always in step with the files without reading them all each time. Only the
// passages that fit go to the brain with the question. Nothing is copied or
// kept elsewhere, and what was read goes when the call ends.
//
// Passages are ranked by BM25: a word few passages hold counts for more than a
// word most of them hold, so "pricing" outweighs "project", in any language,
// with no list of words to leave out. A word finds its family by its first
// letters ("price", "prices", "pricing"), and the file's name counts as words
// of each of its passages.
//
// ponytail: text files only (Markdown, text, CSV, JSON, YAML, HTML) and words,
// not meaning; Word and PDF files, and a question in one language finding a
// file in another, when the folders call for them.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

/// What is read, at most: enough for a company's notes, not a whole disk.
const MAX_FILES: usize = 3000;
const MAX_FILE: u64 = 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
/// A passage: a paragraph, or a stretch of one this long.
const PASSAGE: usize = 900;
const EXTENSIONS: [&str; 10] = ["md", "markdown", "txt", "csv", "tsv", "json", "yaml", "yml", "html", "org"];
/// Folders never worth reading.
const SKIP: [&str; 6] = ["node_modules", "target", "vendor", "dist", "build", "__pycache__"];
/// BM25's usual constants.
const K1: f64 = 1.2;
const B: f64 = 0.75;

/// Words too common to search for, in the languages Ciel's calls are most in.
/// BM25 already makes them count for little; leaving them out keeps the
/// question's words for the ones that tell passages apart.
const COMMON: &[&str] = &[
    "the", "and", "for", "you", "are", "can", "how", "our", "not", "but", "was", "has", "had", "who", "why", "all", "any",
    "that", "this", "with", "from", "have", "what", "when", "will", "would", "could", "should", "your", "about", "there",
    "their", "they", "them", "then", "than", "which", "where", "here", "into", "just", "like", "also", "been", "were",
    "les", "des", "une", "est", "que", "qui", "pas", "sur", "par", "aux", "ont", "pour", "dans", "avec", "vous", "nous",
    "est-ce", "quand", "comment", "pourquoi", "cette", "sont", "mais", "los", "las", "una", "por", "con", "del",
];

/// A text's words, for counting: lower case, three letters or more, not common ones.
fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(|w| w.trim_matches('-').to_lowercase())
        .filter(|w| w.chars().count() >= 3 && !COMMON.contains(&w.as_str()))
}

/// The words a question is made of, each once.
pub fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = tokens(text).collect();
    out.sort();
    out.dedup();
    out
}

fn readable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// A file Ciel may read: its path, size and last change.
struct Listed {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

/// The files Ciel may read in the folder, in a stable order.
fn files(dir: &Path) -> Vec<Listed> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut total = 0u64;
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                continue;
            }
            let Ok(kind) = e.file_type() else { continue };
            let path = e.path();
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && readable(&path) {
                let meta = e.metadata().ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(u64::MAX);
                if size > MAX_FILE || total + size > MAX_TOTAL {
                    continue;
                }
                total += size;
                out.push(Listed { path, size, modified: meta.and_then(|m| m.modified().ok()) });
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

/// Paragraphs, long ones cut at a line or a space.
pub fn passages(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let mut rest = para;
        while rest.chars().count() > PASSAGE {
            let cut = rest.char_indices().nth(PASSAGE).map_or(rest.len(), |(i, _)| i);
            let at = rest[..cut].rfind('\n').or_else(|| rest[..cut].rfind(' ')).filter(|&i| i > 0).unwrap_or(cut);
            out.push(rest[..at].trim().to_string());
            rest = rest[at..].trim_start();
        }
        if !rest.is_empty() {
            out.push(rest.to_string());
        }
    }
    out
}

struct Passage {
    /// The file it comes from, relative to the folder.
    file: String,
    text: String,
    /// Its words and how many times each is there, sorted by word.
    terms: Vec<(String, u32)>,
    len: u32,
}

/// A file as it was read.
struct Doc {
    size: u64,
    modified: Option<SystemTime>,
    passages: Vec<Passage>,
}

fn read_doc(dir: &Path, listed: &Listed) -> Doc {
    let name = listed.path.strip_prefix(dir).unwrap_or(&listed.path).display().to_string();
    // "sales/pricing.md": a question on prices finds its passages.
    let named = words(&name);
    let text = std::fs::read_to_string(&listed.path).unwrap_or_default();
    let passages = passages(&text)
        .into_iter()
        .map(|p| {
            let mut counts: BTreeMap<String, u32> = BTreeMap::new();
            for w in tokens(&p).chain(named.iter().cloned()) {
                *counts.entry(w).or_default() += 1;
            }
            let len = counts.values().sum();
            Passage { file: name.clone(), text: p, terms: counts.into_iter().collect(), len }
        })
        .collect();
    Doc { size: listed.size, modified: listed.modified, passages }
}

/// The folder as Ciel has read it, while a call is noted.
pub struct Index {
    dir: PathBuf,
    docs: BTreeMap<PathBuf, Doc>,
    /// How many passages hold each word.
    df: BTreeMap<String, u32>,
    count: usize,
    avg_len: f64,
}

/// The first letters a word's family shares: the word itself up to four
/// letters, else all but its last two (at least four).
fn stem(word: &str) -> String {
    let n = word.chars().count();
    if n <= 4 {
        word.to_string()
    } else {
        word.chars().take((n - 2).max(4)).collect()
    }
}

impl Index {
    pub fn new(dir: &Path) -> Index {
        Index { dir: dir.to_path_buf(), docs: BTreeMap::new(), df: BTreeMap::new(), count: 0, avg_len: 0.0 }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The files read.
    pub fn files(&self) -> usize {
        self.docs.len()
    }

    /// Reads what changed in the folder since the last time: new and changed
    /// files, and removed ones go. True when something did.
    pub fn refresh(&mut self) -> bool {
        let listed = files(&self.dir);
        let present: HashSet<&PathBuf> = listed.iter().map(|l| &l.path).collect();
        let before = self.docs.len();
        self.docs.retain(|path, _| present.contains(path));
        let mut changed = self.docs.len() != before;
        for l in &listed {
            if self.docs.get(&l.path).is_some_and(|d| d.size == l.size && d.modified == l.modified) {
                continue;
            }
            self.docs.insert(l.path.clone(), read_doc(&self.dir, l));
            changed = true;
        }
        if changed {
            self.df.clear();
            let mut total = 0u64;
            self.count = 0;
            for p in self.docs.values().flat_map(|d| &d.passages) {
                for (term, _) in &p.terms {
                    *self.df.entry(term.clone()).or_default() += 1;
                }
                total += p.len as u64;
                self.count += 1;
            }
            self.avg_len = if self.count == 0 { 0.0 } else { total as f64 / self.count as f64 };
        }
        changed
    }

    /// The words of the folder in `word`'s family, with how rare each is.
    fn family(&self, word: &str) -> Vec<(&str, f64)> {
        let stem = stem(word);
        let longest = word.chars().count() + 4;
        let n = self.count as f64;
        self.df
            .range(stem.clone()..)
            .take_while(|(term, _)| term.starts_with(&stem))
            .filter(|(term, _)| term.chars().count() <= longest)
            .map(|(term, &df)| (term.as_str(), (1.0 + (n - df as f64 + 0.5) / (df as f64 + 0.5)).ln()))
            .collect()
    }

    /// The passages that fit the question best, best first, up to `budget`
    /// characters, with the file each comes from.
    pub fn search(&self, question: &[String], budget: usize) -> Vec<(String, String)> {
        if self.count == 0 {
            return Vec::new();
        }
        let families: Vec<Vec<(&str, f64)>> = question.iter().map(|q| self.family(q)).filter(|f| !f.is_empty()).collect();
        if families.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(f64, &Passage)> = Vec::new();
        for p in self.docs.values().flat_map(|d| &d.passages) {
            let norm = K1 * (1.0 - B + B * p.len as f64 / self.avg_len.max(1.0));
            // Each word of the question counts once, by the best of its family here.
            let score: f64 = families
                .iter()
                .map(|family| {
                    family
                        .iter()
                        .filter_map(|(term, idf)| {
                            let i = p.terms.binary_search_by(|(t, _)| t.as_str().cmp(term)).ok()?;
                            let tf = p.terms[i].1 as f64;
                            Some(idf * tf * (K1 + 1.0) / (tf + norm))
                        })
                        .fold(0.0, f64::max)
                })
                .sum();
            if score > 0.0 {
                scored.push((score, p));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.1.file.cmp(&b.1.file)));
        let mut out = Vec::new();
        let mut used = 0;
        for (_, p) in scored {
            if used + p.text.len() > budget {
                continue;
            }
            used += p.text.len();
            out.push((p.file.clone(), p.text.clone()));
            if used >= budget * 9 / 10 {
                break;
            }
        }
        out
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub exists: bool,
    /// The files Ciel can read there.
    pub files: usize,
}

pub fn info(dir: &Path) -> FolderInfo {
    FolderInfo { exists: dir.is_dir(), files: if dir.is_dir() { files(dir).len() } else { 0 } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ciel-company-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn index(dir: &Path) -> Index {
        let mut index = Index::new(dir);
        index.refresh();
        index
    }

    #[test]
    fn the_passages_that_fit_the_question_come_first_and_only_text_files_are_read() {
        let dir = folder("fit");
        std::fs::create_dir_all(dir.join("sales")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("sales/pricing.md"), "# Pricing\n\nThe Pro plan is 90 euros per seat each month.\n\nDiscounts start at twenty seats.").unwrap();
        std::fs::write(dir.join("team.txt"), "Paul runs the support team.").unwrap();
        std::fs::write(dir.join(".git/config"), "pricing seats").unwrap();
        std::fs::write(dir.join("logo.png"), "pricing seats").unwrap();

        assert_eq!(info(&dir).files, 2);
        let index = index(&dir);
        assert_eq!(index.files(), 2);
        let q = words("What's the price per seat for the Pro plan?");
        assert!(q.contains(&"seat".to_string()) && !q.contains(&"what".to_string()) && !q.contains(&"the".to_string()));
        let found = index.search(&q, 2000);
        assert_eq!(found[0].0, format!("sales{}pricing.md", std::path::MAIN_SEPARATOR));
        assert!(found[0].1.contains("90 euros"));
        assert!(found.iter().all(|(name, _)| !name.contains(".git") && !name.ends_with(".png")));
        assert!(index.search(&words("support"), 2000)[0].1.contains("Paul"));
        assert!(index.search(&words("nothing like it"), 2000).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rare_word_outweighs_a_common_one_and_a_word_finds_its_family() {
        let dir = folder("rare");
        let common: String = (0..12).map(|i| format!("Project update number {i} for the project team.\n\n")).collect();
        std::fs::write(dir.join("projects.md"), common).unwrap();
        std::fs::write(dir.join("notes.md"), "Our SLA for the project: answers within four hours.").unwrap();
        let index = index(&dir);
        let found = index.search(&words("What is the SLA on this project?"), 300);
        assert!(found[0].1.contains("four hours"), "{found:?}");
        // "prices" and "pricing" are one family.
        std::fs::write(dir.join("offer.md"), "Our pricing changed in May.").unwrap();
        let mut index = index;
        assert!(index.refresh());
        assert!(index.search(&words("prices"), 300)[0].1.contains("pricing"));
        assert_eq!(stem("seats"), "seat");
        assert_eq!(stem("sla"), "sla");
        assert_eq!(stem("discounts"), "discoun");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_what_changed_is_read_again_and_a_removed_file_goes() {
        let dir = folder("refresh");
        std::fs::write(dir.join("a.md"), "Shipping takes three days.").unwrap();
        let mut index = index(&dir);
        assert!(!index.refresh(), "nothing changed");
        std::fs::write(dir.join("b.md"), "Refunds within thirty days.").unwrap();
        assert!(index.refresh());
        assert!(index.search(&words("refunds"), 300)[0].1.contains("thirty"));
        std::fs::remove_file(dir.join("b.md")).unwrap();
        assert!(index.refresh());
        assert!(index.search(&words("refunds"), 300).is_empty());
        assert_eq!(index.files(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_paragraphs_are_cut_at_a_space() {
        let long = "word ".repeat(400);
        let parts = passages(&long);
        assert!(parts.len() >= 2 && parts.iter().all(|p| p.chars().count() <= PASSAGE));
    }
}
