// Dev harness: draws Ciel in every state and emote, plus the mini clouds and
// the drop box, in a plain browser. Not part of the app bundle.

import { BotEngine, hexToRGB } from "../src/cloud/engine";
import { Greeting } from "../src/cloud/greeting";
import type { BotEmoteName, BotStateName } from "../src/core/layout";

const SIZE = 150;
const dpr = window.devicePixelRatio || 1;
const cells: { engine: BotEngine; ctx: CanvasRenderingContext2D; canvas: HTMLCanvasElement }[] = [];

function cell(parent: string, label: string, setup: (e: BotEngine) => void, size = SIZE) {
  const wrap = document.createElement("div");
  wrap.className = "cell";
  const canvas = document.createElement("canvas");
  canvas.width = size * dpr;
  canvas.height = size * dpr;
  canvas.style.width = `${size}px`;
  canvas.style.height = `${size}px`;
  const caption = document.createElement("div");
  caption.textContent = label;
  wrap.append(canvas, caption);
  document.getElementById(parent)!.append(wrap);
  const engine = new BotEngine();
  setup(engine);
  cells.push({ engine, ctx: canvas.getContext("2d")!, canvas });
}

const STATES: BotStateName[] = [
  "idle", "working", "thinking", "searching", "approval", "question",
  "error", "finished", "ratelimit", "sleeping", "dizzy",
];
for (const s of STATES) cell("states", s, (e) => e.setState(s, true));

const EMOTES: BotEmoteName[] = ["love", "surprised", "proud", "wink", "yawn", "happy", "annoyed"];
for (const m of EMOTES) {
  cell("emotes", m, (e) => {
    const again = () => { e.triggerEmote(m, 2.4); setTimeout(again, 3200); };
    again();
  });
}
cell("emotes", "greet", (e) => {
  const again = () => { e.greet(); setTimeout(again, 3000); };
  again();
});

for (const [name, hex] of [["GitHub", "#8B949E"], ["Vercel", "#E5E7EB"], ["Stripe", "#635BFF"], ["n8n", "#EA4B71"], ["Codex", "#10A37F"]]) {
  cell("minis", name, (e) => {
    e.isMini = true;
    e.bodyColor = hexToRGB(hex);
  }, 70);
}
cell("minis", "drop box", (e) => {
  const again = () => {
    e.animateMorph(1);
    setTimeout(() => e.gulp(), 900);
    setTimeout(() => e.animateMorph(0), 2600);
    setTimeout(again, 4200);
  };
  again();
});

const greeting = new Greeting();
const gcanvas = document.getElementById("greeting") as HTMLCanvasElement;
gcanvas.width = 640 * dpr;
gcanvas.height = 150 * dpr;
gcanvas.style.width = "640px";
gcanvas.style.height = "150px";
const gctx = gcanvas.getContext("2d")!;
const replay = () => { greeting.start(); setTimeout(replay, 6000); };
replay();

let mouse = { x: 0, y: 0 };
window.addEventListener("mousemove", (ev) => { mouse = { x: ev.clientX, y: ev.clientY }; });

let last = performance.now();
function frame(now: number) {
  const dt = Math.min(0.05, (now - last) / 1000);
  last = now;
  for (const { engine, ctx, canvas } of cells) {
    const r = canvas.getBoundingClientRect();
    engine.lookX = Math.max(-1, Math.min(1, (mouse.x - (r.left + r.width / 2)) / 400));
    engine.lookY = Math.max(-1, Math.min(1, (mouse.y - (r.top + r.height / 2)) / 300));
    engine.update(dt);
    const w = canvas.width / dpr;
    const h = canvas.height / dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.save();
    engine.applyDance(ctx, w, h);
    engine.draw(ctx, w, h);
    ctx.restore();
  }
  gctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  greeting.draw(gctx);
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
