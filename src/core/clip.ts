// What Ciel sees in a copied text, and what it can turn it into (Tools →
// Clipboard). Pure: no DOM, no Tauri.
//
// A link can be cleaned of its trackers, a colour shows its swatch and its
// other spellings, a sum shows its result, JSON can be laid out, and any text
// can change case or lose its extra spaces.

export type ClipKind = "link" | "color" | "email" | "phone" | "math" | "json" | "code" | "path" | "text";

const URL_RE = /^(https?:\/\/|www\.)[^\s]+$/i;
const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[a-z]{2,}$/i;
const PHONE_RE = /^\+?[\d\s().-]{7,20}$/;
const PATH_RE = /^(?:[a-zA-Z]:\\|\\\\|\/|~\/)[^\n]*$/;

export function classify(raw: string): ClipKind {
  const text = raw.trim();
  if (!text) return "text";
  if (!text.includes("\n")) {
    if (URL_RE.test(text)) return "link";
    if (parseColor(text)) return "color";
    if (EMAIL_RE.test(text)) return "email";
    if (evalMath(text) != null) return "math";
    // A date (2026-11-07, 07.11.2026) is not a number to call.
    const date = /^\d{4}-\d{1,2}-\d{1,2}$|^\d{1,2}[./-]\d{1,2}[./-]\d{2,4}$/.test(text);
    if (!date && PHONE_RE.test(text) && (text.match(/\d/g)?.length ?? 0) >= 7) return "phone";
    if (PATH_RE.test(text)) return "path";
  }
  if (/^[[{]/.test(text) && prettyJson(text) != null) return "json";
  if (looksLikeCode(text)) return "code";
  return "text";
}

function looksLikeCode(text: string): boolean {
  const lines = text.split("\n");
  if (lines.length < 2) return /[;{}]\s*$/.test(text) && /[=(]/.test(text);
  const codeish = lines.filter((l) => /^\s{2,}\S|[;{}]\s*$|^\s*(const|let|var|fn|def|function|import|return|if|for|class|pub|use)\b/.test(l));
  return codeish.length >= Math.ceil(lines.length / 2);
}

// ── Links ─────────────────────────────────────────────────────────────────────

/** Query parameters that only follow the reader around. */
const TRACKERS = [
  /^utm_/i, /^fbclid$/i, /^gclid$/i, /^dclid$/i, /^gbraid$/i, /^wbraid$/i, /^msclkid$/i, /^mc_(cid|eid)$/i,
  /^igshid$/i, /^igsh$/i, /^si$/i, /^ref_src$/i, /^ref_url$/i, /^_hsenc$/i, /^_hsmi$/i, /^mkt_tok$/i,
  /^yclid$/i, /^twclid$/i, /^ttclid$/i, /^li_fat_id$/i, /^spm$/i, /^vero_id$/i, /^oly_(anon|enc)_id$/i,
];

/** The link without its trackers; null when there were none (or it isn't a link). */
export function cleanLink(raw: string): string | null {
  let url: URL;
  try {
    url = new URL(/^www\./i.test(raw.trim()) ? `https://${raw.trim()}` : raw.trim());
  } catch {
    return null;
  }
  const before = url.searchParams.toString();
  for (const key of [...url.searchParams.keys()]) {
    if (TRACKERS.some((re) => re.test(key))) url.searchParams.delete(key);
  }
  if (url.searchParams.toString() === before) return null;
  let out = url.toString();
  if (!url.searchParams.toString()) out = out.replace(/\?(?=#|$)/, "");
  return out;
}

/** "github.com" for a link. */
export function linkHost(raw: string): string {
  try {
    return new URL(/^www\./i.test(raw.trim()) ? `https://${raw.trim()}` : raw.trim()).hostname.replace(/^www\./, "");
  } catch {
    return "";
  }
}

// ── Colours ───────────────────────────────────────────────────────────────────

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

export function parseColor(raw: string): Rgba | null {
  const s = raw.trim().toLowerCase();
  let m = /^#?([0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(s);
  // Without a #, only six or eight hex digits mixing figures and letters:
  // "123456" is a number and "facade" a word, "c0ffee" a colour.
  const bare = m && !s.startsWith("#");
  if (m && (!bare || (m[1].length >= 6 && /\d/.test(m[1]) && /[a-f]/.test(m[1])))) {
    const hex = m[1].length <= 4 ? [...m[1]].map((c) => c + c).join("") : m[1];
    const n = (i: number) => parseInt(hex.slice(i, i + 2), 16);
    return { r: n(0), g: n(2), b: n(4), a: hex.length === 8 ? Math.round((n(6) / 255) * 100) / 100 : 1 };
  }
  m = /^rgba?\(\s*(\d{1,3})[\s,]+(\d{1,3})[\s,]+(\d{1,3})(?:[\s,/]+([\d.]+%?))?\s*\)$/.exec(s);
  if (m) {
    const [r, g, b] = [m[1], m[2], m[3]].map(Number);
    if ([r, g, b].some((v) => v > 255)) return null;
    return { r, g, b, a: alpha(m[4]) };
  }
  m = /^hsla?\(\s*([\d.]+)(?:deg)?[\s,]+([\d.]+)%[\s,]+([\d.]+)%(?:[\s,/]+([\d.]+%?))?\s*\)$/.exec(s);
  if (m) {
    const [h, sat, l] = [Number(m[1]) % 360, Number(m[2]) / 100, Number(m[3]) / 100];
    if (sat > 1 || l > 1) return null;
    const { r, g, b } = hslToRgb(h, sat, l);
    return { r, g, b, a: alpha(m[4]) };
  }
  return null;
}

function alpha(raw: string | undefined): number {
  if (raw == null) return 1;
  const v = raw.endsWith("%") ? Number(raw.slice(0, -1)) / 100 : Number(raw);
  return Number.isFinite(v) ? Math.max(0, Math.min(1, v)) : 1;
}

function hslToRgb(h: number, s: number, l: number) {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return { r: Math.round(f(0) * 255), g: Math.round(f(8) * 255), b: Math.round(f(4) * 255) };
}

/** The colour in its three usual spellings. */
export function colorForms(c: Rgba): { hex: string; rgb: string; hsl: string } {
  const hx = (v: number) => v.toString(16).padStart(2, "0");
  const hex = `#${hx(c.r)}${hx(c.g)}${hx(c.b)}${c.a < 1 ? hx(Math.round(c.a * 255)) : ""}`.toUpperCase();
  const rgb = c.a < 1 ? `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a})` : `rgb(${c.r}, ${c.g}, ${c.b})`;
  const [r, g, b] = [c.r / 255, c.g / 255, c.b / 255];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    h *= 60;
  }
  const hsl = `hsl(${Math.round(h)}, ${Math.round(s * 100)}%, ${Math.round(l * 100)}%)`;
  return { hex, rgb, hsl };
}

// ── Sums ──────────────────────────────────────────────────────────────────────

/**
 * The result of a sum like "12*7", "1 200 / 3 + 5", "2^10", "(4+6) × 2,5";
 * null for anything that is not one (a lone number, a date, words). Parsed
 * by hand: nothing is ever evaluated as code.
 */
export function evalMath(raw: string): number | null {
  const s = raw.trim().replace(/[×x]/g, "*").replace(/÷/g, "/").replace(/−/g, "-");
  if (!/^[\d\s.,+\-*/^%()]+$/.test(s)) return null;
  // A sum has an operator between two numbers: "2026-11-07" or "-5" are not ones.
  if (!/\d\s*[+*/^%]\s*[\d(]|\d\s+-\s+\d|\)\s*[+\-*/^]|\d-\d+\s*[+*/^]/.test(s)) return null;
  if (/^\d{4}-\d{2}-\d{2}$/.test(s)) return null;
  const tokens = tokenize(s);
  if (!tokens) return null;
  let i = 0;
  const peek = () => tokens[i];
  const next = () => tokens[i++];
  function primary(): number | null {
    const t = next();
    if (t === "(") {
      const v = sum();
      if (next() !== ")") return null;
      return v;
    }
    if (t === "-") {
      const v = power();
      return v == null ? null : -v;
    }
    if (t === "+") return power();
    return typeof t === "number" ? t : null;
  }
  function power(): number | null {
    const base = primary();
    if (base == null) return null;
    if (peek() === "^") {
      next();
      const exp = power();
      return exp == null ? null : base ** exp;
    }
    return base;
  }
  function product(): number | null {
    let v = power();
    while (v != null && (peek() === "*" || peek() === "/" || peek() === "%")) {
      const op = next();
      const r = power();
      if (r == null) return null;
      v = op === "*" ? v * r : op === "/" ? v / r : v % r;
    }
    return v;
  }
  function sum(): number | null {
    let v = product();
    while (v != null && (peek() === "+" || peek() === "-")) {
      const op = next();
      const r = product();
      if (r == null) return null;
      v = op === "+" ? v + r : v - r;
    }
    return v;
  }
  const v = sum();
  if (v == null || i !== tokens.length || !Number.isFinite(v)) return null;
  return Math.round(v * 1e10) / 1e10;
}

function tokenize(s: string): (number | string)[] | null {
  const out: (number | string)[] = [];
  let i = 0;
  while (i < s.length) {
    const c = s[i];
    if (c === " ") {
      i++;
      continue;
    }
    if ("+-*/^%()".includes(c)) {
      out.push(c);
      i++;
      continue;
    }
    // A number: "1 200,50" or "1,200.50" or "2,5".
    const m = /^\d[\d\s]*(?:[.,]\d+)*/.exec(s.slice(i));
    if (!m) return null;
    const raw = m[0].trimEnd();
    i += raw.length;
    const n = number(raw);
    if (n == null) return null;
    out.push(n);
  }
  return out;
}

function number(raw: string): number | null {
  let s = raw.replace(/\s/g, "");
  const dots = (s.match(/\./g) ?? []).length;
  const commas = (s.match(/,/g) ?? []).length;
  if (dots && commas) {
    // The last one is the decimal point.
    s = s.lastIndexOf(",") > s.lastIndexOf(".") ? s.replace(/\./g, "").replace(",", ".") : s.replace(/,/g, "");
  } else if (commas === 1 && !/,\d{3}$/.test(s)) {
    s = s.replace(",", ".");
  } else if (commas) {
    s = s.replace(/,/g, "");
  } else if (dots > 1) {
    s = s.replace(/\./g, "");
  }
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

/** 84, 0.333…, 1 234 567: as the language writes numbers, up to 6 decimals. */
export function formatNumber(n: number, lang?: string): string {
  return new Intl.NumberFormat(lang, { maximumFractionDigits: 6 }).format(n);
}

// ── Turning text into something else ──────────────────────────────────────────

export type Transform = "upper" | "lower" | "title" | "trim" | "json";

export function prettyJson(text: string): string | null {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return null;
  }
}

export function transform(text: string, how: Transform, lang?: string): string {
  switch (how) {
    case "upper":
      return text.toLocaleUpperCase(lang);
    case "lower":
      return text.toLocaleLowerCase(lang);
    case "title":
      return text.toLocaleLowerCase(lang).replace(/(^|[\s\-"'(«])(\p{L})/gu, (_, sep: string, c: string) => sep + c.toLocaleUpperCase(lang));
    case "trim":
      return text
        .split("\n")
        .map((l) => l.replace(/[ \t]+/g, " ").trim())
        .join("\n")
        .replace(/\n{3,}/g, "\n\n")
        .trim();
    case "json":
      return prettyJson(text) ?? text;
  }
}

/** A text's words and characters, as people count them. */
export function counts(text: string): { words: number; chars: number; lines: number } {
  const words = text.trim() ? text.trim().split(/\s+/u).length : 0;
  return { words, chars: [...text].length, lines: text ? text.split("\n").length : 0 };
}

/** The first line, cut: what the list shows. */
export function preview(text: string, max = 120): string {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
}

/** Items matching a search: every word somewhere in the text or the app. */
export function matches(item: { text: string; app: string }, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = `${item.text} ${item.app}`.toLowerCase();
  return q.split(/\s+/).every((w) => hay.includes(w));
}
