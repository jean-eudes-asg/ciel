// Dev harness: every theme × phase as a still, plus one live animated scene
// and a benchmark of a 3440×1440 frame, in a plain browser (`npx vite`, then
// /dev/sky-preview.html). Not part of the app bundle.

import { DEFAULT_WALLPAPER, MOODS, PHASES, THEMES, type Phase, type Theme } from "../src/sky/prefs";
import { sampleSky, sceneSpec, type SceneSpec } from "../src/sky/scenes";
import { SkyRenderer } from "../src/sky/render";
import type { BotStateName } from "../src/core/layout";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const themeSel = $<HTMLSelectElement>("theme");
const phaseSel = $<HTMLSelectElement>("phase");
const moodSel = $<HTMLSelectElement>("mood");
const seedIn = $<HTMLInputElement>("seed");
const lockIn = $<HTMLInputElement>("lock");
const speedIn = $<HTMLInputElement>("speed");
const aspectSel = $<HTMLSelectElement>("aspect");
const stats = $("stats");

const params = new URLSearchParams(location.search);
for (const t of THEMES) themeSel.append(new Option(t, t));
for (const p of PHASES) phaseSel.append(new Option(p, p));
for (const m of MOODS) moodSel.append(new Option(m, m));
themeSel.value = params.get("theme") ?? "sunset";
phaseSel.value = params.get("phase") ?? "sunset";
moodSel.value = params.get("mood") ?? "idle";
if (params.get("seed")) seedIn.value = params.get("seed")!;
if (params.get("aspect")) aspectSel.value = params.get("aspect")!;
if (params.get("lock")) lockIn.checked = true;

function spec(theme: Theme, phase: Phase, mood: BotStateName, lockScreen = lockIn.checked): SceneSpec {
  const s = sceneSpec(new Date(), mood, DEFAULT_WALLPAPER, {
    choice: { theme, phase }, sky: sampleSky(phase), lockScreen,
  });
  s.seed = Number(seedIn.value) || 1;
  return s;
}

// ── Live ──────────────────────────────────────────────────────────────────────

const live = $<HTMLCanvasElement>("live");
const liveRenderer = new SkyRenderer();
let last = 0;
let t = 40_000;
let frames = 0;
let jsTime = 0;
let statsAt = performance.now();

function sizeLive() {
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const w = live.clientWidth;
  const h = Math.round(w / Number(aspectSel.value));
  live.style.height = `${h}px`;
  live.width = Math.round(w * dpr);
  live.height = Math.round(h * dpr);
}

function tick(now: number) {
  requestAnimationFrame(tick);
  if (now - last < 1000 / 24 - 1) return;
  const dt = Math.min(0.1, (now - last) / 1000);
  last = now;
  t += dt * (speedIn.checked ? 30 : 1);
  const ctx = live.getContext("2d")!;
  const t0 = performance.now();
  liveRenderer.render(ctx, spec(themeSel.value as Theme, phaseSel.value as Phase, moodSel.value as BotStateName), t);
  jsTime += performance.now() - t0;
  frames++;
  if (now - statsAt > 1000) {
    stats.textContent = `${live.width}×${live.height}: ${(jsTime / frames).toFixed(2)} ms/frame (JS), ${frames} fps`;
    frames = 0;
    jsTime = 0;
    statsAt = now;
  }
}

// ── Grid ──────────────────────────────────────────────────────────────────────

function drawGrid() {
  const grid = $("grid");
  grid.replaceChildren();
  grid.append(Object.assign(document.createElement("div"), { className: "head" }));
  for (const p of PHASES) grid.append(Object.assign(document.createElement("div"), { className: "head", textContent: p }));
  const aspect = Number(aspectSel.value);
  const W = 480;
  const H = Math.round(W / aspect);
  const jobs: (() => void)[] = [];
  for (const theme of THEMES) {
    grid.append(Object.assign(document.createElement("div"), { className: "theme", textContent: theme }));
    for (const phase of PHASES) {
      const canvas = document.createElement("canvas");
      canvas.width = W;
      canvas.height = H;
      canvas.title = `${theme} · ${phase}`;
      grid.append(canvas);
      jobs.push(() => new SkyRenderer().render(canvas.getContext("2d")!, spec(theme, phase, moodSel.value as BotStateName), 40_000, { still: true }));
    }
  }
  // A few at a time, so the page stays responsive.
  const run = () => {
    const start = performance.now();
    while (jobs.length && performance.now() - start < 40) jobs.shift()!();
    if (jobs.length) setTimeout(run, 0);
  };
  run();
}

// ── Benchmark ─────────────────────────────────────────────────────────────────

async function bench() {
  const canvas = document.createElement("canvas");
  canvas.width = 3440;
  canvas.height = 1440;
  canvas.style.cssText = "position:fixed;left:0;top:0;width:344px;height:144px;opacity:0.01;pointer-events:none";
  document.body.append(canvas);
  const ctx = canvas.getContext("2d")!;
  const r = new SkyRenderer();
  const s = spec(themeSel.value as Theme, phaseSel.value as Phase, moodSel.value as BotStateName);
  let t0 = performance.now();
  r.render(ctx, s, 1000);
  ctx.getImageData(0, 0, 1, 1);
  const build = performance.now() - t0;
  // Frames as the live page draws them, each waited for on screen.
  const N = 72;
  let js = 0;
  const times: number[] = [];
  for (let i = 0; i < N; i++) {
    // A timer rather than a frame: a background tab throttles frames.
    await new Promise((res) => setTimeout(res, 0));
    t0 = performance.now();
    r.render(ctx, s, 1000 + i / 24);
    js += performance.now() - t0;
    const t1 = performance.now();
    ctx.getImageData(0, 0, 1, 1);
    times.push(performance.now() - t1 + (t1 - t0));
  }
  times.sort((a, b) => a - b);
  canvas.remove();
  return stats.textContent =
    `3440×1440 ${s.theme}/${s.phase}: static layers built in ${build.toFixed(0)} ms; ` +
    `frame ${(js / N).toFixed(2)} ms JS, ${times[Math.floor(N / 2)].toFixed(2)} ms median incl. GPU flush ` +
    `(p95 ${times[Math.floor(N * 0.95)].toFixed(2)} ms) — budget at 24 fps: 41.7 ms`;
}

// ── Inspect ───────────────────────────────────────────────────────────────────
//
// ?inspect=theme,phase[,mood]&region=x0,y0,x1,y1 (fractions): renders a real
// 3440×1440 still and shows that region filling the page, to judge details.

function inspect(arg: string) {
  const [theme, phase, mood = moodSel.value] = arg.split(",");
  const [x0, y0, x1, y1] = (params.get("region") ?? "0,0,1,1").split(",").map(Number);
  const W = Number(params.get("w") ?? 3440);
  const H = Math.round(W / Number(aspectSel.value));
  const full = document.createElement("canvas");
  full.width = W;
  full.height = H;
  const t0 = performance.now();
  new SkyRenderer().render(full.getContext("2d")!, spec(theme as Theme, phase as Phase, mood as BotStateName), Number(params.get("t") ?? 40_000), { still: !params.get("live") });
  const took = performance.now() - t0;
  document.body.replaceChildren();
  document.body.style.margin = "0";
  const view = document.createElement("canvas");
  const sw = (x1 - x0) * W;
  const sh = (y1 - y0) * H;
  view.width = innerWidth;
  view.height = Math.round((innerWidth * sh) / sw);
  view.style.display = "block";
  const c = view.getContext("2d")!;
  c.imageSmoothingQuality = "high";
  c.drawImage(full, x0 * W, y0 * H, sw, sh, 0, 0, view.width, view.height);
  const note = document.createElement("div");
  note.style.cssText = "position:fixed;right:8px;bottom:6px;font:11px monospace;color:#fff;background:#0008;padding:2px 6px;border-radius:4px";
  note.textContent = `${theme}/${phase}/${mood} ${W}×${H} built in ${took.toFixed(0)} ms`;
  document.body.append(view, note);
}

// ?mosaic=theme,phase,mood;theme,phase,mood…: those scenes, two per row, large.
function mosaic(arg: string) {
  document.body.replaceChildren();
  document.body.style.cssText = "margin:0;display:grid;grid-template-columns:1fr 1fr;gap:4px;background:#000";
  const W = Number(params.get("w") ?? 1290);
  const H = Math.round(W / Number(aspectSel.value));
  for (const item of arg.split(";")) {
    const [theme, phase, mood = moodSel.value] = item.split(",");
    const canvas = document.createElement("canvas");
    canvas.width = W;
    canvas.height = H;
    canvas.style.width = "100%";
    new SkyRenderer().render(canvas.getContext("2d")!, spec(theme as Theme, phase as Phase, mood as BotStateName), Number(params.get("t") ?? 40_000), { still: true });
    document.body.append(canvas);
  }
}

for (const el of [themeSel, phaseSel, moodSel, seedIn, lockIn]) el.addEventListener("change", drawGrid);
if (params.get("mosaic")) {
  mosaic(params.get("mosaic")!);
  throw new Error("mosaic mode");
}
if (params.get("inspect")) {
  inspect(params.get("inspect")!);
  throw new Error("inspect mode");
}
aspectSel.addEventListener("change", () => {
  sizeLive();
  drawGrid();
});
$("bench").addEventListener("click", () => void bench());
(window as unknown as { bench: typeof bench }).bench = bench;
window.addEventListener("resize", sizeLive);
sizeLive();
drawGrid();
requestAnimationFrame(tick);
