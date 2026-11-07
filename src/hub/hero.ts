// Ciel in the window's pages: one cloud alone, alive (the Games' opponent),
// asleep on an empty page, and the plain card a page shows a feature in. No
// colour of their own: Ciel is white on the black window, a light grey on
// the white one.
//
// One animation loop draws every live canvas; it stops when none is on screen
// or the window is hidden, so a hidden window costs nothing.

import { BotEngine, hexToRGB } from "../cloud/engine";
import type { BotStateName } from "../core/layout";

interface Animated {
  el: HTMLCanvasElement;
  draw(dt: number, t: number): void;
  /** Pages are built before they are put in the window: dropped only once gone. */
  shown?: boolean;
  born?: number;
}

const live = new Set<Animated>();
let frame: number | null = null;
let last = performance.now();
const start = performance.now();

function loop(now: number) {
  frame = null;
  const dt = Math.min(0.05, (now - last) / 1000);
  last = now;
  const t = (now - start) / 1000;
  for (const a of live) {
    if (!a.el.isConnected) {
      // Built but not shown yet: wait for it (a few seconds at most).
      if (a.shown || now - (a.born ??= now) > 5000) live.delete(a);
      continue;
    }
    a.shown = true;
    a.draw(dt, t);
  }
  if (live.size > 0 && document.visibilityState === "visible") frame = requestAnimationFrame(loop);
}

function wake() {
  if (frame === null && live.size > 0 && document.visibilityState === "visible") {
    last = performance.now();
    frame = requestAnimationFrame(loop);
  }
}

document.addEventListener("visibilitychange", wake);

function add(a: Animated) {
  live.add(a);
  wake();
}

function fit(canvas: HTMLCanvasElement): { w: number; h: number; ctx: CanvasRenderingContext2D | null } {
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext("2d");
  ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
  return { w, h, ctx };
}

/** Ciel's body on the window's background: white on black, light grey on white. */
export function cloudColor(): string {
  return document.documentElement.dataset.theme === "white" ? "#D9D9D9" : "#F5F6F8";
}

/** One Ciel alone (the Games' opponent, an empty page). */
export function liveCloud(canvas: HTMLCanvasElement, color = cloudColor(), state: BotStateName = "idle") {
  const engine = new BotEngine();
  engine.bodyColor = hexToRGB(color);
  engine.setState(state, true);
  canvas.addEventListener("click", () => engine.slap());
  let mouse: { x: number; y: number } | null = null;
  window.addEventListener("pointermove", (e) => {
    const r = canvas.getBoundingClientRect();
    mouse = { x: e.clientX - (r.left + r.width / 2), y: e.clientY - (r.top + r.height / 2) };
  });
  add({
    el: canvas,
    draw(dt) {
      const { w, h, ctx } = fit(canvas);
      if (!ctx || w === 0) return;
      if (mouse) {
        // The page's y grows downwards; lookY points up.
        engine.lookX = Math.max(-1, Math.min(1, mouse.x / 300));
        engine.lookY = Math.max(-1, Math.min(1, -mouse.y / 300));
      }
      engine.update(dt);
      ctx.clearRect(0, 0, w, h);
      engine.draw(ctx, w, h);
    },
  });
  return engine;
}

/** A plain card that puts one feature forward (the Notetaker, dictation to set up). */
export function featureCard(...content: (HTMLElement | string)[]): HTMLElement {
  const el = document.createElement("div");
  el.className = "hub-card feature";
  el.append(...content);
  return el;
}

/** An empty page: a Ciel asleep, and a line. */
export function emptyState(text: string, state: BotStateName = "sleeping"): HTMLElement {
  const el = document.createElement("div");
  el.className = "hub-empty";
  const canvas = document.createElement("canvas");
  const p = document.createElement("div");
  p.textContent = text;
  el.append(canvas, p);
  liveCloud(canvas, cloudColor(), state);
  return el;
}
