// The company folder (Ciel window → Notetaker): when the answer key is
// pressed, Ciel reads it as it is then (so it is always in step with the
// files) for the passages that share the most words with what was just
// asked, and only those go to the brain with the question. Nothing is
// copied or kept elsewhere, and nothing is read until the key is pressed.
//
// ponytail: text files only (Markdown, text, CSV, JSON, YAML, HTML) and word
// counting, not meaning; Word and PDF files, and a search index for very
// big folders, when the folders call for them.

use std::path::{Path, PathBuf};

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

/// Words too common to tell passages apart, in the languages Ciel speaks most.
const COMMON: [&str; 40] = [
    "that", "this", "with", "from", "have", "what", "when", "will", "would", "could", "should", "your", "about", "there",
    "their", "they", "them", "then", "than", "which", "where", "here", "into", "just", "like", "also", "been", "were",
    "pour", "dans", "avec", "vous", "nous", "est-ce", "quand", "comment", "pourquoi", "cette", "sont", "mais",
];

/// The words a question is made of, for counting: lower case, four letters or more, not common ones.
pub fn words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 4 && !COMMON.contains(&w.as_str()))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn readable(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The files Ciel may read in the folder, in a stable order.
fn files(dir: &Path) -> Vec<PathBuf> {
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
                let size = e.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
                if size > MAX_FILE || total + size > MAX_TOTAL {
                    continue;
                }
                total += size;
                out.push(path);
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

/// The passages sharing the most words with the question, best first, up to
/// `budget` characters, with the file each comes from (relative to the folder).
pub fn search(dir: &Path, question: &[String], budget: usize) -> Vec<(String, String)> {
    if question.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, String, String)> = Vec::new();
    for path in files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let name = path.strip_prefix(dir).unwrap_or(&path).display().to_string();
        let in_name = words(&name);
        for p in passages(&text) {
            let lower = p.to_lowercase();
            let hits = question.iter().filter(|w| lower.contains(w.as_str())).count();
            // A word in the file's name counts too: "pricing.md" for a question on prices.
            let named = question.iter().filter(|w| in_name.iter().any(|n| n.starts_with(w.as_str()) || w.starts_with(n.as_str()))).count();
            if hits > 0 {
                scored.push((hits * 2 + named, name.clone(), p));
            }
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = Vec::new();
    let mut used = 0;
    for (_, name, p) in scored {
        if used + p.len() > budget {
            continue;
        }
        used += p.len();
        out.push((name, p));
        if used >= budget * 9 / 10 {
            break;
        }
    }
    out
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

    #[test]
    fn the_passages_that_fit_the_question_come_first_and_only_text_files_are_read() {
        let dir = std::env::temp_dir().join(format!("ciel-company-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sales")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("sales/pricing.md"), "# Pricing\n\nThe Pro plan is 90 euros per seat each month.\n\nDiscounts start at twenty seats.").unwrap();
        std::fs::write(dir.join("team.txt"), "Paul runs the support team.").unwrap();
        std::fs::write(dir.join(".git/config"), "pricing seats").unwrap();
        std::fs::write(dir.join("logo.png"), "pricing seats").unwrap();

        assert_eq!(info(&dir).files, 2);
        let q = words("What's the price per seat for the Pro plan?");
        assert!(q.contains(&"seat".to_string()) && !q.contains(&"what".to_string()));
        let found = search(&dir, &q, 2000);
        assert_eq!(found[0].0, format!("sales{}pricing.md", std::path::MAIN_SEPARATOR));
        assert!(found[0].1.contains("90 euros"));
        assert!(found.iter().all(|(name, _)| !name.contains(".git") && !name.ends_with(".png")));
        assert!(search(&dir, &words("support"), 2000)[0].1.contains("Paul"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_paragraphs_are_cut_at_a_space() {
        let long = "word ".repeat(400);
        let parts = passages(&long);
        assert!(parts.len() >= 2 && parts.iter().all(|p| p.chars().count() <= PASSAGE));
    }
}
