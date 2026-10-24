// The screensaver's flip clock as a still picture, for the lock screen
// (island/wallpaper.ts): pitch black, the two cards and the date, in the
// screensaver's colours and proportions (screensaver.css, flipclock.css).
// Windows writes its own clock and date across the top of the lock screen and
// can't be told not to, so this one keeps to the part below, in the
// screensaver's size when it fits there.

import { makeCanvas, context, toPng } from "../sky/canvas";
import { clockFace, twelveHours, type ClockFormat } from "./clockformat";

export interface ClockPicture {
  format: ClockFormat;
  size: "small" | "medium" | "large";
  /** The date under the cards. */
  date: boolean;
  lang?: string;
}

const CARD = "#141414";
const INK = "#b4b4b4";
const DATE_INK = "#5e5e5e";
const FONT = 'system-ui, "Segoe UI Variable Text", "Segoe UI", "Noto Sans", sans-serif';
/** Windows' clock and date: the picture's clock stays below this fraction of the height. */
export const WINDOWS_CLOCK_BOTTOM = 0.4;
const BOTTOM = 0.94;

/** The screensaver's font size (screensaver.css) on a `W`×`H` screen. */
function flipSize(size: ClockPicture["size"], W: number, H: number): number {
  const vh = H / 100;
  const vw = W / 100;
  if (size === "small") return Math.min(19 * vh, 11 * vw);
  if (size === "medium") return Math.min(30 * vh, 18 * vw);
  return Math.min(46 * vh, 29 * vw);
}

export interface ClockLayout {
  /** The digits' font size; a card is 1.42 × 1.2 of it. */
  size: number;
  cardW: number;
  cardH: number;
  gap: number;
  left: number;
  top: number;
  dateSize: number;
  /** The date's top, or null without the date. */
  dateTop: number | null;
}

/** Where the cards and the date go: centred under Windows' clock, as big as the screensaver's if they fit. */
export function clockLayout(W: number, H: number, opts: Pick<ClockPicture, "size" | "date">): ClockLayout {
  const dateSize = Math.min(24, Math.max(13, 0.022 * H));
  const dateGap = 0.05 * H;
  const below = opts.date ? dateGap + dateSize : 0;
  const room = (BOTTOM - WINDOWS_CLOCK_BOTTOM) * H;
  const size = Math.min(flipSize(opts.size, W, H), (room - below) / 1.2);
  const cardW = size * 1.42;
  const cardH = size * 1.2;
  const gap = size * 0.1;
  const top = WINDOWS_CLOCK_BOTTOM * H + (room - cardH - below) / 2;
  return {
    size, cardW, cardH, gap,
    left: (W - 2 * cardW - gap) / 2,
    top,
    dateSize,
    dateTop: opts.date ? top + cardH + dateGap : null,
  };
}

function roundRectPath(c: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  c.beginPath();
  c.moveTo(x + r, y);
  c.arcTo(x + w, y, x + w, y + h, r);
  c.arcTo(x + w, y + h, x, y + h, r);
  c.arcTo(x, y + h, x, y, r);
  c.arcTo(x, y, x + w, y, r);
  c.closePath();
}

function card(c: CanvasRenderingContext2D, l: ClockLayout, x: number, text: string) {
  c.fillStyle = CARD;
  roundRectPath(c, x, l.top, l.cardW, l.cardH, l.size * 0.12);
  c.fill();
  c.fillStyle = INK;
  c.font = `700 ${l.size}px ${FONT}`;
  c.textAlign = "center";
  c.textBaseline = "alphabetic";
  const m = c.measureText(text);
  c.fillText(text, x + l.cardW / 2, l.top + l.cardH / 2 + (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 2);
  // The hinge: a hairline of shadow across the middle.
  const hinge = Math.max(1, l.size * 0.02);
  c.fillStyle = "rgba(0, 0, 0, 0.55)";
  c.fillRect(x, l.top + (l.cardH - hinge) / 2, l.cardW, hinge);
}

/** The whole picture into `c`, `W`×`H` device pixels, showing `now`. */
export function paintFlipClock(c: CanvasRenderingContext2D, W: number, H: number, now: Date, opts: ClockPicture) {
  const l = clockLayout(W, H, opts);
  const face = clockFace(now, twelveHours(opts.format, opts.lang), opts.lang);
  c.fillStyle = "#000";
  c.fillRect(0, 0, W, H);
  card(c, l, l.left, face.hours);
  card(c, l, l.left + l.cardW + l.gap, face.minutes);
  if (face.period) {
    // AM / PM, small in the hours card's top corner, as on the screensaver.
    c.globalAlpha = 0.7;
    c.fillStyle = INK;
    c.font = `600 ${l.size * 0.12}px ${FONT}`;
    c.textAlign = "left";
    c.textBaseline = "top";
    c.fillText(face.period, l.left + l.size * 0.09, l.top + l.size * 0.07);
    c.globalAlpha = 1;
  }
  if (l.dateTop != null) {
    c.fillStyle = DATE_INK;
    c.font = `500 ${l.dateSize}px ${FONT}`;
    if ("letterSpacing" in c) c.letterSpacing = `${l.dateSize * 0.2}px`;
    c.textAlign = "center";
    c.textBaseline = "top";
    const date = now.toLocaleDateString(opts.lang, { weekday: "long", day: "numeric", month: "long" });
    c.fillText(date.toLocaleUpperCase(opts.lang), W / 2, l.dateTop);
  }
}

/** The picture as a PNG. */
export async function clockPng(W: number, H: number, now: Date, opts: ClockPicture): Promise<Uint8Array> {
  const canvas = makeCanvas(W, H);
  paintFlipClock(context(canvas), W, H, now, opts);
  return toPng(canvas);
}
