// The Finance pill: personal spending, kept in a folder of two plain files.
//
// The folder is shared with a small dashboard app of the user's own, so the
// files are a contract and Ciel reads and writes them exactly as it does:
//
// - `expenses.csv`: UTF-8 with a BOM, the header
//   `id,date,amount,currency,category,note,created_at`, one row per expense,
//   sorted by (date, created_at). A row whose amount is not a number is left
//   out when reading — and left in the file, as it was, when writing.
// - `settings.json`: the base currency, the monthly budget, the exchange rates
//   (1 unit of a currency = N units of the base) and the categories. Ciel only
//   ever reads it; the one time it writes it is when it creates a new folder.
//
// Adding an expense reads the CSV again right before writing, so a row the
// dashboard has just added is never lost, then writes the whole sorted file to
// `expenses.csv.tmp` and renames it over the original — retrying for a moment
// while another program (OneDrive, the dashboard) has the file open.
//
// No network call of any kind: the rates come from settings.json only, and an
// amount in a currency without a rate is left out of the totals rather than
// guessed.

use std::collections::{BTreeMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::Shared;

pub const EXPENSES_FILE: &str = "expenses.csv";
pub const SETTINGS_FILE: &str = "settings.json";
/// The columns of expenses.csv, in the order a new file gets them.
pub const COLUMNS: [&str; 7] = ["id", "date", "amount", "currency", "category", "note", "created_at"];
pub const MAX_CATEGORY: usize = 60;
pub const MAX_NOTE: usize = 200;
const BOM: &str = "\u{feff}";
const DEFAULT_BASE: &str = "USD";
/// The categories of a new folder. Generic on purpose: they are everybody's.
pub const DEFAULT_CATEGORIES: [&str; 12] = [
    "Housing",
    "Bills & Utilities",
    "Groceries",
    "Eating out",
    "Transport",
    "Phone & Internet",
    "Subscriptions",
    "Health",
    "Clothing",
    "Learning",
    "Entertainment",
    "Other",
];
/// How many entries the card lists, newest first.
const RECENT: usize = 5;
/// How many categories the card ranks.
const TOP: usize = 3;
/// A file another program holds is tried again this many times, this far apart.
const ATTEMPTS: u32 = 20;
const PAUSE: Duration = Duration::from_millis(150);
/// A `.tmp` left there is waited for this many attempts (someone may be
/// writing it), then taken as left over from a crash and written over.
const TMP_PATIENCE: u32 = 4;

// ── The folder ────────────────────────────────────────────────────────────────

/// `<Ciel's config dir>/finance`, where a new user's data lives.
pub fn default_folder() -> PathBuf {
    crate::settings::config_dir().join("finance")
}

/// The folder a `financeFolder` setting points at: the default when it is
/// empty or not a full path.
pub fn folder_from(setting: &str) -> PathBuf {
    let path = Path::new(setting.trim());
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        default_folder()
    }
}

/// The folder the user picked in Settings (or the default one).
pub fn folder(settings: &crate::settings::Settings) -> PathBuf {
    folder_from(&settings.finance_folder)
}

/// Makes a folder that is not there yet, with a settings.json of generic
/// defaults and an empty expenses.csv. A folder that exists is left as it is,
/// whatever it holds: settings.json is the dashboard's, never Ciel's to write.
pub fn ensure_folder(dir: &Path) -> std::io::Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let settings = serde_json::to_vec_pretty(&default_settings_json())
        .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
    write_new(&dir.join(SETTINGS_FILE), &settings)?;
    let header = format!("{BOM}{}\r\n", COLUMNS.join(","));
    write_new(&dir.join(EXPENSES_FILE), header.as_bytes())
}

/// Writes `bytes` to a file that must not exist yet.
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err),
    }
}

/// settings.json for a new folder: no budget (0), the dollar as the base.
fn default_settings_json() -> Value {
    json!({
        "base_currency": DEFAULT_BASE,
        "monthly_income": 0,
        "monthly_budget": 0,
        "savings_balance": 0,
        "savings_currency": DEFAULT_BASE,
        "rates": { DEFAULT_BASE: 1 },
        "rates_updated": null,
        "categories": DEFAULT_CATEGORIES,
    })
}

// ── settings.json ─────────────────────────────────────────────────────────────

/// What settings.json says, each value falling back to its default on its own
/// when it is missing or not what it should be.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinanceSettings {
    pub base_currency: String,
    pub monthly_income: f64,
    /// 0 means no budget is set.
    pub monthly_budget: f64,
    pub savings_balance: f64,
    pub savings_currency: String,
    /// 1 unit of the currency = N units of the base; the base itself is 1.
    pub rates: BTreeMap<String, f64>,
    pub rates_updated: Option<String>,
    pub categories: Vec<String>,
}

impl Default for FinanceSettings {
    fn default() -> Self {
        parse_settings(b"")
    }
}

impl FinanceSettings {
    /// What one unit of `currency` is worth in the base currency, when known.
    pub fn rate(&self, currency: &str) -> Option<f64> {
        if currency == self.base_currency {
            return Some(1.0);
        }
        self.rates.get(currency).copied()
    }

    /// The currencies an expense may be in: the base first, then the others.
    pub fn currencies(&self) -> Vec<String> {
        let mut list = vec![self.base_currency.clone()];
        list.extend(self.rates.keys().filter(|c| **c != self.base_currency).cloned());
        list
    }
}

/// Three capital letters, as in ISO 4217.
fn is_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
}

fn number(value: Option<&Value>) -> Option<f64> {
    let n = match value? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(BOM.as_bytes()).unwrap_or(bytes)
}

/// settings.json from its bytes. Nothing in it is required: an empty or broken
/// file gives the defaults.
pub fn parse_settings(bytes: &[u8]) -> FinanceSettings {
    let root: Value = serde_json::from_slice(strip_bom(bytes)).unwrap_or(Value::Null);
    let get = |key: &str| root.get(key);
    let code = |key: &str| {
        get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| is_currency_code(c))
            .map(str::to_string)
    };
    let base_currency = code("base_currency").unwrap_or_else(|| DEFAULT_BASE.to_string());

    let mut rates = BTreeMap::new();
    if let Some(map) = get("rates").and_then(Value::as_object) {
        for (currency, value) in map {
            let currency = currency.trim();
            if let Some(rate) = number(Some(value)).filter(|r| *r > 0.0) {
                if is_currency_code(currency) {
                    rates.insert(currency.to_string(), rate);
                }
            }
        }
    }
    rates.insert(base_currency.clone(), 1.0);

    let mut categories: Vec<String> = Vec::new();
    if let Some(list) = get("categories").and_then(Value::as_array) {
        for name in list.iter().filter_map(Value::as_str).map(str::trim) {
            if !name.is_empty() && !categories.iter().any(|c| c == name) {
                categories.push(name.to_string());
            }
        }
    }
    if categories.is_empty() {
        categories = DEFAULT_CATEGORIES.iter().map(|c| c.to_string()).collect();
    }

    FinanceSettings {
        monthly_income: number(get("monthly_income")).unwrap_or(0.0),
        monthly_budget: number(get("monthly_budget")).filter(|b| *b > 0.0).unwrap_or(0.0),
        savings_balance: number(get("savings_balance")).unwrap_or(0.0),
        savings_currency: code("savings_currency").unwrap_or_else(|| base_currency.clone()),
        rates,
        rates_updated: get("rates_updated").and_then(Value::as_str).map(str::to_string),
        categories,
        base_currency,
    }
}

/// The folder's settings.json, or the defaults when there is none.
pub fn read_settings(dir: &Path) -> std::io::Result<FinanceSettings> {
    match std::fs::read(dir.join(SETTINGS_FILE)) {
        Ok(bytes) => Ok(parse_settings(&bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(FinanceSettings::default()),
        Err(err) => Err(err),
    }
}

// ── expenses.csv ──────────────────────────────────────────────────────────────

/// One expense, as the card and « OK Ciel » see it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expense {
    pub id: String,
    pub date: String,
    pub amount: f64,
    pub currency: String,
    pub category: String,
    pub note: String,
    pub created_at: String,
}

/// expenses.csv as it was read: the header and every record, field for field
/// as written, so writing it back changes nothing but the row added.
#[derive(Debug, Clone, PartialEq)]
struct Sheet {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    /// The file's own line ending, kept when it is written back.
    newline: &'static str,
}

/// Where each of the contract's columns sits in the file's header.
type Columns = [usize; 7];

impl Sheet {
    fn empty() -> Self {
        Sheet {
            header: COLUMNS.iter().map(|c| c.to_string()).collect(),
            rows: Vec::new(),
            newline: "\r\n",
        }
    }

    fn parse(text: &str) -> Self {
        let text = text.strip_prefix(BOM).unwrap_or(text);
        let newline = if text.contains("\r\n") || !text.contains('\n') { "\r\n" } else { "\n" };
        let mut records = parse_csv(text).into_iter();
        let Some(header) = records.next() else { return Sheet { newline, ..Sheet::empty() } };
        Sheet { header, rows: records.collect(), newline }
    }

    /// None when the header lacks one of the contract's columns.
    fn columns(&self) -> Option<Columns> {
        let mut at = [0; 7];
        for (i, name) in COLUMNS.iter().enumerate() {
            at[i] = self.header.iter().position(|h| h.trim().eq_ignore_ascii_case(name))?;
        }
        Some(at)
    }

    /// The expenses whose amount is a number, and how many rows were not.
    fn expenses(&self, at: &Columns) -> (Vec<Expense>, usize) {
        let mut out = Vec::new();
        let mut skipped = 0;
        for row in &self.rows {
            let field = |i: usize| row.get(at[i]).map(|s| s.trim().to_string()).unwrap_or_default();
            let Some(amount) = field(2).parse::<f64>().ok().filter(|a| a.is_finite()) else {
                skipped += 1;
                continue;
            };
            out.push(Expense {
                id: field(0),
                date: field(1),
                amount,
                currency: field(3).to_ascii_uppercase(),
                category: field(4),
                note: field(5),
                created_at: field(6),
            });
        }
        (out, skipped)
    }

    /// The whole file: BOM, header, then every row, in (date, created_at) order.
    fn render(&self, at: &Columns) -> String {
        let key = |row: &Vec<String>| {
            let get = |i: usize| row.get(at[i]).map(|s| s.trim().to_string()).unwrap_or_default();
            (get(1), get(6))
        };
        let mut rows: Vec<&Vec<String>> = self.rows.iter().collect();
        rows.sort_by_cached_key(|row| key(*row)); // stable: equal keys keep their order
        let mut out = String::from(BOM);
        for record in std::iter::once(&self.header).chain(rows) {
            let fields: Vec<String> = record.iter().map(|f| csv_field(f)).collect();
            out.push_str(&fields.join(","));
            out.push_str(self.newline);
        }
        out
    }
}

/// RFC 4180: commas, double quotes doubled inside quoted fields, and line
/// breaks inside them. Blank lines are not records.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    // Whether the record holds anything yet (a lone empty line does not).
    let mut started = false;
    let end_record = |record: &mut Vec<String>, field: &mut String, records: &mut Vec<Vec<String>>, started: &mut bool| {
        if *started {
            record.push(std::mem::take(field));
            records.push(std::mem::take(record));
        }
        *started = false;
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                started = true;
            }
            ',' => {
                record.push(std::mem::take(&mut field));
                started = true;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                end_record(&mut record, &mut field, &mut records, &mut started);
            }
            '\n' => end_record(&mut record, &mut field, &mut records, &mut started),
            _ => {
                field.push(c);
                started = true;
            }
        }
    }
    end_record(&mut record, &mut field, &mut records, &mut started);
    records
}

/// A field as Python's csv module writes it (QUOTE_MINIMAL).
fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// The folder's expenses.csv, or None when there is none yet.
fn read_sheet(dir: &Path) -> std::io::Result<Option<Sheet>> {
    match std::fs::read(dir.join(EXPENSES_FILE)) {
        Ok(bytes) => Ok(Some(Sheet::parse(&String::from_utf8_lossy(&bytes)))),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum FinanceError {
    /// Not a number above 0 with at most two decimals.
    Amount,
    /// Not a real YYYY-MM-DD date.
    Date,
    /// Not three capital letters, or no rate for it in settings.json.
    Currency(String),
    /// Empty, or longer than MAX_CATEGORY characters.
    Category,
    /// Longer than MAX_NOTE characters.
    Note,
    /// expenses.csv lacks one of the contract's columns: it is left alone.
    Columns,
    /// The folder could not be read or written.
    Io(String),
}

impl std::fmt::Display for FinanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::i18n::{t, tf};
        let text = match self {
            FinanceError::Amount => t("Enter an amount above 0 with at most 2 decimals."),
            FinanceError::Date => t("Enter the date as YYYY-MM-DD."),
            FinanceError::Currency(code) => tf("No exchange rate for {currency} in settings.json.", &[("currency", code)]),
            FinanceError::Category => t("Pick a category (60 characters at most)."),
            FinanceError::Note => t("Keep the note to 200 characters."),
            FinanceError::Columns => t("expenses.csv doesn't have the expected columns, so Ciel leaves it alone."),
            FinanceError::Io(error) => tf("Could not use the finance folder: {error}", &[("error", error)]),
        };
        f.write_str(&text)
    }
}

impl From<std::io::Error> for FinanceError {
    fn from(err: std::io::Error) -> Self {
        FinanceError::Io(err.to_string())
    }
}

// ── Dates ─────────────────────────────────────────────────────────────────────

/// A real calendar day written YYYY-MM-DD.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| {
        b[r.clone()].iter().all(u8::is_ascii_digit).then(|| s[r].parse::<u32>().ok()).flatten()
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=days).contains(&day)
}

/// The moment an expense is added: its default date and its `created_at`.
#[derive(Debug, Clone)]
pub struct Now {
    /// YYYY-MM-DD, local.
    pub date: String,
    /// YYYY-MM-DDTHH:MM:SS, local, no time zone — as the dashboard writes it.
    pub created_at: String,
}

impl Now {
    pub fn local() -> Self {
        let t = crate::platform::local_time();
        let date = format!("{:04}-{:02}-{:02}", t.year, t.month, t.day);
        let created_at = format!("{date}T{:02}:{:02}:{:02}", t.hour, t.minute, t.second);
        Now { date, created_at }
    }
}

// ── The summary ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    pub category: String,
    /// In the base currency.
    pub amount: f64,
}

/// The current month at a glance, every amount in the base currency.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub base_currency: String,
    /// YYYY-MM
    pub month: String,
    /// YYYY-MM-DD
    pub today: String,
    pub spent_month: f64,
    pub spent_today: f64,
    /// None when no budget is set.
    pub budget: Option<f64>,
    /// Budget minus what was spent this month; negative once over.
    pub remaining: Option<f64>,
    /// Share of the budget spent, in percent (may pass 100).
    pub percent_used: Option<f64>,
    pub top_categories: Vec<CategoryTotal>,
    /// The newest entries first, in their own currency.
    pub recent: Vec<Expense>,
    /// Every entry in the file whose amount is a number.
    pub entry_count: usize,
    /// The entries dated this month.
    pub month_count: usize,
    /// Rows left out because their amount is not a number.
    pub skipped_rows: usize,
    /// Currencies of this month's entries that have no rate, left out of the totals.
    pub unconverted: Vec<String>,
    /// What the add form offers.
    pub currencies: Vec<String>,
    pub categories: Vec<String>,
    pub monthly_income: f64,
    pub savings_balance: f64,
    pub savings_currency: String,
    pub rates_updated: Option<String>,
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// The summary of `expenses` on day `today` (YYYY-MM-DD).
pub fn summarize(settings: &FinanceSettings, mut expenses: Vec<Expense>, skipped: usize, today: &str) -> Summary {
    let month = today.get(..7).unwrap_or_default().to_string();
    let in_month = |e: &Expense| e.date.len() == 10 && e.date.starts_with(&month) && e.date.as_bytes()[7] == b'-';

    let mut spent_month = 0.0;
    let mut spent_today = 0.0;
    let mut month_count = 0;
    let mut by_category: BTreeMap<String, f64> = BTreeMap::new();
    let mut unconverted: Vec<String> = Vec::new();
    for e in expenses.iter().filter(|e| in_month(e)) {
        month_count += 1;
        let Some(rate) = settings.rate(&e.currency) else {
            if !unconverted.contains(&e.currency) {
                unconverted.push(e.currency.clone());
            }
            continue;
        };
        let base = e.amount * rate;
        spent_month += base;
        if e.date == today {
            spent_today += base;
        }
        *by_category.entry(e.category.clone()).or_default() += base;
    }
    unconverted.sort();

    let mut top: Vec<CategoryTotal> = by_category
        .into_iter()
        .map(|(category, amount)| CategoryTotal { category, amount: round2(amount) })
        .collect();
    // Biggest first; equal amounts by name (the map's order, kept by a stable sort).
    top.sort_by(|a, b| b.amount.total_cmp(&a.amount));
    top.truncate(TOP);

    let entry_count = expenses.len();
    expenses.sort_by(|a, b| (&a.date, &a.created_at).cmp(&(&b.date, &b.created_at)));
    let recent: Vec<Expense> = expenses.into_iter().rev().take(RECENT).collect();

    let budget = (settings.monthly_budget > 0.0).then_some(settings.monthly_budget);
    let spent_month = round2(spent_month);
    Summary {
        base_currency: settings.base_currency.clone(),
        month,
        today: today.to_string(),
        spent_month,
        spent_today: round2(spent_today),
        budget,
        remaining: budget.map(|b| round2(b - spent_month)),
        percent_used: budget.map(|b| spent_month / b * 100.0),
        top_categories: top,
        recent,
        entry_count,
        month_count,
        skipped_rows: skipped,
        unconverted,
        currencies: settings.currencies(),
        categories: settings.categories.clone(),
        monthly_income: settings.monthly_income,
        savings_balance: settings.savings_balance,
        savings_currency: settings.savings_currency.clone(),
        rates_updated: settings.rates_updated.clone(),
    }
}

/// The folder's summary for `today`. Reads only: nothing is created or written.
pub fn summary_on(dir: &Path, today: &str) -> Result<Summary, FinanceError> {
    let settings = read_settings(dir)?;
    let (expenses, skipped) = match read_sheet(dir)? {
        Some(sheet) => {
            let at = sheet.columns().ok_or(FinanceError::Columns)?;
            sheet.expenses(&at)
        }
        None => (Vec::new(), 0),
    };
    Ok(summarize(&settings, expenses, skipped, today))
}

/// This month's summary of the finance folder `dir`, as of now.
pub fn summary(dir: &Path) -> Result<Summary, FinanceError> {
    summary_on(dir, &Now::local().date)
}

// ── Adding an expense ─────────────────────────────────────────────────────────

/// An expense to add, as the card's form (or « OK Ciel ») gives it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewExpense {
    pub amount: f64,
    pub currency: String,
    pub category: String,
    #[serde(default)]
    pub note: String,
    /// YYYY-MM-DD; today when left out.
    #[serde(default)]
    pub date: Option<String>,
}

/// One line of text: a line break would be a second line in the dashboard.
fn one_line(s: &str) -> String {
    s.split(['\r', '\n']).map(str::trim).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// At most two decimals: what is left after the cents is rounding noise.
fn at_most_two_decimals(amount: f64) -> bool {
    let cents = amount * 100.0;
    (cents - cents.round()).abs() <= 1e-6 * cents.abs().max(1.0)
}

/// The amount as it goes in the file: no exponent, no trailing zeros ("12.5").
fn amount_text(amount: f64) -> String {
    format!("{}", round2(amount))
}

/// Checks `new` against the contract and the folder's settings; the expense
/// that would be written, minus its id.
fn validate(new: &NewExpense, settings: &FinanceSettings, now: &Now) -> Result<Expense, FinanceError> {
    if !(new.amount.is_finite() && new.amount > 0.0 && new.amount < 1e12 && at_most_two_decimals(new.amount))
        || round2(new.amount) <= 0.0
    {
        return Err(FinanceError::Amount);
    }
    let date = new.date.as_deref().map(str::trim).filter(|d| !d.is_empty()).unwrap_or(&now.date).to_string();
    if !is_date(&date) {
        return Err(FinanceError::Date);
    }
    let currency = new.currency.trim().to_string();
    if !is_currency_code(&currency) || settings.rate(&currency).is_none() {
        return Err(FinanceError::Currency(currency));
    }
    let category = one_line(&new.category);
    if category.is_empty() || category.chars().count() > MAX_CATEGORY {
        return Err(FinanceError::Category);
    }
    let note = one_line(&new.note);
    if note.chars().count() > MAX_NOTE {
        return Err(FinanceError::Note);
    }
    Ok(Expense {
        id: String::new(),
        date,
        amount: round2(new.amount),
        currency,
        category,
        note,
        created_at: now.created_at.clone(),
    })
}

/// Ten lowercase hex characters, none of the `taken` ones.
fn new_id(taken: &HashSet<String>) -> String {
    use ring::rand::SecureRandom;
    let rng = ring::rand::SystemRandom::new();
    let mut counter: u64 = 0;
    loop {
        let mut bytes = [0u8; 5];
        if rng.fill(&mut bytes).is_err() {
            // Never seen in practice; the time keeps ids apart all the same.
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            let mixed = nanos ^ (u64::from(std::process::id()) << 20) ^ counter.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            bytes.copy_from_slice(&mixed.to_le_bytes()[..5]);
            counter += 1;
        }
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        if !taken.contains(&id) {
            return id;
        }
    }
}

/// Adds `new` to the folder `dir` at the moment `now`, and returns it.
pub fn add_expense_at(dir: &Path, new: &NewExpense, now: &Now) -> Result<Expense, FinanceError> {
    ensure_folder(dir)?;
    let settings = retry(|| read_settings(dir))?;
    let mut expense = validate(new, &settings, now)?;

    let path = dir.join(EXPENSES_FILE);
    let tmp = dir.join(format!("{EXPENSES_FILE}.tmp"));
    let mut last_error = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(PAUSE);
        }
        // Read again right before writing: whatever the dashboard added since
        // the card was drawn is in what gets written back.
        let mut sheet = match read_sheet(dir) {
            Ok(sheet) => sheet.unwrap_or_else(Sheet::empty),
            Err(err) => {
                last_error = Some(err);
                continue;
            }
        };
        let at = sheet.columns().ok_or(FinanceError::Columns)?;
        if expense.id.is_empty() {
            let taken: HashSet<String> =
                sheet.rows.iter().filter_map(|r| r.get(at[0])).map(|id| id.trim().to_string()).collect();
            expense.id = new_id(&taken);
        }
        let mut row = vec![String::new(); sheet.header.len()];
        let values = [
            &expense.id,
            &expense.date,
            &amount_text(expense.amount),
            &expense.currency,
            &expense.category,
            &expense.note,
            &expense.created_at,
        ];
        for (i, value) in values.into_iter().enumerate() {
            row[at[i]] = value.clone();
        }
        sheet.rows.push(row);

        // Someone may be writing the .tmp right now: give them a moment.
        if tmp.exists() && attempt < TMP_PATIENCE {
            last_error = Some(std::io::Error::new(ErrorKind::AlreadyExists, "expenses.csv.tmp is in use"));
            continue;
        }
        let written = write_then_rename(&tmp, &path, sheet.render(&at).as_bytes());
        match written {
            Ok(()) => return Ok(expense),
            Err(err) => {
                let _ = std::fs::remove_file(&tmp);
                last_error = Some(err);
            }
        }
    }
    Err(last_error.map(FinanceError::from).unwrap_or(FinanceError::Io("busy".into())))
}

/// Writes `bytes` to `tmp`, waits until they are on the disk, then puts `tmp`
/// in place of `path` in one step: a crash leaves the old file, never half of one.
fn write_then_rename(tmp: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)
}

/// Adds an expense to the finance folder `dir`, now.
pub fn add_expense(dir: &Path, new: &NewExpense) -> Result<Expense, FinanceError> {
    add_expense_at(dir, new, &Now::local())
}

/// `read` again a few times while another program holds the file.
fn retry<T>(mut read: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut attempt = 0;
    loop {
        match read() {
            Ok(value) => return Ok(value),
            Err(err) if attempt + 1 >= ATTEMPTS => return Err(err),
            Err(_) => {
                attempt += 1;
                std::thread::sleep(PAUSE);
            }
        }
    }
}

// ── Watching the files ────────────────────────────────────────────────────────

/// Changes when either file, the folder or the day does: two `stat`s, so the
/// island can ask every few seconds while the pill is on screen.
pub fn stamp(dir: &Path, today: &str) -> String {
    let file = |name: &str| match std::fs::metadata(dir.join(name)) {
        Ok(m) => {
            let modified = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("{modified}:{}", m.len())
        }
        Err(_) => "-".into(),
    };
    format!("{}|{today}|{}|{}", dir.display(), file(EXPENSES_FILE), file(SETTINGS_FILE))
}

/// What the island gets when something changed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    pub stamp: String,
    pub folder: String,
    pub summary: Option<Summary>,
    pub error: Option<String>,
}

/// None while `known` is still the folder's stamp; the fresh summary otherwise.
fn poll(dir: &Path, known: Option<&str>) -> Option<Poll> {
    let stamp = stamp(dir, &Now::local().date);
    if known == Some(stamp.as_str()) {
        return None;
    }
    let (summary, error) = match summary(dir) {
        Ok(s) => (Some(s), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Some(Poll { stamp, folder: dir.display().to_string(), summary, error })
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn configured_folder(shared: &State<'_, Shared>) -> PathBuf {
    folder(&shared.settings.lock().unwrap())
}

/// The island asks while the pill is on screen; None when nothing changed.
/// Off the main thread, like every file access here.
#[tauri::command]
pub async fn finance_poll(shared: State<'_, Shared>, stamp: Option<String>) -> Result<Option<Poll>, String> {
    let dir = configured_folder(&shared);
    tauri::async_runtime::spawn_blocking(move || poll(&dir, stamp.as_deref()))
        .await
        .map_err(|e| e.to_string())
}

/// The card's Add button. Returns the summary with the new expense in it.
#[tauri::command]
pub async fn finance_add_expense(shared: State<'_, Shared>, expense: NewExpense) -> Result<Poll, String> {
    let dir = configured_folder(&shared);
    tauri::async_runtime::spawn_blocking(move || {
        let added = add_expense(&dir, &expense).map_err(|e| e.to_string())?;
        crate::log::line(format!("finance: expense {} added", added.id));
        poll(&dir, None).ok_or_else(|| "no summary".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Settings → Integrations → Finance: what is in a folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub path: String,
    pub default_path: String,
    pub is_default: bool,
    pub exists: bool,
    pub has_expenses: bool,
    pub has_settings: bool,
}

fn folder_info(dir: &Path) -> FolderInfo {
    let default = default_folder();
    FolderInfo {
        path: dir.display().to_string(),
        default_path: default.display().to_string(),
        is_default: dir == default,
        exists: dir.is_dir(),
        has_expenses: dir.join(EXPENSES_FILE).is_file(),
        has_settings: dir.join(SETTINGS_FILE).is_file(),
    }
}

/// `path` as the setting would read it (the saved one when None).
#[tauri::command]
pub async fn finance_folder_info(shared: State<'_, Shared>, path: Option<String>) -> Result<FolderInfo, String> {
    let dir = match path {
        Some(p) => folder_from(&p),
        None => configured_folder(&shared),
    };
    tauri::async_runtime::spawn_blocking(move || folder_info(&dir)).await.map_err(|e| e.to_string())
}

/// "Open folder": a new folder is made first, with its defaults.
#[tauri::command]
pub async fn finance_open_folder(shared: State<'_, Shared>) -> Result<(), String> {
    let dir = configured_folder(&shared);
    tauri::async_runtime::spawn_blocking(move || {
        ensure_folder(&dir).map_err(|e| FinanceError::from(e).to_string())?;
        crate::platform::reveal_folder(&dir.to_string_lossy());
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory of our own; `finance` inside it is not created.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ciel-finance-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const HEADER: &str = "id,date,amount,currency,category,note,created_at";

    /// A folder like the dashboard's: settings.json and expenses.csv as given.
    fn folder_with(name: &str, settings: &str, rows: &[&str]) -> PathBuf {
        let dir = scratch(name);
        std::fs::write(dir.join(SETTINGS_FILE), settings).unwrap();
        let mut csv = format!("{BOM}{HEADER}\r\n");
        for row in rows {
            csv.push_str(row);
            csv.push_str("\r\n");
        }
        std::fs::write(dir.join(EXPENSES_FILE), csv).unwrap();
        dir
    }

    const SETTINGS: &str = r#"{
  "base_currency": "EUR",
  "monthly_income": 3000,
  "monthly_budget": 1000,
  "savings_balance": 500,
  "savings_currency": "USD",
  "rates": { "EUR": 1, "USD": 0.5, "GBP": 2 },
  "rates_updated": "2026-10-01",
  "categories": ["Food", "Rent", "Fun"]
}"#;

    fn now(date: &str, time: &str) -> Now {
        Now { date: date.into(), created_at: format!("{date}T{time}") }
    }

    fn new(amount: f64, currency: &str, category: &str, note: &str, date: Option<&str>) -> NewExpense {
        NewExpense {
            amount,
            currency: currency.into(),
            category: category.into(),
            note: note.into(),
            date: date.map(str::to_string),
        }
    }

    fn file(dir: &Path) -> String {
        String::from_utf8(std::fs::read(dir.join(EXPENSES_FILE)).unwrap()).unwrap()
    }

    // ── CSV ───────────────────────────────────────────────────────────────────

    #[test]
    fn the_csv_round_trips_with_its_bom_quotes_commas_and_line_breaks() {
        let text = format!(
            "{BOM}{HEADER}\r\n\
             a1b2c3d4e5,2026-10-02,12.5,EUR,Food,\"Bread, milk and \"\"good\"\" cheese\",2026-10-02T09:00:00\r\n\
             0000000001,2026-10-01,3,USD,Fun,\"two\nlines\",2026-10-01T20:15:00\r\n"
        );
        let sheet = Sheet::parse(&text);
        assert_eq!(sheet.header, COLUMNS);
        assert_eq!(sheet.rows[0][5], "Bread, milk and \"good\" cheese");
        assert_eq!(sheet.rows[1][5], "two\nlines");
        let at = sheet.columns().unwrap();

        let written = sheet.render(&at);
        assert!(written.starts_with(BOM));
        // Sorted by (date, created_at), each field quoted only where it must be.
        assert_eq!(
            written,
            format!(
                "{BOM}{HEADER}\r\n\
                 0000000001,2026-10-01,3,USD,Fun,\"two\nlines\",2026-10-01T20:15:00\r\n\
                 a1b2c3d4e5,2026-10-02,12.5,EUR,Food,\"Bread, milk and \"\"good\"\" cheese\",2026-10-02T09:00:00\r\n"
            )
        );
        // Reading what was written gives the same sheet back.
        let again = Sheet::parse(&written);
        assert_eq!(again.rows, {
            let mut rows = sheet.rows.clone();
            rows.swap(0, 1);
            rows
        });
        assert_eq!(again.render(&at), written);
    }

    #[test]
    fn a_file_with_unix_line_endings_keeps_them_and_rows_sort_by_date_then_time() {
        let text = format!(
            "{HEADER}\n\
             c,2026-10-05,1,EUR,Food,,2026-10-05T10:00:00\n\
             b,2026-10-05,2,EUR,Food,,2026-10-05T08:00:00\n\
             a,2026-09-30,3,EUR,Food,,2026-10-06T08:00:00\n\
             \n"
        );
        let sheet = Sheet::parse(&text);
        assert_eq!(sheet.rows.len(), 3, "a blank line is not a row");
        let written = sheet.render(&sheet.columns().unwrap());
        let ids: Vec<&str> = written.lines().skip(1).map(|l| &l[..1]).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        assert!(!written.contains('\r'));
        assert!(written.starts_with(BOM), "the BOM is always written");
    }

    #[test]
    fn rows_whose_amount_is_not_a_number_are_skipped_but_kept_in_the_file() {
        let dir = folder_with(
            "bad-rows",
            SETTINGS,
            &[
                "aaaaaaaaaa,2026-10-01,10,EUR,Food,,2026-10-01T10:00:00",
                "bbbbbbbbbb,2026-10-01,ten,EUR,Food,,2026-10-01T11:00:00",
                "cccccccccc,2026-10-01,,EUR,Food,,2026-10-01T12:00:00",
                "dddddddddd,2026-10-02,NaN,EUR,Food,,2026-10-02T12:00:00",
            ],
        );
        let s = summary_on(&dir, "2026-10-02").unwrap();
        assert_eq!((s.entry_count, s.skipped_rows, s.spent_month), (1, 3, 10.0));

        add_expense_at(&dir, &new(1.0, "EUR", "Food", "", None), &now("2026-10-02", "13:00:00")).unwrap();
        let text = file(&dir);
        for id in ["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd"] {
            assert!(text.contains(id), "{id} must survive the write");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── settings.json ─────────────────────────────────────────────────────────

    #[test]
    fn missing_or_unusable_settings_fall_back_one_by_one() {
        let d = FinanceSettings::default();
        assert_eq!(d.base_currency, "USD");
        assert_eq!(d.monthly_budget, 0.0);
        assert_eq!(d.rates, BTreeMap::from([("USD".to_string(), 1.0)]));
        assert_eq!(d.categories, DEFAULT_CATEGORIES);
        assert_eq!(d.savings_currency, "USD");

        let s = parse_settings(
            format!(
                "{BOM}{}",
                r#"{ "base_currency": "TRY", "monthly_budget": "abc", "rates": { "EUR": 35, "usd": 30, "GBP": -1, "XAU": "2500" }, "categories": [] }"#
            )
            .as_bytes(),
        );
        assert_eq!(s.base_currency, "TRY");
        assert_eq!(s.monthly_budget, 0.0);
        assert_eq!(s.savings_currency, "TRY");
        // The base is always 1; a lowercase code or a rate below 0 is not a rate.
        assert_eq!(
            s.rates,
            BTreeMap::from([("EUR".to_string(), 35.0), ("TRY".to_string(), 1.0), ("XAU".to_string(), 2500.0)])
        );
        assert_eq!(s.currencies(), ["TRY", "EUR", "XAU"]);
        assert_eq!(s.categories, DEFAULT_CATEGORIES);
        assert_eq!(parse_settings(b"not json"), d);
    }

    // ── Summary ───────────────────────────────────────────────────────────────

    #[test]
    fn the_summary_converts_to_the_base_and_counts_only_this_month() {
        let dir = folder_with(
            "summary",
            SETTINGS,
            &[
                "0000000001,2026-09-30,999,EUR,Rent,last month,2026-09-30T10:00:00",
                "0000000002,2026-10-01,100,EUR,Rent,,2026-10-01T10:00:00",
                "0000000003,2026-10-03,40,USD,Food,lunch,2026-10-03T12:00:00",
                "0000000004,2026-10-03,10,GBP,Fun,cinema,2026-10-03T21:00:00",
                "0000000005,2026-10-02,7,JPY,Food,no rate,2026-10-02T08:00:00",
                "0000000006,2026-11-01,5,EUR,Food,next month,2026-10-03T22:00:00",
            ],
        );
        let s = summary_on(&dir, "2026-10-03").unwrap();
        assert_eq!(s.base_currency, "EUR");
        assert_eq!(s.month, "2026-10");
        // 100 EUR + 40 USD × 0.5 + 10 GBP × 2; the yen has no rate and is left out.
        assert_eq!(s.spent_month, 140.0);
        assert_eq!(s.spent_today, 40.0);
        assert_eq!(s.unconverted, ["JPY"]);
        assert_eq!((s.budget, s.remaining), (Some(1000.0), Some(860.0)));
        assert!((s.percent_used.unwrap() - 14.0).abs() < 1e-9);
        assert_eq!(
            s.top_categories,
            [
                CategoryTotal { category: "Rent".into(), amount: 100.0 },
                CategoryTotal { category: "Food".into(), amount: 20.0 },
                CategoryTotal { category: "Fun".into(), amount: 20.0 },
            ]
        );
        assert_eq!((s.entry_count, s.month_count), (6, 4));
        let recent: Vec<&str> = s.recent.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(recent, ["0000000006", "0000000004", "0000000003", "0000000005", "0000000002"]);
        assert_eq!(s.recent[1].note, "cinema");
        assert_eq!(s.currencies, ["EUR", "GBP", "USD"]);
        assert_eq!(s.categories, ["Food", "Rent", "Fun"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_budget_there_is_no_remaining_and_no_percentage() {
        let dir = folder_with(
            "no-budget",
            r#"{ "base_currency": "EUR", "rates": { "EUR": 1 } }"#,
            &["0000000001,2026-10-01,25.5,EUR,Food,,2026-10-01T10:00:00"],
        );
        let s = summary_on(&dir, "2026-10-20").unwrap();
        assert_eq!(s.spent_month, 25.5);
        assert_eq!((s.budget, s.remaining, s.percent_used), (None, None, None));
        assert_eq!(s.spent_today, 0.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_is_not_there_reads_as_empty_and_is_not_created() {
        let dir = scratch("absent").join("finance");
        let s = summary_on(&dir, "2026-10-20").unwrap();
        assert_eq!((s.entry_count, s.spent_month, s.base_currency.as_str()), (0, 0.0, "USD"));
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn a_csv_without_the_expected_columns_is_refused_and_left_alone() {
        let dir = scratch("columns");
        std::fs::write(dir.join(EXPENSES_FILE), "when,how much\n2026-10-01,3\n").unwrap();
        assert_eq!(summary_on(&dir, "2026-10-01"), Err(FinanceError::Columns));
        let err = add_expense_at(&dir, &new(1.0, "USD", "Food", "", None), &now("2026-10-01", "10:00:00"));
        assert_eq!(err.unwrap_err(), FinanceError::Columns);
        assert_eq!(file(&dir), "when,how much\n2026-10-01,3\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Adding ────────────────────────────────────────────────────────────────

    #[test]
    fn adding_writes_a_valid_row_in_place_and_keeps_the_file_sorted() {
        let dir = folder_with(
            "add",
            SETTINGS,
            &[
                "0000000001,2026-10-01,10,EUR,Food,,2026-10-01T10:00:00",
                "0000000002,2026-10-05,20,EUR,Food,,2026-10-05T10:00:00",
            ],
        );
        let added = add_expense_at(
            &dir,
            &new(12.345 - 0.005, "USD", " Fun ", "Board games, \"deluxe\"\nedition", Some("2026-10-03")),
            &now("2026-10-06", "18:30:05"),
        )
        .unwrap();
        assert_eq!(added.id.len(), 10);
        assert!(added.id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(added.category, "Fun");
        assert_eq!(added.note, "Board games, \"deluxe\" edition");
        assert_eq!(added.created_at, "2026-10-06T18:30:05");

        let text = file(&dir);
        let lines: Vec<&str> = text.split("\r\n").collect();
        assert_eq!(lines[0], format!("{BOM}{HEADER}"));
        assert_eq!(lines[1], "0000000001,2026-10-01,10,EUR,Food,,2026-10-01T10:00:00");
        assert_eq!(
            lines[2],
            format!("{},2026-10-03,12.34,USD,Fun,\"Board games, \"\"deluxe\"\" edition\",2026-10-06T18:30:05", added.id)
        );
        assert_eq!(lines[3], "0000000002,2026-10-05,20,EUR,Food,,2026-10-05T10:00:00");
        assert_eq!(lines[4], "");
        assert!(!dir.join("expenses.csv.tmp").exists());

        let s = summary_on(&dir, "2026-10-06").unwrap();
        assert_eq!(s.spent_month, 36.17);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_is_not_valid_is_refused_and_nothing_is_written() {
        let dir = folder_with("invalid", SETTINGS, &[]);
        let before = file(&dir);
        let at = now("2026-10-06", "10:00:00");
        let cases: [(NewExpense, FinanceError); 10] = [
            (new(0.0, "EUR", "Food", "", None), FinanceError::Amount),
            (new(-3.0, "EUR", "Food", "", None), FinanceError::Amount),
            (new(1.234, "EUR", "Food", "", None), FinanceError::Amount),
            (new(f64::NAN, "EUR", "Food", "", None), FinanceError::Amount),
            (new(1.0, "EUR", "Food", "", Some("2026-02-30")), FinanceError::Date),
            (new(1.0, "EUR", "Food", "", Some("06/10/2026")), FinanceError::Date),
            (new(1.0, "JPY", "Food", "", None), FinanceError::Currency("JPY".into())),
            (new(1.0, "EUR", "  ", "", None), FinanceError::Category),
            (new(1.0, "EUR", &"x".repeat(61), "", None), FinanceError::Category),
            (new(1.0, "EUR", "Food", &"é".repeat(201), None), FinanceError::Note),
        ];
        for (expense, error) in cases {
            assert_eq!(add_expense_at(&dir, &expense, &at).unwrap_err(), error, "{expense:?}");
        }
        assert_eq!(file(&dir), before);
        // The limits themselves pass, and a leap day is a day.
        add_expense_at(&dir, &new(0.01, "GBP", &"x".repeat(60), &"é".repeat(200), Some("2028-02-29")), &at).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_the_dashboard_added_meanwhile_is_kept() {
        let dir = folder_with("concurrent", SETTINGS, &["0000000001,2026-10-01,10,EUR,Food,,2026-10-01T10:00:00"]);
        // The card was drawn from one row…
        assert_eq!(summary_on(&dir, "2026-10-02").unwrap().entry_count, 1);
        // …then the dashboard wrote a second one…
        let mut text = file(&dir);
        text.push_str("0000000002,2026-10-02,5,EUR,Fun,from the dashboard,2026-10-02T09:00:00\r\n");
        std::fs::write(dir.join(EXPENSES_FILE), text).unwrap();
        // …and Ciel adds a third.
        add_expense_at(&dir, &new(1.0, "EUR", "Food", "", None), &now("2026-10-02", "09:30:00")).unwrap();
        let s = summary_on(&dir, "2026-10-02").unwrap();
        assert_eq!(s.entry_count, 3);
        assert!(file(&dir).contains("from the dashboard"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_json_is_never_written() {
        let dir = folder_with("settings-untouched", SETTINGS, &[]);
        let path = dir.join(SETTINGS_FILE);
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        for day in ["2026-10-01", "2026-10-02"] {
            add_expense_at(&dir, &new(2.0, "EUR", "Food", "", Some(day)), &now(day, "10:00:00")).unwrap();
            summary_on(&dir, day).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SETTINGS);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), modified);

        // A folder without one gets none either: only a new folder does.
        std::fs::remove_file(&path).unwrap();
        add_expense_at(&dir, &new(2.0, "USD", "Food", "", None), &now("2026-10-03", "10:00:00")).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_folder_gets_generic_defaults_and_its_first_expense() {
        let dir = scratch("new").join("finance");
        add_expense_at(&dir, &new(9.99, "USD", "Groceries", "", None), &now("2026-10-06", "10:00:00")).unwrap();
        let settings: Value = serde_json::from_slice(&std::fs::read(dir.join(SETTINGS_FILE)).unwrap()).unwrap();
        assert_eq!(settings["base_currency"], "USD");
        assert_eq!(settings["monthly_budget"], 0);
        assert_eq!(settings["rates"], json!({ "USD": 1 }));
        assert_eq!(settings["categories"], json!(DEFAULT_CATEGORIES));
        // settings.json has no BOM, which a reader not expecting one would choke on.
        assert_eq!(std::fs::read(dir.join(SETTINGS_FILE)).unwrap()[0], b'{');
        let text = file(&dir);
        assert!(text.starts_with(&format!("{BOM}{HEADER}\r\n")));
        assert!(text.contains(",2026-10-06,9.99,USD,Groceries,,2026-10-06T10:00:00\r\n"));
        let s = summary_on(&dir, "2026-10-06").unwrap();
        assert_eq!((s.spent_month, s.budget), (9.99, None));
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn ids_are_ten_lowercase_hex_characters_and_never_taken() {
        let taken: HashSet<String> = HashSet::new();
        let id = new_id(&taken);
        assert_eq!(id.len(), 10);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_ne!(new_id(&taken), id);
    }

    #[test]
    fn dates_are_real_days() {
        for ok in ["2026-01-31", "2024-02-29", "2000-02-29", "2026-12-01"] {
            assert!(is_date(ok), "{ok}");
        }
        for bad in ["2026-13-01", "2026-00-10", "2026-04-31", "1900-02-29", "2026-1-01", "20261001", "2026-10-0a", ""] {
            assert!(!is_date(bad), "{bad}");
        }
    }

    #[test]
    fn the_stamp_changes_with_the_files_and_the_day() {
        let dir = folder_with("stamp", SETTINGS, &[]);
        let first = stamp(&dir, "2026-10-01");
        assert_eq!(stamp(&dir, "2026-10-01"), first);
        assert_ne!(stamp(&dir, "2026-10-02"), first);
        add_expense_at(&dir, &new(1.0, "EUR", "Food", "", None), &now("2026-10-01", "10:00:00")).unwrap();
        assert_ne!(stamp(&dir, "2026-10-01"), first);
        assert!(poll(&dir, None).unwrap().summary.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_folder_setting_falls_back_to_the_default_unless_it_is_a_full_path() {
        assert_eq!(folder_from(""), default_folder());
        assert_eq!(folder_from("relative/data"), default_folder());
        let full = std::env::temp_dir().join("some-dashboard").join("data");
        assert_eq!(folder_from(&format!("  {}  ", full.display())), full);
        assert!(default_folder().ends_with("finance"));
    }

    /// Opens `path` the way a process that shares nothing does (OneDrive
    /// syncing it, the dashboard writing it) until the handle is dropped.
    #[cfg(windows)]
    fn locked(path: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).share_mode(0).open(path).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn a_file_held_by_another_program_is_written_once_it_is_free() {
        let dir = folder_with("locked", SETTINGS, &["0000000001,2026-10-01,10,EUR,Food,,2026-10-01T10:00:00"]);
        let lock = locked(&dir.join(EXPENSES_FILE));
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            drop(lock);
        });
        add_expense_at(&dir, &new(1.0, "EUR", "Food", "", None), &now("2026-10-02", "10:00:00")).unwrap();
        release.join().unwrap();
        assert_eq!(summary_on(&dir, "2026-10-02").unwrap().entry_count, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
