// The Finance pill on the page: what src-tauri/src/finance.rs reports about
// the finance folder (expenses.csv + settings.json), and the pure rules the
// pill, its card and the Add expense form follow. Nothing here touches the
// DOM or the files.

import { dayMonth, language, t, tn } from "../i18n/i18n";
import { HOST_OS, type HostOs } from "./pills";

export const FINANCE_ID = "integration_finance";
export const FINANCE_GREEN = "#10B981";
/** The same limits finance.rs checks. */
export const MAX_CATEGORY = 60;
export const MAX_NOTE = 200;
/** How often the island looks at the files while the pill is on screen. */
export const POLL_MS = 4000;

export interface FinanceEntry {
  id: string;
  /** YYYY-MM-DD */
  date: string;
  amount: number;
  currency: string;
  category: string;
  note: string;
  createdAt: string;
}

/** finance.rs `Summary`: this month, every total in the base currency. */
export interface FinanceSummary {
  baseCurrency: string;
  /** YYYY-MM */
  month: string;
  /** YYYY-MM-DD */
  today: string;
  spentMonth: number;
  spentToday: number;
  /** null: no budget set. */
  budget: number | null;
  remaining: number | null;
  percentUsed: number | null;
  topCategories: { category: string; amount: number }[];
  /** Newest first, each in its own currency. */
  recent: FinanceEntry[];
  entryCount: number;
  monthCount: number;
  skippedRows: number;
  /** This month's currencies that have no rate, so are not in the totals. */
  unconverted: string[];
  currencies: string[];
  categories: string[];
}

/** What `finance_poll` answers when something changed. */
export interface FinancePoll {
  stamp: string;
  folder: string;
  summary: FinanceSummary | null;
  error: string | null;
}

export interface NewExpense {
  amount: number;
  currency: string;
  category: string;
  note: string;
  date: string;
}

/** Settings → Integrations → Finance: what is in a folder. */
export interface FinanceFolderInfo {
  path: string;
  defaultPath: string;
  isDefault: boolean;
  exists: boolean;
  hasExpenses: boolean;
  hasSettings: boolean;
}

const isNum = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** The summary an integration's data holds, if it really is one. */
export function readSummary(data: unknown): FinanceSummary | null {
  const d = data as Partial<FinanceSummary> | null;
  if (!d || typeof d !== "object" || typeof d.baseCurrency !== "string" || !isNum(d.spentMonth)) return null;
  return {
    baseCurrency: d.baseCurrency,
    month: String(d.month ?? ""),
    today: String(d.today ?? ""),
    spentMonth: d.spentMonth,
    spentToday: isNum(d.spentToday) ? d.spentToday : 0,
    budget: isNum(d.budget) && d.budget > 0 ? d.budget : null,
    remaining: isNum(d.remaining) ? d.remaining : null,
    percentUsed: isNum(d.percentUsed) ? d.percentUsed : null,
    topCategories: (Array.isArray(d.topCategories) ? d.topCategories : [])
      .filter((c) => c && typeof c.category === "string" && isNum(c.amount)),
    recent: (Array.isArray(d.recent) ? d.recent : []).filter((e) => e && isNum(e.amount)),
    entryCount: isNum(d.entryCount) ? d.entryCount : 0,
    monthCount: isNum(d.monthCount) ? d.monthCount : 0,
    skippedRows: isNum(d.skippedRows) ? d.skippedRows : 0,
    unconverted: strings(d.unconverted),
    currencies: strings(d.currencies),
    categories: strings(d.categories),
  };
}

// ── Numbers ───────────────────────────────────────────────────────────────────

function numberFormat(digits: number, lang: string): Intl.NumberFormat {
  try {
    // Latin digits everywhere, as the rest of the island writes them.
    return new Intl.NumberFormat(`${lang}-u-nu-latn`, { minimumFractionDigits: digits, maximumFractionDigits: digits });
  } catch {
    return new Intl.NumberFormat("en", { minimumFractionDigits: digits, maximumFractionDigits: digits });
  }
}

/** "1,234.50": two decimals, the language's separators. */
export function formatAmount(n: number, lang: string = language()): string {
  return numberFormat(2, lang).format(n);
}

/** "2,000" for a whole amount, "2,000.50" otherwise: for the budget. */
export function formatShort(n: number, lang: string = language()): string {
  return numberFormat(Number.isInteger(Math.round(n * 100) / 100) ? 0 : 2, lang).format(n);
}

/** "12.50 EUR" */
export function formatMoney(n: number, currency: string, lang: string = language()): string {
  return `${formatAmount(n, lang)} ${currency}`;
}

// ── The pill ──────────────────────────────────────────────────────────────────

/** What the pill says next to its mini Ciel: "62% of budget", else the month's total. */
export function pillText(s: FinanceSummary | null, lang: string = language()): string | null {
  if (!s) return null;
  if (s.budget != null && s.percentUsed != null) {
    return t("{percent}% of budget", { percent: Math.round(s.percentUsed) });
  }
  return t("{amount} this month", { amount: `${formatShort(s.spentMonth, lang)} ${s.baseCurrency}` });
}

// ── The card ──────────────────────────────────────────────────────────────────

export type BudgetLevel = "ok" | "near" | "over";

/** Green until 80 % of the budget, amber up to it, red past it. */
export function budgetLevel(percent: number | null): BudgetLevel {
  if (percent == null || percent < 80) return "ok";
  return percent <= 100 ? "near" : "over";
}

export const LEVEL_COLORS: Record<BudgetLevel, string> = {
  ok: FINANCE_GREEN,
  near: "#F5A524",
  over: "#F4505E",
};

export interface FinanceRow {
  label: string;
  amount: string;
  /** Share of the month's spending, 0…1. */
  share: number;
}

export interface FinanceEntryRow {
  id: string;
  /** "Oct 3" */
  date: string;
  category: string;
  amount: string;
  note: string;
}

/** Everything the card writes, ready to show. */
export interface FinanceView {
  /** "1,234.50" */
  spent: string;
  /** "/ 2,000 USD", or just "USD" without a budget. */
  of: string;
  /** "62%", null without a budget. */
  percent: string | null;
  /** How full the bar is, 0…1; null without a budget. */
  fill: number | null;
  level: BudgetLevel;
  color: string;
  /** "765.50 left", "12.00 over" or "No budget set". */
  remaining: string;
  /** "12.00 today" */
  today: string;
  top: FinanceRow[];
  recent: FinanceEntryRow[];
  /** "23 entries this month" */
  count: string;
  /** Something the totals leave out, said once; null when nothing is. */
  warning: string | null;
}

/** "2026-10-03" → "Oct 3"; anything else as it is. */
export function shortDate(date: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  return m ? dayMonth(Number(m[2]) - 1, Number(m[3])) : date;
}

export function financeView(s: FinanceSummary, lang: string = language()): FinanceView {
  const level = budgetLevel(s.percentUsed);
  const remaining =
    s.remaining == null
      ? t("No budget set")
      : s.remaining >= 0
        ? t("{amount} left", { amount: formatAmount(s.remaining, lang) })
        : t("{amount} over", { amount: formatAmount(-s.remaining, lang) });
  const warnings: string[] = [];
  if (s.unconverted.length > 0) {
    warnings.push(t("No rate for {currencies}: not counted", { currencies: s.unconverted.join(", ") }));
  }
  if (s.skippedRows > 0) warnings.push(tn("{count} row skipped", "{count} rows skipped", s.skippedRows));
  return {
    spent: formatAmount(s.spentMonth, lang),
    of: s.budget != null ? `/ ${formatShort(s.budget, lang)} ${s.baseCurrency}` : s.baseCurrency,
    percent: s.percentUsed != null ? `${Math.round(s.percentUsed)}%` : null,
    fill: s.percentUsed != null ? Math.min(1, Math.max(0, s.percentUsed / 100)) : null,
    level,
    color: LEVEL_COLORS[level],
    remaining,
    today: t("{amount} today", { amount: formatAmount(s.spentToday, lang) }),
    top: s.topCategories.slice(0, 3).map((c) => ({
      label: c.category || "—",
      amount: formatAmount(c.amount, lang),
      share: s.spentMonth > 0 ? Math.min(1, Math.max(0, c.amount / s.spentMonth)) : 0,
    })),
    recent: s.recent.slice(0, 5).map((e) => ({
      id: e.id,
      date: shortDate(e.date),
      category: e.category || "—",
      amount: formatMoney(e.amount, e.currency, lang),
      note: e.note,
    })),
    count: tn("{count} entry this month", "{count} entries this month", s.monthCount),
    warning: warnings.length > 0 ? warnings.join(" · ") : null,
  };
}

// ── The Add expense form ──────────────────────────────────────────────────────

/** Today on this computer, YYYY-MM-DD. */
export function localDate(d: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** A real calendar day written YYYY-MM-DD (finance.rs `is_date`). */
export function isDate(s: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s);
  if (!m) return false;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (y < 1 || mo < 1 || mo > 12 || d < 1) return false;
  return d <= new Date(Date.UTC(y, mo, 0)).getUTCDate();
}

/**
 * What was typed in the amount field as a number: "12", "12.5", "12,50" (a
 * decimal comma) or "1,234.50". Null for anything else, more than two
 * decimals, or nothing above 0 \u2014 and for "1,234", which could be either a
 * thousand and more or one and a bit: better asked again than guessed.
 */
export function parseAmount(text: string): number | null {
  let s = text.trim().replace(/[\s\u00a0\u202f]/g, "");
  if (/^\d{1,3}(,\d{3})+\.\d{1,2}$/.test(s)) s = s.replace(/,/g, "");
  else if (/^\d+,\d{1,2}$/.test(s)) s = s.replace(",", ".");
  if (!/^\d+(\.\d{1,2})?$/.test(s)) return null;
  const n = Number(s);
  return Number.isFinite(n) && n > 0 && n < 1e12 ? n : null;
}

export interface ExpenseForm {
  amount: string;
  currency: string;
  category: string;
  note: string;
  date: string;
}

/** One line: a line break would be a second line in the dashboard. */
const oneLine = (s: string) => s.split(/[\r\n]+/).map((p) => p.trim()).filter(Boolean).join(" ");

/**
 * The expense the form describes, or the reason it can't be added — the same
 * rules and words as finance.rs, which checks again before writing.
 */
export function checkExpense(
  form: ExpenseForm,
  currencies: readonly string[],
): { ok: true; expense: NewExpense } | { ok: false; error: string } {
  const amount = parseAmount(form.amount);
  if (amount == null) return { ok: false, error: t("Enter an amount above 0 with at most 2 decimals.") };
  const date = form.date.trim();
  if (!isDate(date)) return { ok: false, error: t("Enter the date as YYYY-MM-DD.") };
  const currency = form.currency.trim().toUpperCase();
  if (!/^[A-Z]{3}$/.test(currency) || !currencies.includes(currency)) {
    return { ok: false, error: t("No exchange rate for {currency} in settings.json.", { currency: currency || "—" }) };
  }
  const category = oneLine(form.category);
  if (!category || [...category].length > MAX_CATEGORY) {
    return { ok: false, error: t("Pick a category (60 characters at most).") };
  }
  const note = oneLine(form.note);
  if ([...note].length > MAX_NOTE) return { ok: false, error: t("Keep the note to 200 characters.") };
  return { ok: true, expense: { amount, currency, category, note, date } };
}

// ── Settings ──────────────────────────────────────────────────────────────────

/**
 * A full path, as finance.rs takes one: C:\… or \\server\share on Windows,
 * /… on Linux. Anything else would quietly mean the default folder.
 */
export function isFullPath(path: string, os: HostOs = HOST_OS): boolean {
  const p = path.trim();
  if (os === "linux") return p.startsWith("/");
  return /^[A-Za-z]:[\\/]/.test(p) || /^\\\\[^\\]+\\[^\\]+/.test(p);
}
