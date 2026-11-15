// Your work (Ciel window → Chat → Your work): what « OK Ciel » knows of the
// user's projects at the start of a conversation, whoever its brain is.
//
// Two things, put together in a few lines for the brain:
//
// - what must be done: Work/work.md in Ciel's own folder, a plain checklist
//   the user can open and edit, and that the brain adds to or ticks when told
//   to (the `work` tool);
// - how the folders the user named stand, looked at when they speak to Ciel
//   and kept five minutes: for a git project, its branch, how many files
//   aren't committed, how many commits aren't pushed and the last commits'
//   first lines; for any other folder, the files changed lately, by name.
//
// Nothing is looked at until the user named a folder, and nothing runs while
// no one talks to Ciel. Git is only ever asked to read (`status`, `log`),
// without its optional locks, so an agent working in the project is never
// held up. Never a file's contents, never a diff. With Claude these lines go
// to Anthropic with the question, as the settings say.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::settings::Settings;

/// A project or a file counts while it was touched in the last 30 days.
const ACTIVE: u64 = 30 * 86_400;
/// What was seen of the folders is looked at again after this.
const KEPT: Duration = Duration::from_secs(5 * 60);
/// The longest one `git` may take.
const GIT: Duration = Duration::from_secs(8);
/// The folders looked at, the projects of one folder, and the gits at once.
const MAX_FOLDERS: usize = 8;
const MAX_PROJECTS: usize = 40;
const AT_ONCE: usize = 8;
/// A folder that is no project: how far it is walked, and what is said of it.
const WALK_ENTRIES: usize = 4000;
const WALK_DEPTH: usize = 3;
const RECENT_FILES: usize = 6;
/// What is said of the list at most, and of one line of it.
const MAX_TODO: usize = 25;
const TODO_CHARS: usize = 200;
const SUBJECT_CHARS: usize = 80;
/// The brief in the instructions of Claude, and of a local model's small context.
pub const PROMPT_CHARS: usize = 3000;
pub const LOCAL_PROMPT_CHARS: usize = 1200;
/// What the tool gives back at most.
const TOOL_CHARS: usize = 6000;

/// Folders never worth walking, and files that are no one's work.
const SKIP: [&str; 9] = ["node_modules", "target", "dist", "build", "__pycache__", "venv", "System Volume Information", "desktop.ini", "Thumbs.db"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Commit {
    /// Unix seconds.
    pub at: u64,
    pub subject: String,
}

/// A git project as it stands.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    /// Files changed or new, not committed.
    pub changed: usize,
    /// Commits its upstream hasn't.
    pub ahead: usize,
    /// The last ones, newest first.
    pub commits: Vec<Commit>,
    /// When it was last worked in: its last commit or its index, Unix seconds.
    pub touched: u64,
}

/// A folder that is no git project: the files changed lately, newest first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plain {
    pub name: String,
    pub path: PathBuf,
    pub files: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Project(Project),
    Folder(Plain),
}

/// One line of the checklist.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub done: bool,
    pub text: String,
}

fn now() -> u64 {
    secs(SystemTime::now())
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path).and_then(|m| m.modified()).map(secs).unwrap_or(0)
}

fn cut(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > max {
        text.chars().take(max).chain("…".chars()).collect()
    } else {
        text
    }
}

fn name_of(dir: &Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.to_string_lossy().to_string())
}

/// The folders the user named, as they are looked at.
pub fn folders(settings: &Settings) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for folder in settings.work_folders.iter().map(|f| f.trim()).filter(|f| !f.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(folder)) {
            out.push(folder.to_string());
        }
    }
    out.truncate(MAX_FOLDERS);
    out
}

/// The user named a folder: the brain is told of their work.
pub fn on(settings: &Settings) -> bool {
    !folders(settings).is_empty()
}

// ── Git, read only ────────────────────────────────────────────────────────────

/// `git <args>` in `dir`: what it printed, or None when it failed or took too
/// long. Never its optional locks (an agent may be committing there) and never
/// the file watcher a project's own config could name.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    crate::platform::no_console(&mut cmd);
    let mut child = cmd
        .args(["--no-optional-locks", "-c", "core.fsmonitor=false", "-C"])
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        let _ = tx.send(String::from_utf8_lossy(&bytes).to_string());
    });
    let out = rx.recv_timeout(GIT);
    if out.is_err() {
        let _ = child.kill();
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    out.ok().filter(|_| ok)
}

const STATUS: [&str; 3] = ["status", "--porcelain=v2", "--branch"];

/// `git status --porcelain=v2 --branch`: the branch, the commits not pushed
/// and the files not committed.
fn parse_status(out: &str) -> (String, usize, usize) {
    let (mut branch, mut ahead, mut changed) = (String::new(), 0, 0);
    for line in out.lines() {
        if let Some(head) = line.strip_prefix("# branch.head ") {
            branch = head.trim().to_string();
        } else if let Some(ab) = line.strip_prefix("# branch.ab ") {
            ahead = ab.split_whitespace().next().and_then(|a| a.strip_prefix('+')).and_then(|n| n.parse().ok()).unwrap_or(0);
        } else if !line.starts_with('#') && !line.starts_with('!') && !line.trim().is_empty() {
            changed += 1;
        }
    }
    (branch, ahead, changed)
}

/// The files of the same output, by path.
fn changed_paths(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let path = match line.chars().next()? {
                '1' => line.splitn(9, ' ').nth(8)?,
                '2' => line.splitn(10, ' ').nth(9)?.split('\t').next()?,
                'u' => line.splitn(11, ' ').nth(10)?,
                '?' => line.get(2..)?,
                _ => return None,
            };
            Some(path.trim().to_string()).filter(|p| !p.is_empty())
        })
        .collect()
}

/// `git log --format=%ct%x09%s`.
fn parse_log(out: &str) -> Vec<Commit> {
    out.lines()
        .filter_map(|line| {
            let (at, subject) = line.split_once('\t')?;
            Some(Commit { at: at.trim().parse().ok()?, subject: subject.trim().to_string() })
        })
        .collect()
}

fn log_args(count: &str) -> [&str; 3] {
    ["log", count, "--format=%ct%x09%s"]
}

// ── Looking at the folders ────────────────────────────────────────────────────

fn skipped(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || name.starts_with('~') || SKIP.iter().any(|s| s.eq_ignore_ascii_case(name))
}

/// A checkout or a worktree: `.git` is a folder in one and a file in the other.
fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// A project as it stands. One left alone for 30 days is asked its last
/// commits only: its status, the slow question, waits until someone asks
/// about that project (`detail`).
fn project(dir: &Path, now: u64) -> Project {
    let mut p = Project { name: name_of(dir), path: dir.to_path_buf(), ..Project::default() };
    if let Some(out) = git(dir, &log_args("-3")) {
        p.commits = parse_log(&out);
    }
    p.touched = p.commits.first().map_or(0, |c| c.at).max(modified(&dir.join(".git").join("index")));
    if p.touched >= now.saturating_sub(ACTIVE) {
        if let Some(out) = git(dir, &STATUS) {
            (p.branch, p.ahead, p.changed) = parse_status(&out);
        }
    }
    p
}

/// A folder that is no project, walked a little way: its files changed since `now - ACTIVE`.
fn plain(folder: &Path, now: u64) -> Plain {
    let since = now.saturating_sub(ACTIVE);
    let mut files: Vec<(String, u64)> = Vec::new();
    let mut seen = 0;
    let mut queue = VecDeque::from([(folder.to_path_buf(), 0)]);
    'walk: while let Some((dir, depth)) = queue.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > WALK_ENTRIES {
                break 'walk;
            }
            if skipped(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if depth < WALK_DEPTH {
                    queue.push_back((entry.path(), depth + 1));
                }
            } else {
                let at = meta.modified().map(secs).unwrap_or(0);
                if at >= since {
                    let path = entry.path();
                    let inside = path.strip_prefix(folder).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                    files.push((inside, at));
                }
            }
        }
    }
    files.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    files.truncate(RECENT_FILES);
    Plain { name: name_of(folder), path: folder.to_path_buf(), files }
}

/// One of the user's folders: itself a project, a folder of projects, or a plain folder.
fn look(folder: &Path, now: u64) -> Vec<Place> {
    if !folder.is_dir() {
        return Vec::new();
    }
    if is_repo(folder) {
        return vec![Place::Project(project(folder, now))];
    }
    let mut repos: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| !skipped(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .filter(|p| p.is_dir() && is_repo(p))
                .collect()
        })
        .unwrap_or_default();
    if repos.is_empty() {
        return vec![Place::Folder(plain(folder, now))];
    }
    repos.sort();
    repos.truncate(MAX_PROJECTS);
    let mut out = Vec::new();
    for some in repos.chunks(AT_ONCE) {
        std::thread::scope(|scope| {
            let asked: Vec<_> = some.iter().map(|dir| scope.spawn(move || project(dir, now))).collect();
            for one in asked {
                if let Ok(p) = one.join() {
                    out.push(Place::Project(p));
                }
            }
        });
    }
    out
}

struct Seen {
    at: Instant,
    folders: Vec<String>,
    places: Vec<Place>,
}

static SEEN: LazyLock<Mutex<Option<Seen>>> = LazyLock::new(|| Mutex::new(None));
/// One look at a time: who comes second waits for the first one's.
static LOOKING: Mutex<()> = Mutex::new(());

fn fresh(folders: &[String]) -> Option<Vec<Place>> {
    SEEN.lock().unwrap().as_ref().filter(|s| s.at.elapsed() < KEPT && s.folders == folders).map(|s| s.places.clone())
}

/// How the folders stand, as seen in the last five minutes or looked at now.
pub fn places(folders: &[String]) -> Vec<Place> {
    if let Some(places) = fresh(folders) {
        return places;
    }
    let _one = LOOKING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(places) = fresh(folders) {
        return places;
    }
    let now = now();
    let mut all: Vec<Place> = Vec::new();
    for folder in folders {
        for place in look(Path::new(folder), now) {
            let path = |p: &Place| match p {
                Place::Project(p) => p.path.clone(),
                Place::Folder(f) => f.path.clone(),
            };
            // A folder named twice, or inside another, is said once.
            if !all.iter().any(|known| path(known) == path(&place)) {
                all.push(place);
            }
        }
    }
    *SEEN.lock().unwrap() = Some(Seen { at: Instant::now(), folders: folders.to_vec(), places: all.clone() });
    all
}

/// « OK Ciel » was heard: the folders are looked at while the question comes.
pub fn warm(app: &AppHandle) {
    let Some(settings) = app.try_state::<crate::Shared>().map(|s| s.settings.lock().unwrap().clone()) else { return };
    if on(&settings) {
        std::thread::spawn(move || {
            places(&folders(&settings));
        });
    }
}

// ── The checklist ─────────────────────────────────────────────────────────────

/// Work/work.md in Ciel's own folder.
pub fn file() -> PathBuf {
    crate::settings::local_dir().join("Work").join("work.md")
}

/// `- [ ] text` or `- [x] text`, as Markdown writes a checklist.
fn item(line: &str) -> Option<Item> {
    let line = line.trim_start();
    let rest = line.strip_prefix("- [").or_else(|| line.strip_prefix("* ["))?;
    let mut chars = rest.chars();
    let done = match chars.next()? {
        ' ' => false,
        'x' | 'X' => true,
        _ => return None,
    };
    let text = chars.as_str().strip_prefix(']')?.trim();
    (!text.is_empty()).then(|| Item { done, text: text.to_string() })
}

pub fn items(file: &str) -> Vec<Item> {
    file.lines().filter_map(item).collect()
}

/// The file's line ending, kept as the user's editor left it.
fn eol(file: &str) -> &'static str {
    if file.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// The file with one more thing to do at its end; a new file gets `title`.
fn added(file: &str, title: &str, text: &str) -> String {
    let eol = eol(file);
    let mut out = if file.trim().is_empty() { format!("# {title}{eol}{eol}") } else { file.to_string() };
    if !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&format!("- [ ] {text}{eol}"));
    out
}

/// The file with the one open line `query` names ticked, and that line's
/// text; or what to tell the brain when none or several fit.
fn ticked(file: &str, query: &str) -> Result<(String, String), String> {
    let query = query.trim().to_lowercase();
    let lines: Vec<&str> = file.split_inclusive('\n').collect();
    let open: Vec<(usize, Item)> = lines.iter().enumerate().filter_map(|(i, l)| item(l).filter(|it| !it.done).map(|it| (i, it))).collect();
    if query.is_empty() || open.is_empty() {
        return Err("Nothing is left to do in the list.".into());
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut hits: Vec<&(usize, Item)> = open.iter().filter(|(_, it)| it.text.to_lowercase() == query).collect();
    if hits.is_empty() {
        hits = open.iter().filter(|(_, it)| it.text.to_lowercase().contains(&query)).collect();
    }
    if hits.is_empty() {
        hits = open
            .iter()
            .filter(|(_, it)| {
                let text = it.text.to_lowercase();
                words.iter().all(|w| text.contains(w))
            })
            .collect();
    }
    match hits.as_slice() {
        [] => Err(format!("Nothing in the list fits that. What is left: {}.", open.iter().map(|(_, it)| it.text.as_str()).collect::<Vec<_>>().join("; "))),
        [(at, it)] => {
            let mut out = String::with_capacity(file.len());
            for (i, line) in lines.iter().enumerate() {
                if i == *at {
                    out.push_str(&line.replacen("[ ]", "[x]", 1));
                } else {
                    out.push_str(line);
                }
            }
            Ok((out, it.text.clone()))
        }
        several => Err(format!("Several fit: {}. Ask the user which one.", several.iter().map(|(_, it)| it.text.as_str()).collect::<Vec<_>>().join("; "))),
    }
}

// ── What the brain is told ────────────────────────────────────────────────────

fn ago(now: u64, at: u64) -> String {
    let past = now.saturating_sub(at);
    match past {
        _ if at == 0 => "at an unknown time".into(),
        0..=3599 => "in the last hour".into(),
        3600..=86_399 => format!("{} h ago", past / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", past / 86_400),
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn project_line(p: &Project, now: u64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !p.branch.is_empty() {
        parts.push(format!("on {}", p.branch));
    }
    if p.changed > 0 {
        parts.push(format!("{} not committed", count(p.changed, "file", "files")));
    }
    if p.ahead > 0 {
        parts.push(format!("{} not pushed", count(p.ahead, "commit", "commits")));
    }
    if let Some(last) = p.commits.first() {
        parts.push(format!("last commit {}: {}", ago(now, last.at), cut(&last.subject, SUBJECT_CHARS)));
    }
    let mut line = format!("- {} ({})", p.name, p.path.display());
    if !parts.is_empty() {
        line.push_str(": ");
        line.push_str(&parts.join(", "));
    }
    line.push('\n');
    line
}

fn folder_line(f: &Plain, now: u64) -> String {
    if f.files.is_empty() {
        return format!("- {} ({}), a folder: nothing changed in the last 30 days\n", f.name, f.path.display());
    }
    let files: Vec<String> = f.files.iter().map(|(name, at)| format!("{name} ({})", ago(now, *at))).collect();
    format!("- {} ({}), a folder: changed lately: {}\n", f.name, f.path.display(), files.join(", "))
}

/// The list and the folders in a few lines, `max` bytes or so: what is left
/// to do first, then the projects worked in lately, the latest first.
fn brief(items: &[Item], places: &[Place], now: u64, max: usize) -> String {
    let mut out = String::new();
    let open: Vec<&Item> = items.iter().filter(|i| !i.done).collect();
    if open.is_empty() {
        out.push_str("To do: nothing noted.\n");
    } else {
        out.push_str("To do:\n");
        for (n, it) in open.iter().enumerate() {
            if n == MAX_TODO || out.len() > max / 2 {
                out.push_str(&format!("- and {} more in the file\n", open.len() - n));
                break;
            }
            out.push_str(&format!("- {}\n", cut(&it.text, TODO_CHARS)));
        }
    }
    let since = now.saturating_sub(ACTIVE);
    let mut projects: Vec<&Project> = places.iter().filter_map(|p| if let Place::Project(p) = p { Some(p) } else { None }).collect();
    let all = projects.len();
    projects.retain(|p| p.touched >= since);
    projects.sort_by(|a, b| b.touched.cmp(&a.touched).then_with(|| a.name.cmp(&b.name)));
    if !projects.is_empty() {
        out.push_str("Projects worked in lately, the latest first:\n");
        let mut left: Vec<&str> = Vec::new();
        for p in &projects {
            let line = project_line(p, now);
            if out.len() + line.len() > max {
                left.push(&p.name);
            } else {
                out.push_str(&line);
            }
        }
        if !left.is_empty() {
            out.push_str(&format!("Also worked in lately: {}.\n", left.join(", ")));
        }
    } else if all > 0 {
        out.push_str("No project was worked in over the last 30 days.\n");
    }
    for place in places {
        if let Place::Folder(f) = place {
            let line = folder_line(f, now);
            if out.len() + line.len() <= max + 400 {
                out.push_str(&line);
            }
        }
    }
    out
}

/// For the brain's instructions: "" until the user named a folder.
pub fn for_prompt(settings: &Settings, max: usize) -> String {
    if !on(settings) {
        return String::new();
    }
    let list = std::fs::read_to_string(file()).unwrap_or_default();
    let mut out = brief(&items(&list), &places(&folders(settings)), now(), max);
    // What they scheduled in Claude, if they asked for that too (schedules.rs).
    out.push_str(&crate::schedules::for_prompt(settings, max / 2));
    out
}

/// One project in full: its last ten commits and the files not committed.
fn detail(p: &Project, now: u64) -> String {
    // Asked again: a project left alone was not asked its status, and one
    // worked in may have moved since the folders were looked at.
    let status = git(&p.path, &STATUS);
    let mut p = p.clone();
    if let Some(status) = &status {
        (p.branch, p.ahead, p.changed) = parse_status(status);
    }
    let mut out = project_line(&p, now);
    if let Some(log) = git(&p.path, &log_args("-10")) {
        out.push_str("Last commits:\n");
        for c in parse_log(&log) {
            out.push_str(&format!("- {}: {}\n", ago(now, c.at), cut(&c.subject, 120)));
        }
    }
    if p.changed > 0 {
        if let Some(status) = &status {
            let paths = changed_paths(status);
            out.push_str("Not committed:\n");
            for path in paths.iter().take(30) {
                out.push_str(&format!("- {path}\n"));
            }
            if paths.len() > 30 {
                out.push_str(&format!("- and {} more\n", paths.len() - 30));
            }
        }
    }
    out
}

/// The project `wanted` names: by its name, then by a part of it.
pub(crate) fn find<'a>(places: &'a [Place], wanted: &str) -> Option<&'a Project> {
    let wanted = wanted.trim().to_lowercase();
    let projects = || places.iter().filter_map(|p| if let Place::Project(p) = p { Some(p) } else { None });
    if wanted.is_empty() {
        return None;
    }
    projects().find(|p| p.name.to_lowercase() == wanted).or_else(|| projects().find(|p| p.name.to_lowercase().contains(&wanted) || wanted.contains(&p.name.to_lowercase())))
}

/// Where a session may start (agent/launch.rs), by its name and then by a
/// part of it: a project, or a folder the user named for their work that
/// holds no git project, which is theirs to work in all the same. A project
/// comes first.
pub(crate) fn startable<'a>(places: &'a [Place], wanted: &str) -> Option<(&'a str, &'a Path)> {
    if let Some(p) = find(places, wanted) {
        return Some((p.name.as_str(), p.path.as_path()));
    }
    let wanted = wanted.trim().to_lowercase();
    if wanted.is_empty() {
        return None;
    }
    // A drive named whole has no name to be asked by.
    let folders = || places.iter().filter_map(|p| if let Place::Folder(f) = p { Some(f) } else { None }).filter(|f| !f.name.is_empty());
    folders()
        .find(|f| f.name.to_lowercase() == wanted)
        .or_else(|| folders().find(|f| f.name.to_lowercase().contains(&wanted) || wanted.contains(&f.name.to_lowercase())))
        .map(|f| (f.name.as_str(), f.path.as_path()))
}

/// Their names, for the brain to ask which one.
pub(crate) fn startable_names(places: &[Place]) -> Vec<&str> {
    places
        .iter()
        .map(|p| match p {
            Place::Project(p) => p.name.as_str(),
            Place::Folder(f) => f.name.as_str(),
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// The `work` tool: what goes back to the brain, and whether the list changed.
pub fn for_brain(settings: &Settings, input: &Value) -> (String, bool) {
    let path = file();
    let read = || std::fs::read_to_string(&path).unwrap_or_default();
    let write = |text: &str| crate::wallpaper::write_atomic(&path, text.as_bytes()).is_ok();
    match input["action"].as_str().unwrap_or("brief") {
        "add" => {
            let mut text = cut(input["text"].as_str().unwrap_or(""), TODO_CHARS);
            if text.is_empty() {
                return ("Nothing to note: ask the user what must be done.".into(), false);
            }
            let project = cut(input["project"].as_str().unwrap_or(""), 60);
            if !project.is_empty() && !text.to_lowercase().starts_with(&project.to_lowercase()) {
                text = format!("{project}: {text}");
            }
            if write(&added(&read(), &crate::i18n::t("Your work"), &text)) {
                (format!("Noted in the list: {text}"), true)
            } else {
                ("The list couldn't be written.".into(), false)
            }
        }
        "done" => match ticked(&read(), input["text"].as_str().unwrap_or("")) {
            Ok((file, text)) if write(&file) => (format!("Ticked in the list: {text}"), true),
            Ok(_) => ("The list couldn't be written.".into(), false),
            Err(why) => (why, false),
        },
        "project" => {
            let places = places(&folders(settings));
            match find(&places, input["name"].as_str().unwrap_or("")) {
                Some(p) => (detail(p, now()), false),
                None => {
                    let names: Vec<&str> = places.iter().filter_map(|p| if let Place::Project(p) = p { Some(p.name.as_str()) } else { None }).collect();
                    (format!("No project of that name. The projects are: {}.", names.join(", ")), false)
                }
            }
        }
        _ => {
            let mut out = brief(&items(&read()), &places(&folders(settings)), now(), TOOL_CHARS);
            out.push_str(&crate::schedules::for_prompt(settings, TOOL_CHARS / 2));
            (out, false)
        }
    }
}

// ── For the Ciel window ───────────────────────────────────────────────────────

/// Where the list is, to show under the folders.
#[tauri::command]
pub fn work_file() -> String {
    file().to_string_lossy().to_string()
}

/// Opens the list in the user's editor, made first if it isn't there yet.
#[tauri::command]
pub fn work_open() {
    let path = file();
    if !path.exists() {
        let _ = crate::wallpaper::write_atomic(&path, format!("# {}\n\n", crate::i18n::t("Your work")).as_bytes());
    }
    crate::platform::open_url(&path.to_string_lossy());
}

/// Whether each of these folders is there.
#[tauri::command]
pub async fn work_folders_exist(paths: Vec<String>) -> Vec<bool> {
    tauri::async_runtime::spawn_blocking(move || paths.iter().map(|p| !p.trim().is_empty() && Path::new(p.trim()).is_dir()).collect())
        .await
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;
    const DAY: u64 = 86_400;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ciel-work-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_status_gives_the_branch_what_is_not_pushed_and_what_is_not_committed() {
        let out = "# branch.oid 238e38d0\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -0\n\
1 .M N... 100644 100644 100644 aaa bbb src/main.rs\n\
2 R. N... 100644 100644 100644 aaa bbb R100 src/new name.rs\tsrc/old.rs\n\
u UU N... 100644 100644 100644 100644 aaa bbb ccc both.rs\n\
? .claude/\n! target/\n";
        assert_eq!(parse_status(out), ("main".to_string(), 2, 4));
        assert_eq!(changed_paths(out), ["src/main.rs", "src/new name.rs", "both.rs", ".claude/"]);
        // No upstream, nothing changed, a detached head.
        assert_eq!(parse_status("# branch.oid abc\n# branch.head (detached)\n"), ("(detached)".to_string(), 0, 0));
        assert_eq!(parse_status(""), (String::new(), 0, 0));
    }

    #[test]
    fn the_log_gives_each_commit_its_time_and_first_line() {
        let commits = parse_log("1799990000\tThe microphone keeps\ta fifth of a second\n1799900000\tA new look for Controls\nnot a commit\n");
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0], Commit { at: 1_799_990_000, subject: "The microphone keeps\ta fifth of a second".into() });
        assert_eq!(commits[1].subject, "A new look for Controls");
    }

    #[test]
    fn the_list_is_a_markdown_checklist_the_user_can_edit() {
        let file = "# Your work\r\n\r\nSome words of mine.\r\n- [ ] ciel: ship the work file\r\n  * [x] Ahidaho: renew the certificate\r\n- [X] done too\r\n- [] not one\r\n- [ ] \r\n- plain\r\n";
        assert_eq!(
            items(file),
            [
                Item { done: false, text: "ciel: ship the work file".into() },
                Item { done: true, text: "Ahidaho: renew the certificate".into() },
                Item { done: true, text: "done too".into() },
            ]
        );
    }

    #[test]
    fn a_thing_to_do_is_added_at_the_end_and_nothing_else_moves() {
        assert_eq!(added("", "Your work", "call the bank"), "# Your work\n\n- [ ] call the bank\n");
        assert_eq!(added("# Mine\r\n\r\n- [x] one", "Your work", "two"), "# Mine\r\n\r\n- [x] one\r\n- [ ] two\r\n");
        assert_eq!(added("notes\n", "Your work", "three"), "notes\n- [ ] three\n");
    }

    #[test]
    fn only_the_one_line_meant_is_ticked() {
        let file = "# Work\r\n- [ ] ciel: ship the work file\r\n- [ ] ciel: write the changelog\r\n- [x] ciel: old\r\nmy notes\r\n";
        let (after, text) = ticked(file, "Ship the WORK file").unwrap();
        assert_eq!(text, "ciel: ship the work file");
        assert_eq!(after, "# Work\r\n- [x] ciel: ship the work file\r\n- [ ] ciel: write the changelog\r\n- [x] ciel: old\r\nmy notes\r\n");
        // By its words in any order.
        assert_eq!(ticked(file, "changelog ciel").unwrap().1, "ciel: write the changelog");
        // Several fit, or none: nothing is ticked, and the brain is told what there is.
        let several = ticked(file, "ciel").unwrap_err();
        assert!(several.starts_with("Several fit") && several.contains("write the changelog"), "{several}");
        let none = ticked(file, "the bank").unwrap_err();
        assert!(none.contains("ciel: ship the work file; ciel: write the changelog"), "{none}");
        // What is done already is never looked at.
        assert!(ticked(file, "old").is_err());
        assert!(ticked("- [x] all done\n", "all done").is_err());
    }

    fn a_project(name: &str, days: u64) -> Project {
        Project {
            name: name.into(),
            path: PathBuf::from(format!("/work/{name}")),
            branch: "main".into(),
            changed: 0,
            ahead: 0,
            commits: vec![Commit { at: NOW - days * DAY, subject: format!("Work on {name}") }],
            touched: NOW - days * DAY,
        }
    }

    #[test]
    fn the_brief_says_what_is_left_then_the_projects_worked_in_lately() {
        let items = [Item { done: false, text: "ciel: ship the work file".into() }, Item { done: true, text: "old".into() }];
        let mut ciel = a_project("ciel", 0);
        ciel.changed = 3;
        ciel.ahead = 1;
        ciel.touched = NOW - 600;
        ciel.commits[0].at = NOW - 600;
        let places = [
            Place::Project(a_project("asleep", 90)),
            Place::Project(a_project("brollyhub", 2)),
            Place::Project(ciel),
            Place::Folder(Plain { name: "AYIHALABS".into(), path: PathBuf::from("/desk/AYIHALABS"), files: vec![("ops/plan.md".into(), NOW - 3 * DAY)] }),
        ];
        let said = brief(&items, &places, NOW, PROMPT_CHARS);
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(lines[0], "To do:");
        assert_eq!(lines[1], "- ciel: ship the work file");
        assert_eq!(lines[2], "Projects worked in lately, the latest first:");
        assert_eq!(lines[3], "- ciel (/work/ciel): on main, 3 files not committed, 1 commit not pushed, last commit in the last hour: Work on ciel");
        assert_eq!(lines[4], "- brollyhub (/work/brollyhub): on main, last commit 2 days ago: Work on brollyhub");
        assert_eq!(lines[5], "- AYIHALABS (/desk/AYIHALABS), a folder: changed lately: ops/plan.md (3 days ago)");
        assert_eq!(lines.len(), 6, "a project asleep for 90 days and what is done are left out");
    }

    #[test]
    fn a_long_brief_names_the_projects_it_has_no_room_for() {
        let places: Vec<Place> = (0..30).map(|i| Place::Project(a_project(&format!("project-{i:02}"), i))).collect();
        let said = brief(&[], &places, NOW, 600);
        assert!(said.starts_with("To do: nothing noted.\n"));
        assert!(said.contains("- project-00 "));
        assert!(said.contains("Also worked in lately: "), "{said}");
        assert!(said.contains("project-29"));
        assert!(said.len() < 1200, "{}", said.len());
        // And with nothing recent, it says so rather than nothing.
        let old = [Place::Project(a_project("asleep", 45))];
        assert!(brief(&[], &old, NOW, 600).contains("No project was worked in over the last 30 days."));
    }

    #[test]
    fn a_folder_of_projects_gives_its_projects_and_a_plain_folder_its_late_files() {
        let root = temp("look");
        // No git here answers for these two: they are still projects, by name.
        for name in ["alpha", "beta"] {
            std::fs::create_dir_all(root.join(name).join(".git")).unwrap();
        }
        std::fs::create_dir_all(root.join("node_modules").join(".git")).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        let found = look(&root, now());
        let names: Vec<String> = found.iter().map(|p| if let Place::Project(p) = p { p.name.clone() } else { "a folder".into() }).collect();
        assert_eq!(names, ["alpha", "beta"]);

        let plain_root = temp("plain");
        std::fs::create_dir_all(plain_root.join("ops").join("deep").join("deeper").join("too deep")).unwrap();
        std::fs::create_dir_all(plain_root.join(".hidden")).unwrap();
        std::fs::write(plain_root.join("README.md"), "x").unwrap();
        std::fs::write(plain_root.join("ops").join("plan.md"), "x").unwrap();
        std::fs::write(plain_root.join(".hidden").join("secret.md"), "x").unwrap();
        std::fs::write(plain_root.join("~$draft.docx"), "x").unwrap();
        std::fs::write(plain_root.join("ops").join("deep").join("deeper").join("too deep").join("far.md"), "x").unwrap();
        let found = look(&plain_root, now());
        let [Place::Folder(folder)] = found.as_slice() else { panic!("{found:?}") };
        let mut files: Vec<&str> = folder.files.iter().map(|(name, _)| name.as_str()).collect();
        files.sort();
        assert_eq!(files, ["README.md", "ops/plan.md"]);
        // Long ago, nothing here was changed lately.
        assert!(plain(&plain_root, now() + 2 * ACTIVE).files.is_empty());
        assert!(look(&root.join("not there"), now()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&plain_root);
    }

    #[test]
    fn nothing_is_looked_at_until_a_folder_is_named() {
        let mut settings = Settings::default();
        assert!(!on(&settings));
        assert_eq!(for_prompt(&settings, PROMPT_CHARS), "");
        settings.work_folders = vec!["  ".into(), " D:\\work ".into(), "d:\\WORK".into()];
        assert!(on(&settings));
        assert_eq!(folders(&settings), ["D:\\work"]);
    }

    #[test]
    fn a_project_is_found_by_its_name_or_a_part_of_it() {
        let places = [Place::Project(a_project("brollyhub-Main", 1)), Place::Project(a_project("brollyhub-academy", 2)), Place::Project(a_project("ciel", 0))];
        assert_eq!(find(&places, "Ciel").unwrap().name, "ciel");
        assert_eq!(find(&places, "brollyhub-academy").unwrap().name, "brollyhub-academy");
        assert_eq!(find(&places, "academy").unwrap().name, "brollyhub-academy");
        assert_eq!(find(&places, "the ciel project").unwrap().name, "ciel");
        assert!(find(&places, "verilex").is_none() && find(&places, " ").is_none());
    }

    #[test]
    fn a_session_may_start_in_a_project_or_in_a_work_folder_that_holds_none() {
        let folder = |name: &str, path: &str| Place::Folder(Plain { name: name.into(), path: PathBuf::from(path), files: Vec::new() });
        let places = [Place::Project(a_project("ciel", 0)), folder("AYIHALABS", "/desk/AYIHALABS"), folder("", "/"), folder("ciel-notes", "/desk/ciel-notes")];
        assert_eq!(startable(&places, "ayihalabs"), Some(("AYIHALABS", Path::new("/desk/AYIHALABS"))));
        assert_eq!(startable(&places, "the AYIHALABS folder").unwrap().0, "AYIHALABS");
        assert_eq!(startable(&places, "notes").unwrap().0, "ciel-notes");
        // A project comes before a folder that only holds its name.
        assert_eq!(startable(&places, "ciel"), Some(("ciel", Path::new("/work/ciel"))));
        // A folder with no name is never the one meant.
        assert!(startable(&places, "verilex").is_none() && startable(&places, " ").is_none());
        assert_eq!(startable_names(&places), ["ciel", "AYIHALABS", "ciel-notes"]);
    }

    #[test]
    fn times_are_said_the_way_people_say_them() {
        assert_eq!(ago(NOW, NOW - 60), "in the last hour");
        assert_eq!(ago(NOW, NOW - 5 * 3600), "5 h ago");
        assert_eq!(ago(NOW, NOW - 30 * 3600), "yesterday");
        assert_eq!(ago(NOW, NOW - 9 * DAY), "9 days ago");
        assert_eq!(ago(NOW, 0), "at an unknown time");
    }
}
