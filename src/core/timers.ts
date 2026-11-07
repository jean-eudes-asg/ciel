// Timers as people type them (Tools → Timers), and how they read. Pure.
//
// The quick field takes what reads the same in every language: "5" (minutes),
// "25m", "1h30", "90s", "4 min tea", and a time of day for a reminder —
// "18:30 call mum", "6pm", "18h30". Words around them become the label.

import type { TimerItem } from "./tools";

export type Quick =
  | { kind: "timer"; label: string; ms: number }
  | { kind: "reminder"; label: string; due: number };

const MIN = 60_000;
const UNIT: Record<string, number> = { h: 60 * MIN, m: MIN, s: 1000 };

/** "min", "mins", "minutes", "minuto"… → "m"; "hrs", "heures"… → "h". */
function unitOf(word: string): string | null {
  const w = word.toLowerCase();
  if (/^(h|hr|hrs|hours?|heures?|horas?|std|ч|小时|घंटे)$/.test(w)) return "h";
  if (/^(m|mn|min|mins|minutes?|minutos?|menit|мин|分钟?|मिनट)$/.test(w)) return "m";
  if (/^(s|sec|secs|seconds?|secondes?|segundos?|detik|сек|秒|सेकंड)$/.test(w)) return "s";
  return null;
}

/**
 * What the quick field asks for, at `now`; null when it says nothing usable.
 * A time already gone today is tomorrow's.
 */
export function parseQuick(input: string, now: Date = new Date()): Quick | null {
  let text = ` ${input.trim()} `;
  if (!text.trim()) return null;

  // A time of day: 18:30, 6:05pm, 6pm, 18h30 (from 5h on: before, "1h30" is a length).
  const time =
    /(?:^|\s)(?:at\s+|@\s*|à\s+)?(\d{1,2})[:.](\d{2})\s*(am|pm)?(?=\s)/i.exec(text) ??
    /(?:^|\s)(?:at\s+|@\s*|à\s+)?(\d{1,2})\s*(am|pm)(?=\s)/i.exec(text) ??
    /(?:^|\s)(?:à\s+)?((?:[5-9]|1\d|2[0-3]))h(\d{2})?(?=\s)/i.exec(text);
  if (time) {
    let hours = Number(time[1]);
    const minutes = time[2] && /^\d+$/.test(time[2]) ? Number(time[2]) : 0;
    const period = (time[3] ?? (time[2] && !/^\d+$/.test(time[2]) ? time[2] : "")).toLowerCase();
    if (period === "pm" && hours < 12) hours += 12;
    if (period === "am" && hours === 12) hours = 0;
    if (hours > 23 || minutes > 59) return null;
    const due = new Date(now);
    due.setHours(hours, minutes, 0, 0);
    if (due.getTime() <= now.getTime()) due.setDate(due.getDate() + 1);
    const label = tidy(text.replace(time[0], " "));
    return { kind: "reminder", label, due: due.getTime() };
  }

  // A length: one or more "<number><unit>", or a lone number (minutes).
  let ms = 0;
  let found = false;
  // "1h30": the minutes after the hours.
  text = text.replace(/(\d+)\s*h\s*(\d{1,2})(?=\s|$)/gi, (_, h: string, m: string) => {
    ms += Number(h) * UNIT.h + Number(m) * MIN;
    found = true;
    return " ";
  });
  text = text.replace(/(\d+(?:[.,]\d+)?)\s*([\p{L}]+)/gu, (all, n: string, word: string) => {
    const unit = unitOf(word);
    if (!unit) return all;
    ms += Number(n.replace(",", ".")) * UNIT[unit];
    found = true;
    return " ";
  });
  if (!found) {
    const lone = /(?:^|\s)(\d+(?:[.,]\d+)?)(?=\s)/.exec(text);
    if (!lone) return null;
    ms = Number(lone[1].replace(",", ".")) * MIN;
    text = text.replace(lone[0], " ");
  }
  if (!(ms >= 1000) || ms > 24 * 60 * MIN) return null;
  return { kind: "timer", label: tidy(text), ms: Math.round(ms) };
}

/** What's left of the words, without a dangling "in", "for", "at". */
function tidy(text: string): string {
  return text
    .replace(/\s+/g, " ")
    .trim()
    .replace(/^(in|for|at|dans|pour|en|à|de)\s+/i, "")
    .replace(/\s+(in|for|at|dans|pour|en|à)$/i, "")
    .slice(0, 80);
}

/** Ms left: what a paused one kept, or until it rings. */
export function msLeft(t: TimerItem, now: number): number {
  if (t.left != null) return t.left;
  return Math.max(0, t.due - now);
}

/** 0…1 of the way done (a reminder is never "on its way"). */
export function progress(t: TimerItem, now: number): number {
  if (t.kind === "reminder" || !(t.duration > 0)) return 0;
  return Math.max(0, Math.min(1, 1 - msLeft(t, now) / t.duration));
}

/** "4:59", "1:02:03", "0:07". */
export function clock(ms: number): string {
  const total = Math.ceil(ms / 1000);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** The ones that ring soonest first, ringing ones before all. */
export function ordered(list: readonly TimerItem[], now: number): TimerItem[] {
  return [...list].sort((a, b) => Number(b.ringing) - Number(a.ringing) || msLeft(a, now) - msLeft(b, now));
}

/** The presets under the quick field: minutes, and the word for each (English keys). */
export const PRESETS: readonly { minutes: number }[] = [
  { minutes: 1 },
  { minutes: 3 },
  { minutes: 5 },
  { minutes: 10 },
  { minutes: 15 },
  { minutes: 30 },
];
