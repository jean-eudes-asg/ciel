// A split-flap clock, like an old flip clock (and Fliqlo): the hours and the
// minutes each on a card, whose top half folds down onto the next value. One
// clock for the island (tiny), Home (large) and the screensaver (huge): its
// size is the --flip-size font size, its colours --flip-card and --flip-ink.

import "./flipclock.css";

export interface FlipClock {
  el: HTMLElement;
  /** Shows `now`; a card that changes flips. */
  set(now: Date): void;
}

const FLIP_MS = 640;

/** "auto" follows the interface's language; Settings → General → Clock. */
export type ClockFormat = "auto" | "12h" | "24h";
let format: ClockFormat = "auto";
/** The clocks on screen, redrawn when the format changes. */
const clocks = new Set<FlipClock>();

export function setClockFormat(next: ClockFormat | undefined) {
  const f = next === "12h" || next === "24h" ? next : "auto";
  if (f === format) return;
  format = f;
  for (const c of clocks) {
    if (c.el.isConnected) c.set(new Date());
    else clocks.delete(c);
  }
}

interface Card {
  el: HTMLElement;
  shown: string;
  topNext: HTMLElement;
  bottomNow: HTMLElement;
  flapNow: HTMLElement;
  flapNext: HTMLElement;
  timer: number | null;
}

function half(cls: string): [HTMLElement, HTMLElement] {
  const el = document.createElement("div");
  el.className = cls;
  const span = document.createElement("span");
  el.append(span);
  return [el, span];
}

function card(): Card {
  const el = document.createElement("div");
  el.className = "flip-card";
  const [top, topNext] = half("flip-half flip-top");
  const [bottom, bottomNow] = half("flip-half flip-bottom");
  const [flapTop, flapNow] = half("flip-flap flip-top");
  const [flapBottom, flapNext] = half("flip-flap flip-bottom");
  el.append(top, bottom, flapTop, flapBottom);
  return { el, shown: "", topNext, bottomNow, flapNow, flapNext, timer: null };
}

const still = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

function show(c: Card, value: string) {
  if (value === c.shown) return;
  const before = c.shown;
  c.shown = value;
  if (c.timer != null) window.clearTimeout(c.timer);
  c.topNext.textContent = value;
  c.flapNext.textContent = value;
  if (!before || still()) {
    c.bottomNow.textContent = value;
    c.flapNow.textContent = value;
    c.el.classList.remove("flipping");
    return;
  }
  // The old value's top folds down over the new one, then the new bottom lands.
  c.bottomNow.textContent = before;
  c.flapNow.textContent = before;
  c.el.classList.remove("flipping");
  void c.el.offsetWidth;
  c.el.classList.add("flipping");
  c.timer = window.setTimeout(() => {
    c.timer = null;
    c.bottomNow.textContent = value;
    c.flapNow.textContent = value;
    c.el.classList.remove("flipping");
  }, FLIP_MS);
}

/** 12 or 24 hours: as chosen, or as the interface's language says. */
function twelveHours(lang: string | undefined): boolean {
  if (format !== "auto") return format === "12h";
  const cycle = new Intl.DateTimeFormat(lang, { hour: "numeric" }).resolvedOptions().hourCycle;
  return cycle === "h12" || cycle === "h11";
}

export function buildFlipClock(className = ""): FlipClock {
  const el = document.createElement("div");
  el.className = `flip-clock ${className}`.trim();
  const hours = card();
  const minutes = card();
  const period = document.createElement("span");
  period.className = "flip-period";
  hours.el.append(period);
  el.append(hours.el, minutes.el);

  const clock: FlipClock = {
    el,
    set(now) {
      const lang = document.documentElement.lang || undefined;
      const twelve = twelveHours(lang);
      let h = now.getHours();
      if (twelve) h = h % 12 || 12;
      show(hours, twelve ? String(h) : String(h).padStart(2, "0"));
      show(minutes, String(now.getMinutes()).padStart(2, "0"));
      period.textContent = twelve
        ? new Intl.DateTimeFormat(lang, { hour: "numeric", hour12: true }).formatToParts(now).find((p) => p.type === "dayPeriod")?.value ?? ""
        : "";
    },
  };
  clocks.add(clock);
  return clock;
}

/** Sets `clock` now and on every minute after, until `stop` is called. */
export function tick(clock: FlipClock): () => void {
  let timer = 0;
  const next = () => {
    const now = new Date();
    clock.set(now);
    timer = window.setTimeout(next, 60_000 - (now.getTime() % 60_000) + 20);
  };
  next();
  return () => window.clearTimeout(timer);
}
