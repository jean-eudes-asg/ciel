// Constellations on the Games page (stars.ts): in Stratus's night sky, join
// each pair of stars of one colour with a line, the lines never crossing.
// Draw from a star (or from the end of a line); going back over your own line
// shortens it, crossing another cuts that one. All joined, and Ciel's brain
// names the constellation you drew; every point filled, and it's perfect.

import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { Bridge } from "../core/bridge";
import { Sound } from "../core/sound";
import { button } from "../hub/ui";
import { SkyRenderer } from "../sky/render";
import { rival, skySpec } from "./cast";
import { adjacent, progress, sky as makeSky, type Sky } from "./stars";
import { Sfx } from "./sfx";

const KEY = "ciel.stars";
const SKY_FPS = 20;
/** The stars' colours, as real stars have them, then a few to tell ten pairs apart. */
const COLORS = ["#7FB2FF", "#FFD86B", "#FF8A5C", "#FF5E7A", "#F2F4FF", "#5FE0B5", "#C792FF", "#FF9BD2", "#5CD3FF", "#C8E66A"];

interface Saved {
  level: number;
  perfect: number;
}

function load(): Saved {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "") as Saved;
    if (typeof s.level === "number") return { level: s.level, perfect: s.perfect ?? 0 };
  } catch {
    // A first night.
  }
  return { level: 1, perfect: 0 };
}

export function starsGame(): { el: HTMLElement; leave(): void } {
  const saved = load();
  const keep = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(saved));
    } catch {
      // Not kept.
    }
  };

  const canvas = h("canvas", { class: "cl-canvas st-canvas" }) as HTMLCanvasElement;
  const levelText = h("b", {});
  const joinedText = h("span", { class: "mz-time" });
  const filledText = h("span", { class: "mz-best" });
  const banner = h("div", { class: "mz-banner" });
  const bar = h("div", { class: "mz-bar" },
    levelText, joinedText, filledText, h("span", { class: "spacer" }),
    button(t("Clear"), () => {
      lines = s.pairs.map(() => []);
      update();
    }, "", true),
    button(t("New sky"), () => begin(), "", true),
  );
  const el = h("div", { class: "mz" }, bar, h("div", { class: "mz-stage" }, canvas, banner),
    h("div", { class: "mz-help", text: t("Join each pair of stars; the lines can't cross. Fill the whole sky for a perfect one.") }));

  const night = rival("stratus");
  const renderer = new SkyRenderer();
  let s: Sky = makeSky(saved.level);
  let lines: number[][] = [];
  let done = false;
  let joinedBefore = 0;
  let skyLayer: HTMLCanvasElement | null = null;
  let skyAt = 0;
  let geo = { x: 0, y: 0, cell: 40 };
  let flash = 0;

  function begin() {
    s = makeSky(saved.level);
    lines = s.pairs.map(() => []);
    done = false;
    joinedBefore = 0;
    banner.classList.remove("on");
    levelText.textContent = t("Level {0}", { 0: saved.level });
    update();
  }

  const pairAt = (cell: number) => s.pairs.findIndex(([a, b]) => a === cell || b === cell);
  const lineAt = (cell: number) => lines.findIndex((l) => l.includes(cell));

  function update() {
    const p = progress(s, lines);
    joinedText.textContent = t("{0}/{1} joined", { 0: p.joined, 1: s.pairs.length });
    filledText.textContent = `${Math.round(p.filled * 100)}%`;
    if (p.joined > joinedBefore) Sfx.star();
    joinedBefore = p.joined;
    if (p.joined === s.pairs.length && !done) finish(p.filled === 1);
  }

  function finish(perfect: boolean) {
    done = true;
    flash = 1;
    Sfx.tune("win");
    Sound.play(perfect ? "love" : "proud");
    if (perfect) saved.perfect++;
    const name = h("em", {});
    clear(banner);
    banner.append(
      h("b", { text: perfect ? t("A perfect sky!") : t("All joined!") }),
      name,
      button(t("Next sky"), () => {
        saved.level++;
        keep();
        begin();
      }, "primary", true),
    );
    keep();
    window.setTimeout(() => banner.classList.add("on"), 900);
    // Ciel names what you drew: the shape as text, for the brain.
    const shape = lines.map((l) => l.map((c) => `(${c % s.size},${Math.floor(c / s.size)})`).join("-")).join("; ");
    void Bridge.gameLine([
      "You are Ciel himself, the little cloud who lives at the top of the user's screen: playful and warm.",
      `The game is Constellations: the user just joined ${s.pairs.length} pairs of stars on a ${s.size}x${s.size} night sky${perfect ? ", filling every point" : ""}.`,
      `Their lines, as points (column,row): ${shape}`,
      "Give the constellation they drew a short poetic name of two to four words, then a few words on why, all as one line.",
    ].join("\n")).then((line) => {
      if (line && done) name.textContent = line;
    });
  }

  // ── Drawing lines ──
  const cellAt = (x: number, y: number) => {
    const r = canvas.getBoundingClientRect();
    const c = Math.floor((x - r.left - geo.x) / geo.cell);
    const row = Math.floor((y - r.top - geo.y) / geo.cell);
    return c < 0 || row < 0 || c >= s.size || row >= s.size ? -1 : row * s.size + c;
  };
  let drawing = -1;

  canvas.addEventListener("pointerdown", (e) => {
    if (done) return;
    const cell = cellAt(e.clientX, e.clientY);
    if (cell < 0) return;
    const star = pairAt(cell);
    if (star >= 0) {
      // From a star: its line starts again from here.
      lines[star] = [cell];
      drawing = star;
    } else {
      const k = lineAt(cell);
      if (k < 0) return;
      lines[k] = lines[k].slice(0, lines[k].indexOf(cell) + 1);
      drawing = k;
    }
    canvas.setPointerCapture(e.pointerId);
    Sfx.pick();
    update();
  });

  canvas.addEventListener("pointermove", (e) => {
    if (drawing < 0) return;
    const cell = cellAt(e.clientX, e.clientY);
    const line = lines[drawing];
    const end = line[line.length - 1];
    if (cell < 0 || cell === end || !adjacent(s.size, end, cell)) return;
    const [a, b] = s.pairs[drawing];
    // Already at the other star: the line is done.
    if (line.length > 1 && (end === a || end === b)) {
      if (line.includes(cell)) lines[drawing] = line.slice(0, line.indexOf(cell) + 1);
      return;
    }
    if (line.includes(cell)) {
      lines[drawing] = line.slice(0, line.indexOf(cell) + 1);
      Sfx.step();
      return update();
    }
    const star = pairAt(cell);
    if (star >= 0 && star !== drawing) return;
    // Over another line: that one is cut there.
    const other = lineAt(cell);
    if (other >= 0) {
      lines[other] = lines[other].slice(0, lines[other].indexOf(cell));
      Sfx.bump();
    }
    line.push(cell);
    Sfx.step();
    update();
  });
  const stop = () => (drawing = -1);
  canvas.addEventListener("pointerup", stop);
  canvas.addEventListener("pointercancel", stop);

  // ── The night ──
  const sparkle = (x: CanvasRenderingContext2D, px: number, py: number, r: number, turn: number, color: string) => {
    x.save();
    x.translate(px, py);
    x.rotate(turn);
    x.fillStyle = color;
    x.beginPath();
    for (let k = 0; k < 8; k++) {
      const rr = k % 2 ? r * 0.3 : r;
      const a = (k * Math.PI) / 4;
      x.lineTo(Math.cos(a) * rr, Math.sin(a) * rr);
    }
    x.closePath();
    x.fill();
    x.restore();
  };

  function draw(dt: number, now: number) {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = canvas.clientWidth;
    const hgt = canvas.clientHeight;
    if (!w || !hgt) return;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(hgt * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(hgt * dpr);
      skyLayer = null;
      renderer.reset();
    }
    const cell = Math.floor(Math.min((w - 40) / s.size, (hgt - 40) / s.size));
    geo = { x: Math.round((w - cell * s.size) / 2), y: Math.round((hgt - cell * s.size) / 2), cell };
    if (!skyLayer || now - skyAt > 1000 / SKY_FPS) {
      skyAt = now;
      skyLayer ??= document.createElement("canvas");
      skyLayer.width = canvas.width;
      skyLayer.height = canvas.height;
      renderer.render(skyLayer.getContext("2d")!, skySpec(night), now / 1000 + 36_000);
    }
    const x = canvas.getContext("2d")!;
    x.setTransform(1, 0, 0, 1, 0, 0);
    x.drawImage(skyLayer, 0, 0);
    x.setTransform(dpr, 0, 0, dpr, 0, 0);
    const centre = (i: number) => [geo.x + ((i % s.size) + 0.5) * cell, geo.y + (Math.floor(i / s.size) + 0.5) * cell];
    flash = Math.max(0, flash - dt * 0.6);

    // The points of the sky still free.
    const used = new Set(lines.flat());
    x.fillStyle = "rgba(220,228,255,0.28)";
    for (let i = 0; i < s.size * s.size; i++) {
      if (used.has(i)) continue;
      const [px, py] = centre(i);
      x.beginPath();
      x.arc(px, py, Math.max(1.5, cell * 0.035), 0, Math.PI * 2);
      x.fill();
    }

    // The lines: a wide soft glow, then a fine bright one.
    x.lineCap = "round";
    x.lineJoin = "round";
    lines.forEach((l, k) => {
      if (l.length < 2) return;
      const color = COLORS[k % COLORS.length];
      const path = new Path2D();
      l.forEach((cellIndex, j) => {
        const [px, py] = centre(cellIndex);
        if (j) path.lineTo(px, py);
        else path.moveTo(px, py);
      });
      x.globalAlpha = 0.22 + 0.3 * flash;
      x.strokeStyle = color;
      x.lineWidth = cell * 0.42;
      x.stroke(path);
      x.globalAlpha = 0.95;
      x.lineWidth = Math.max(2, cell * 0.08);
      x.stroke(path);
      x.globalAlpha = 1;
    });

    // The stars.
    s.pairs.forEach((pair, k) => {
      const color = COLORS[k % COLORS.length];
      const joined = progress({ size: s.size, pairs: [pair] }, [lines[k]]).joined === 1;
      for (const cellIndex of pair) {
        const [px, py] = centre(cellIndex);
        const tw = 0.85 + 0.15 * Math.sin(now / 300 + cellIndex);
        const g = x.createRadialGradient(px, py, 0, px, py, cell * 0.6);
        g.addColorStop(0, color);
        g.addColorStop(0.3, `${color}88`);
        g.addColorStop(1, `${color}00`);
        x.fillStyle = g;
        x.beginPath();
        x.arc(px, py, cell * 0.6 * (joined ? 1.15 : 1), 0, Math.PI * 2);
        x.fill();
        sparkle(x, px, py, cell * 0.3 * tw, joined ? now / 1500 : 0, color);
        x.fillStyle = "#ffffff";
        x.beginPath();
        x.arc(px, py, cell * 0.06, 0, Math.PI * 2);
        x.fill();
      }
    });
  }

  let frame = 0;
  let last = performance.now();
  const loop = (now: number) => {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    if (el.isConnected && document.visibilityState === "visible") draw(dt, now);
    frame = requestAnimationFrame(loop);
  };

  void Sound.preload(Bridge);
  begin();
  frame = requestAnimationFrame(loop);

  return {
    el,
    leave: () => cancelAnimationFrame(frame),
  };
}
