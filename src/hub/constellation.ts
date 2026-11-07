// Ciel's silhouette (cloud/shape.ts) as flat SVG: the window's logo, the mark
// beside the page you are on, and the constellation of the setup — one star
// per step on his outline. A finished step lights its star; two finished
// steps side by side light the outline between them; all of them, and he
// opens his eyes.

import { outlineAt } from "../cloud/shape";

const NS = "http://www.w3.org/2000/svg";

function svg(w: number, h: number, cls: string): SVGSVGElement {
  const el = document.createElementNS(NS, "svg");
  el.setAttribute("viewBox", `0 0 ${w} ${h}`);
  el.setAttribute("aria-hidden", "true");
  el.setAttribute("class", cls);
  return el;
}

function node(name: string, attrs: Record<string, string | number>): SVGElement {
  const el = document.createElementNS(NS, name);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  return el;
}

/** The point of his outline at `deg` degrees (0 = right, 90 = down), radius `r` around (cx, cy). */
function at(deg: number, r: number, cx: number, cy: number): [number, number] {
  const a = (deg * Math.PI) / 180;
  const d = outlineAt(a) * r;
  return [cx + Math.cos(a) * d, cy + Math.sin(a) * d];
}

const f = (n: number) => n.toFixed(1);

/** His whole outline as a closed path. */
export function cielPath(r: number, cx: number, cy: number, steps = 144): string {
  let d = "";
  for (let i = 0; i < steps; i++) {
    const [x, y] = at((i / steps) * 360, r, cx, cy);
    d += `${i ? "L" : "M"}${f(x)} ${f(y)}`;
  }
  return `${d}Z`;
}

/** His outline from `from` to `to` degrees, clockwise. */
function arc(from: number, to: number, r: number, cx: number, cy: number): string {
  const end = to < from ? to + 360 : to;
  let d = "";
  for (let deg = from; deg <= end; deg += 1.5) {
    const [x, y] = at(deg, r, cx, cy);
    d += `${d ? "L" : "M"}${f(x)} ${f(y)}`;
  }
  return d;
}

/** Ciel as a flat glyph in the text colour (`currentColor`), `w` wide. */
export function cielMark(w = 26, cls = "ciel-mark"): SVGSVGElement {
  const h = Math.round(w * 0.8);
  const el = svg(w, h, cls);
  // R so that his 2.14 R width fills the box, his middle a hair below centre.
  el.append(node("path", { d: cielPath(w / 2.2, w / 2, h / 2 + w * 0.02), fill: "currentColor" }));
  return el;
}

/** Where the steps' stars sit on his outline: up his left side, over the crown, down and along the base. */
const STAR_ANGLES = [200, 245, 290, 335, 25, 100, 155];

export interface Sky {
  /** One per step, in order. */
  done: boolean[];
  /** The step being shown, or -1. */
  current: number;
  big: boolean;
}

/** The setup's constellation, `w` × `h`, white on the night (it always sits on black). */
export function constellation(w: number, h: number, sky: Sky): SVGSVGElement {
  const el = svg(w, h, sky.big ? "constellation" : "constellation mini");
  const r = Math.min(w / 2.3, h / 1.85);
  const cx = w / 2;
  const cy = h / 2 + r * 0.04;
  const n = STAR_ANGLES.length;
  const all = sky.done.length >= n && sky.done.slice(0, n).every(Boolean);

  el.append(node("path", {
    d: cielPath(r, cx, cy),
    fill: "none",
    stroke: sky.big ? "rgba(255,255,255,0.3)" : "rgba(255,255,255,0.22)",
    "stroke-width": sky.big ? 1.2 : 1,
    "stroke-dasharray": sky.big ? "0.5 5" : "0.5 2.5",
    "stroke-linecap": "round",
  }));
  for (let i = 0; i < n; i++) {
    const j = (i + 1) % n;
    // The way back to the first star closes him only once every step is done.
    const lit = j === 0 ? all : sky.done[i] && sky.done[j];
    if (!lit) continue;
    el.append(node("path", {
      d: arc(STAR_ANGLES[i], STAR_ANGLES[j], r, cx, cy),
      fill: "none",
      stroke: "rgba(255,255,255,0.78)",
      "stroke-width": sky.big ? 1.6 : 1.1,
      "stroke-linecap": "round",
      "stroke-linejoin": "round",
    }));
  }
  STAR_ANGLES.forEach((deg, i) => {
    const [x, y] = at(deg, r, cx, cy);
    const done = sky.done[i];
    const now = i === sky.current;
    if (now && sky.big) {
      el.append(node("circle", { cx: f(x), cy: f(y), r: 11, fill: "none", stroke: "rgba(255,255,255,0.45)", "stroke-width": 1 }));
    }
    const size = sky.big ? (now ? 5.5 : done ? 4 : 2.5) : now ? 3 : done ? 2.3 : 1.5;
    el.append(node("circle", { cx: f(x), cy: f(y), r: size, fill: done || now ? "#fff" : "rgba(255,255,255,0.38)" }));
  });
  if (all && sky.big) {
    for (const side of [-1, 1]) {
      el.append(node("ellipse", { cx: f(cx + side * 0.22 * r), cy: f(cy + 0.02 * r), rx: 5, ry: 6.5, fill: "#fff" }));
    }
  }
  return el;
}

/** A still field of faint stars on `canvas`: drawn once, and again on resize. */
export function starField(canvas: HTMLCanvasElement, seed = 7) {
  let drawn = false;
  const draw = () => {
    if (drawn && !canvas.isConnected) {
      observer.disconnect();
      return;
    }
    drawn = true;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    if (!w || !h) return;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const g = canvas.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    let s = seed;
    const rnd = () => (s = (s * 16807) % 2147483647) / 2147483647;
    const count = Math.round((w * h) / 2600);
    for (let i = 0; i < count; i++) {
      const x = rnd() * w;
      const y = rnd() * h;
      const big = rnd() > 0.94;
      g.fillStyle = `rgba(255,255,255,${(0.1 + rnd() * 0.32).toFixed(2)})`;
      g.beginPath();
      g.arc(x, y, big ? 1.1 : 0.3 + rnd() * 0.6, 0, Math.PI * 2);
      g.fill();
    }
  };
  const observer = new ResizeObserver(draw);
  observer.observe(canvas);
}
