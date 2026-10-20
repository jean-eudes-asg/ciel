// Notifications from other apps, shown in the island (island/notifications.ts):
// new mail in Gmail (src-tauri/src/gmail.rs), and what websites in Chrome, Edge
// or Brave show (the Ciel for Chrome extension, through `ciel-hook --chrome`;
// src-tauri/src/notify.rs), and on Linux the other apps' (notify.rs too). Pure rules here; nothing is stored, nothing is kept
// past the queue.

export type NotificationSource = "chrome" | "gmail" | "desktop" | "ciel";

export interface CielNotification {
  /** Unique, given by Rust. */
  id: string;
  source: NotificationSource;
  /** The app or site it comes from: "Gmail", "WhatsApp", "calendar.google.com". */
  app: string;
  title: string;
  body: string;
  /** A data: URL, or null. */
  icon: string | null;
  /** Unix ms. */
  at: number;
  /** A notification with the same tag replaces the older one (a site updating its own). */
  tag: string | null;
  /** The card's own buttons instead of Open (a meeting's Take notes and Join); the last is the white one. */
  buttons: NotificationButton[];
  /** Unix ms: the card stays in the island until then (a meeting's start), whatever the setting says. */
  until: number | null;
  /** A meeting's start and end, Unix ms. */
  time: { start: number; end: number } | null;
}

export interface NotificationButton {
  /** What comes back to Rust as the action. */
  id: string;
  label: string;
}

/** How long a notification shows in the island before it folds away (unless hovered), without a setting. */
export const SHOW_MS = 7000;
/** The choices for how long, in seconds (Settings); 0 is until opened or dismissed. */
export const SHOW_CHOICES = [5, 7, 10, 20, 0] as const;
/** The most kept waiting. */
export const MAX_QUEUED = 20;
/** The most kept in Ciel's list until seen (Rust keeps what Open needs for 64). */
export const MAX_UNSEEN = 50;

/** Longest title and body shown: a page can send a book. */
const MAX_TITLE = 160;
const MAX_BODY = 600;

const clip = (s: unknown, max: number) => {
  const text = typeof s === "string" ? s.replace(/\s+/g, " ").trim() : "";
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
};

/** A notification as Rust sent it, checked and trimmed; null when it has nothing to show. */
export function parseNotification(raw: unknown): CielNotification | null {
  if (!raw || typeof raw !== "object") return null;
  const r = raw as Record<string, unknown>;
  const source = r.source === "gmail" || r.source === "chrome" || r.source === "desktop" || r.source === "ciel" ? r.source : null;
  const title = clip(r.title, MAX_TITLE);
  const body = clip(r.body, MAX_BODY);
  if (!source || typeof r.id !== "string" || !r.id || (!title && !body)) return null;
  const icon = typeof r.icon === "string" && r.icon.startsWith("data:image/") ? r.icon : null;
  return {
    id: r.id,
    source,
    app: clip(r.app, 60) || { gmail: "Gmail", chrome: "Chrome", desktop: "", ciel: "Ciel" }[source],
    title: title || body,
    body: title ? body : "",
    icon,
    at: typeof r.at === "number" && Number.isFinite(r.at) ? r.at : Date.now(),
    tag: typeof r.tag === "string" && r.tag ? r.tag : null,
    buttons: parseButtons(r.buttons),
    until: typeof r.until === "number" && Number.isFinite(r.until) ? r.until : null,
    time: parseTime(r.time),
  };
}

/** At most three, each with an id and a label. */
function parseButtons(raw: unknown): NotificationButton[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter((b): b is Record<string, unknown> => !!b && typeof b === "object")
    .map((b) => ({ id: typeof b.id === "string" ? b.id : "", label: clip(b.label, 30) }))
    .filter((b) => b.id && b.id !== "dismiss" && b.label)
    .slice(0, 3);
}

function parseTime(raw: unknown): { start: number; end: number } | null {
  if (!raw || typeof raw !== "object") return null;
  const { start, end } = raw as Record<string, unknown>;
  if (typeof start !== "number" || !Number.isFinite(start)) return null;
  return { start, end: typeof end === "number" && Number.isFinite(end) && end >= start ? end : start };
}

/** "14:00 – 14:30" in the interface's language. */
export function timeSpan(time: { start: number; end: number }, lang?: string): string {
  const clock = (ms: number) => new Date(ms).toLocaleTimeString(lang, { hour: "2-digit", minute: "2-digit" });
  return time.end > time.start ? `${clock(time.start)} – ${clock(time.end)}` : clock(time.start);
}

/**
 * How long the card shows, in ms; 0 is until opened or dismissed. A card
 * with an end of its own (a meeting) stays until then.
 */
export function showFor(n: CielNotification, seconds: unknown, now = Date.now()): number {
  if (n.until != null) return Math.max(1000, n.until - now);
  return typeof seconds === "number" && seconds >= 0 ? seconds * 1000 : SHOW_MS;
}

/** The queue with `n` in front: one with the same app and tag goes, and the oldest past the cap. */
export function addNotification(queue: readonly CielNotification[], n: CielNotification, max = MAX_QUEUED): CielNotification[] {
  const rest = queue.filter((q) => q.id !== n.id && !(n.tag && q.tag === n.tag && q.app === n.app));
  return [n, ...rest].slice(0, max);
}
