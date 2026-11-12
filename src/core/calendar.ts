// Tools → Calendar, the pure parts: the month's grid from the week's first
// day where the user lives, the days with events, and the world clocks' time
// and hours apart. No DOM, no Tauri: tested in Node.

/** A day as its key: "2026-11-09", in the computer's time. */
export function dayKey(d: Date): string {
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
}

/** Where the week starts on Sunday (CLDR); elsewhere, Monday. */
const SUNDAY_FIRST = new Set([
  "US", "CA", "MX", "BR", "JP", "PH", "IL", "KR", "TW", "HK", "MO", "IN", "SA", "ZA", "AR", "PE", "CO", "VE",
  "GT", "HN", "PA", "PR", "DO", "JM", "KE", "ZW", "TH", "ID", "SG", "BD", "PK", "NP", "ET",
]);

/** 0 for Sunday, 1 for Monday…: what the locale's calendars start with. */
export function firstDayOfWeek(locale: string): number {
  try {
    const l = new Intl.Locale(locale) as Intl.Locale & { getWeekInfo?: () => { firstDay: number }; weekInfo?: { firstDay: number } };
    const first = l.getWeekInfo?.().firstDay ?? l.weekInfo?.firstDay;
    if (first) return first % 7;
  } catch {
    // An odd tag: the region below decides.
  }
  const region = /[-_]([A-Za-z]{2})\b/.exec(locale)?.[1]?.toUpperCase();
  return region && SUNDAY_FIRST.has(region) ? 0 : 1;
}

/** The six weeks shown for a month (0-11), from the week's first day. */
export function monthGrid(year: number, month: number, firstDay: number): Date[] {
  const first = new Date(year, month, 1);
  const back = (first.getDay() - firstDay + 7) % 7;
  return Array.from({ length: 42 }, (_, i) => new Date(year, month, 1 - back + i));
}

/** The seven days of the week `day` is in, from the locale's first day of the week. */
export function weekDays(day: Date, firstDay: number): Date[] {
  const back = (day.getDay() - firstDay + 7) % 7;
  return Array.from({ length: 7 }, (_, i) => new Date(day.getFullYear(), day.getMonth(), day.getDate() - back + i));
}

/** The ISO 8601 week number: the week with the year's first Thursday is week 1. */
export function isoWeek(day: Date): number {
  const d = new Date(Date.UTC(day.getFullYear(), day.getMonth(), day.getDate()));
  const weekday = d.getUTCDay() || 7;
  d.setUTCDate(d.getUTCDate() + 4 - weekday);
  const yearStart = Date.UTC(d.getUTCFullYear(), 0, 1);
  return Math.ceil(((d.getTime() - yearStart) / 86_400_000 + 1) / 7);
}

/** What comes next: the first timed event not over yet, within `aheadMs`. */
export function nextEvent(events: readonly CalendarEvent[], now: number, aheadMs = 36 * 3600_000): CalendarEvent | null {
  return events
    .filter((e) => !e.allDay && e.end > now && e.start - now < aheadMs)
    .sort((a, b) => a.start - b.start)[0] ?? null;
}

export interface CalendarEvent {
  title: string;
  /** Unix ms. */
  start: number;
  end: number;
  allDay: boolean;
}

/** The events that touch a day, all-day ones first, then by start. */
export function eventsOn(events: readonly CalendarEvent[], day: Date): CalendarEvent[] {
  const from = new Date(day.getFullYear(), day.getMonth(), day.getDate()).getTime();
  const to = new Date(day.getFullYear(), day.getMonth(), day.getDate() + 1).getTime();
  return events
    .filter((e) => e.start < to && Math.max(e.end, e.start + 1) > from)
    .sort((a, b) => Number(b.allDay) - Number(a.allDay) || a.start - b.start);
}

/** Every day an event touches, as dayKey()s. */
export function busyDays(events: readonly CalendarEvent[]): Set<string> {
  const out = new Set<string>();
  for (const e of events) {
    // An all-day event ends at midnight: that day isn't one of its own.
    const last = Math.max(e.start, e.end - 1);
    for (let d = new Date(e.start); d.getTime() <= last && out.size < 1000; d = new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1)) {
      out.add(dayKey(d));
    }
  }
  return out;
}

/** Minutes ahead of UTC in `tz` at `at`. */
export function zoneOffset(tz: string, at: number): number {
  const minute = Math.floor(at / 60_000) * 60_000;
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone: tz, hourCycle: "h23", year: "numeric", month: "numeric", day: "numeric", hour: "numeric", minute: "numeric",
  }).formatToParts(new Date(minute));
  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0);
  return Math.round((Date.UTC(get("year"), get("month") - 1, get("day"), get("hour") % 24, get("minute")) - minute) / 60_000);
}

/** "+1", "-5:30", or "" for the same time as here. */
export function hoursApart(tz: string, at: number, localOffset = -new Date(at).getTimezoneOffset()): string {
  const diff = zoneOffset(tz, at) - localOffset;
  if (diff === 0) return "";
  const sign = diff > 0 ? "+" : "-";
  const h = Math.floor(Math.abs(diff) / 60);
  const m = Math.abs(diff) % 60;
  return m ? `${sign}${h}:${String(m).padStart(2, "0")}` : `${sign}${h}`;
}

/** A time zone the browser knows. */
export function validZone(tz: string): boolean {
  try {
    new Intl.DateTimeFormat("en-US", { timeZone: tz });
    return !!tz;
  } catch {
    return false;
  }
}
