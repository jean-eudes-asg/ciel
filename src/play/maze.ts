// The maze on the Games page: a new one each time, a little bigger at each
// level, in the sky of the cloud you race (cast.ts: two levels each, from
// Cirrus's morning to Nimbus's storm). You are a spark of light; he takes the
// shortest way home at his own pace (or waits there, alone mode). Three stars
// hide in the dead ends, and at night you only see around you. Arrow keys,
// WASD or ZQSD. The sky draws at 20 frames a second and the walls once per
// maze; a frame is two copies, the glows and the rival.

import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { Bridge } from "../core/bridge";
import { Sound } from "../core/sound";
import { BotEngine, hexToRGB } from "../cloud/engine";
import { button, segmented } from "../hub/ui";
import { SkyRenderer } from "../sky/render";
import { lightningAt } from "../sky/draw";
import { css, mix, hex } from "../sky/palette";
import { lightOf, mazeRival, skySpec, type Rival } from "./cast";
import { Sfx } from "./sfx";

const N = 1, E = 2, S = 4, W = 8;
const STEP: Record<number, [number, number]> = { [N]: [0, -1], [E]: [1, 0], [S]: [0, 1], [W]: [-1, 0] };
const OPPOSITE: Record<number, number> = { [N]: S, [S]: N, [E]: W, [W]: E };
const KEYS: Record<string, number> = {
  ArrowUp: N, ArrowRight: E, ArrowDown: S, ArrowLeft: W, w: N, d: E, s: S, a: W, z: N, q: W,
};
/** Your speed, in cells a second. */
const SPEED = 7.5;
const COUNTDOWN = 3;
const STARS = 3;
const SKY_FPS = 20;
const KEY = "ciel.maze";

export interface Maze {
  cols: number;
  rows: number;
  /** Each cell's walls: N 1, E 2, S 4, W 8. */
  walls: Uint8Array;
}

/** A maze with one way between any two cells, then a few dead ends opened into loops. */
export function generate(cols: number, rows: number, random = Math.random): Maze {
  const walls = new Uint8Array(cols * rows).fill(N | E | S | W);
  const seen = new Uint8Array(cols * rows);
  const stack = [0];
  seen[0] = 1;
  const next = (cell: number) =>
    [N, E, S, W]
      .map((d) => [d, cell % cols + STEP[d][0], Math.floor(cell / cols) + STEP[d][1]] as const)
      .filter(([, x, y]) => x >= 0 && y >= 0 && x < cols && y < rows);
  while (stack.length) {
    const cell = stack[stack.length - 1];
    const open = next(cell).filter(([, x, y]) => !seen[y * cols + x]);
    if (!open.length) {
      stack.pop();
      continue;
    }
    const [d, x, y] = open[Math.floor(random() * open.length)];
    const to = y * cols + x;
    walls[cell] &= ~d;
    walls[to] &= ~OPPOSITE[d];
    seen[to] = 1;
    stack.push(to);
  }
  for (let cell = 1; cell < cols * rows - 1; cell++) {
    const closed = [N, E, S, W].filter((d) => walls[cell] & d).length;
    if (closed !== 3 || random() > 0.3) continue;
    const shut = next(cell).filter(([d]) => walls[cell] & d);
    const [d, x, y] = shut[Math.floor(random() * shut.length)];
    walls[cell] &= ~d;
    walls[y * cols + x] &= ~OPPOSITE[d];
  }
  return { cols, rows, walls };
}

/** The shortest way from one cell to another, both included. */
export function path(m: Maze, from: number, to: number): number[] {
  const back = new Int32Array(m.cols * m.rows).fill(-1);
  back[from] = from;
  const queue = [from];
  for (let k = 0; k < queue.length && back[to] < 0; k++) {
    const cell = queue[k];
    for (const d of [N, E, S, W]) {
      if (m.walls[cell] & d) continue;
      const next = cell + STEP[d][0] + STEP[d][1] * m.cols;
      if (back[next] >= 0) continue;
      back[next] = cell;
      queue.push(next);
    }
  }
  const out = [to];
  while (out[0] !== from) out.unshift(back[out[0]]);
  return out;
}

/** The cells with one way in, farthest from `from` first: where the stars hide. */
export function deadEnds(m: Maze, from: number): number[] {
  const far = (cell: number) => path(m, from, cell).length;
  const ends: number[] = [];
  for (let cell = 0; cell < m.cols * m.rows; cell++) {
    if (cell !== from && cell !== m.cols * m.rows - 1 && [N, E, S, W].filter((d) => m.walls[cell] & d).length === 3) ends.push(cell);
  }
  return ends.sort((a, b) => far(b) - far(a));
}

interface Saved {
  level: number;
  race: boolean;
  night: boolean;
  best: Record<string, number>;
}

function load(): Saved {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "") as Saved;
    if (typeof s.level === "number") return { ...s, best: s.best ?? {} };
  } catch {
    // A first maze.
  }
  return { level: 1, race: true, night: false, best: {} };
}

const clock = (s: number) => `${Math.floor(s / 60)}:${(s % 60).toFixed(1).padStart(4, "0")}`;

interface Mote {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  age: number;
  size: number;
  color: string;
}

export function mazeGame(): { el: HTMLElement; leave(): void } {
  const saved = load();
  const keep = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(saved));
    } catch {
      // Not kept.
    }
  };

  const canvas = h("canvas", { class: "mz-canvas", tabindex: 0 }) as HTMLCanvasElement;
  const levelText = h("b", {});
  const foeText = h("span", { class: "mz-foe" });
  const timeText = h("span", { class: "mz-time" });
  const starText = h("span", { class: "mz-stars" });
  const bestText = h("span", { class: "mz-best" });
  const banner = h("div", { class: "mz-banner" });
  const nightBtn = button(t("Night"), () => {
    saved.night = !saved.night;
    nightBtn.classList.toggle("on", saved.night);
    keep();
  }, "", true);
  nightBtn.classList.add("mz-toggle");
  nightBtn.classList.toggle("on", saved.night);
  const bar = h("div", { class: "mz-bar" },
    levelText, foeText, timeText, starText, bestText, h("span", { class: "spacer" }),
    segmented<"race" | "alone">([["race", t("Race")], ["alone", t("Alone")]], saved.race ? "race" : "alone", (v) => {
      saved.race = v === "race";
      keep();
      start();
    }),
    nightBtn,
    button(t("Restart"), () => start(), "", true),
  );
  const stage = h("div", { class: "mz-stage" }, canvas, banner);
  const el = h("div", { class: "mz" }, bar, stage, h("div", { class: "mz-help", text: t("Arrow keys or WASD to move.") }));

  const face = new BotEngine();
  const sky = new SkyRenderer();
  let foe: Rival = mazeRival(saved.level);
  let maze = generate(12, 7);
  let route: number[] = [];
  let stars: number[] = [];
  let skyLayer: HTMLCanvasElement | null = null;
  let walls: HTMLCanvasElement | null = null;
  let fog: HTMLCanvasElement | null = null;
  let skyAt = 0;
  let cell = 20;
  let ox = 0;
  let oy = 0;
  // You: the cell you are on or leaving, the one you go to, how far between.
  let you = { at: 0, to: 0, k: 0 };
  const motes: Mote[] = [];
  let wanted = 0;
  const held = new Set<number>();
  let clockStart = 0;
  let elapsed = 0;
  let phase: "count" | "play" | "won" | "lost" = "count";
  let lastCount = -1;
  let bumpAt = 0;
  let shake = 0;
  let thunder = -1;

  function start() {
    foe = mazeRival(saved.level);
    const cols = Math.min(36, 12 + 2 * (saved.level - 1));
    // As tall as the sky it's drawn in, so a big window is filled, not framed.
    const shape = canvas.clientWidth > 0 ? canvas.clientHeight / canvas.clientWidth : 0.56;
    const rows = Math.max(7, Math.round(cols * Math.min(1.2, Math.max(0.4, shape))));
    maze = generate(cols, rows);
    route = path(maze, 0, cols * rows - 1);
    stars = deadEnds(maze, 0).slice(0, STARS);
    you = { at: 0, to: 0, k: 0 };
    motes.length = 0;
    wanted = 0;
    phase = "count";
    lastCount = -1;
    clockStart = performance.now();
    elapsed = 0;
    walls = null;
    skyLayer = null;
    sky.reset();
    face.bodyColor = hexToRGB(foe.color);
    face.setState("idle", true);
    face.setPermanentEmote(null);
    banner.classList.remove("on");
    levelText.textContent = t("Level {0}", { 0: saved.level });
    foeText.textContent = saved.race ? t("against {0}", { 0: foe.name }) : "";
    const best = saved.best[saved.level];
    bestText.textContent = best ? t("Best {0}", { 0: clock(best) }) : "";
    Sfx.ambience(foe.sky.theme === "storm");
    canvas.focus();
  }

  const light = () => lightOf(foe);

  /** The walls as banks of cloud in the rival's sky: a shadow, a body, a lit edge. */
  function paintWalls(w: number, hgt: number, dpr: number) {
    const c = document.createElement("canvas");
    c.width = w * dpr;
    c.height = hgt * dpr;
    const x = c.getContext("2d")!;
    x.scale(dpr, dpr);
    const l = light();
    const lines = new Path2D();
    for (let i = 0; i < maze.cols * maze.rows; i++) {
      const cx = ox + (i % maze.cols) * cell;
      const cy = oy + Math.floor(i / maze.cols) * cell;
      const wl = maze.walls[i];
      if (wl & N) lines.moveTo(cx, cy), lines.lineTo(cx + cell, cy);
      if (wl & W) lines.moveTo(cx, cy), lines.lineTo(cx, cy + cell);
      if (wl & E && i % maze.cols === maze.cols - 1) lines.moveTo(cx + cell, cy), lines.lineTo(cx + cell, cy + cell);
      if (wl & S && i >= maze.cols * (maze.rows - 1)) lines.moveTo(cx, cy + cell), lines.lineTo(cx + cell, cy + cell);
    }
    const thick = Math.max(4, cell * 0.24);
    x.lineCap = "round";
    x.lineJoin = "round";
    x.save();
    x.translate(0, thick * 0.35);
    x.strokeStyle = css(l.shade, 0.75);
    x.lineWidth = thick * 1.15;
    x.filter = `blur(${Math.max(1, thick * 0.25)}px)`;
    x.stroke(lines);
    x.restore();
    x.filter = "none";
    // A soft edge, so a wall still reads over the sea of clouds.
    x.strokeStyle = css(mix(l.shade, l.zenith, 0.3), 0.55);
    x.lineWidth = thick * 1.35;
    x.stroke(lines);
    x.strokeStyle = css(mix(l.cmid, l.lit, 0.35));
    x.lineWidth = thick;
    x.stroke(lines);
    x.save();
    x.translate(0, -thick * 0.18);
    x.strokeStyle = css(mix(l.lit, hex("#ffffff"), 0.4), 0.9);
    x.lineWidth = thick * 0.45;
    x.stroke(lines);
    x.restore();
    return c;
  }

  const centre = (i: number) => [ox + (i % maze.cols + 0.5) * cell, oy + (Math.floor(i / maze.cols) + 0.5) * cell];
  const glow = (x: CanvasRenderingContext2D, px: number, py: number, r: number, stops: [number, string][]) => {
    const g = x.createRadialGradient(px, py, 0, px, py, r);
    for (const [o, c] of stops) g.addColorStop(o, c);
    x.fillStyle = g;
    x.beginPath();
    x.arc(px, py, r, 0, Math.PI * 2);
    x.fill();
  };
  const sparkle = (x: CanvasRenderingContext2D, px: number, py: number, r: number, turn: number, color: string) => {
    x.save();
    x.translate(px, py);
    x.rotate(turn);
    x.fillStyle = color;
    x.beginPath();
    for (let k = 0; k < 8; k++) {
      const rr = k % 2 ? r * 0.32 : r;
      const a = (k * Math.PI) / 4;
      x.lineTo(Math.cos(a) * rr, Math.sin(a) * rr);
    }
    x.closePath();
    x.fill();
    x.restore();
  };
  const burst = (px: number, py: number, color: string, n: number) => {
    for (let k = 0; k < n; k++) {
      const a = Math.random() * Math.PI * 2;
      const v = cell * (1 + Math.random() * 2.5);
      motes.push({ x: px, y: py, vx: Math.cos(a) * v, vy: Math.sin(a) * v, life: 0.6 + Math.random() * 0.6, age: 0, size: cell * (0.05 + Math.random() * 0.07), color });
    }
  };

  function step(dt: number, now: number) {
    if (phase === "count") {
      const left = Math.ceil(COUNTDOWN - (now - clockStart) / 1000);
      if (left !== lastCount && left > 0) {
        lastCount = left;
        Sfx.count(false);
      }
      if (left <= 0) {
        phase = "play";
        clockStart = now;
        Sfx.count(true);
      }
    }
    if (phase !== "play") return;
    elapsed = (now - clockStart) / 1000;
    // You.
    if (you.at === you.to) {
      const d = wanted || [...held].at(-1) || 0;
      if (d && !(maze.walls[you.at] & d)) {
        you.to = you.at + STEP[d][0] + STEP[d][1] * maze.cols;
        you.k = 0;
      } else if (d && wanted && now - bumpAt > 250) {
        bumpAt = now;
        shake = 1;
        Sfx.bump();
      }
      wanted = 0;
    }
    if (you.at !== you.to) {
      you.k += dt * SPEED;
      if (you.k >= 1) {
        you = { at: you.to, to: you.to, k: 0 };
        Sfx.step();
        const star = stars.indexOf(you.at);
        if (star >= 0) {
          stars.splice(star, 1);
          Sfx.star();
          const [sx, sy] = centre(you.at);
          burst(sx, sy, "#fff3c4", 18);
          drawStars();
        }
        if (you.at === maze.cols * maze.rows - 1) finish(true);
      }
    }
    // The rival, on his way.
    if (saved.race && phase === "play" && elapsed * foe.maze >= route.length - 1) finish(false);
  }

  const drawStars = () => (starText.textContent = `★ ${STARS - stars.length}/${STARS}`);

  function finish(won: boolean) {
    phase = won ? "won" : "lost";
    clear(banner);
    const [ex, ey] = centre(maze.cols * maze.rows - 1);
    if (won) {
      const best = saved.best[saved.level];
      if (!best || elapsed < best) saved.best[saved.level] = elapsed;
      face.triggerEmote(saved.race ? "surprised" : "love", 2.5);
      Sound.play(saved.race ? "annoyed" : "love", foe.pitch);
      Sfx.tune("win");
      burst(ex, ey, "#fff3c4", 40);
      const next = mazeRival(saved.level + 1);
      banner.append(
        h("b", { text: saved.race ? t("You got home first!") : t("Home!") }),
        h("span", { text: `${clock(elapsed)}  ·  ★ ${STARS - stars.length}/${STARS}` }),
      );
      if (next.id !== foe.id) banner.append(h("em", { text: t("Next, {0}: {1}", { 0: next.name, 1: next.tagline() }) }));
      banner.append(button(t("Next level"), () => start(), "primary", true));
      saved.level++;
    } else {
      face.triggerEmote("proud", 2.5);
      Sound.play("proud", foe.pitch);
      Sfx.tune("lose");
      banner.append(h("b", { text: t("{0} got home first.", { 0: foe.name }) }), button(t("Again"), () => start(), "primary", true));
    }
    keep();
    banner.classList.add("on");
  }

  function draw(dt: number, now: number) {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = canvas.clientWidth;
    const hgt = canvas.clientHeight;
    if (!w || !hgt) return;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(hgt * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(hgt * dpr);
      walls = null;
      skyLayer = null;
      sky.reset();
    }
    if (!walls) {
      cell = Math.floor(Math.min((w - 28) / maze.cols, (hgt - 28) / maze.rows));
      ox = Math.round((w - cell * maze.cols) / 2);
      oy = Math.round((hgt - cell * maze.rows) / 2);
      walls = paintWalls(w, hgt, dpr);
      drawStars();
    }
    // The sky, alive: its clouds drift, its stars twinkle, its storm flashes.
    if (!skyLayer || now - skyAt > 1000 / SKY_FPS) {
      skyAt = now;
      skyLayer ??= document.createElement("canvas");
      skyLayer.width = canvas.width;
      skyLayer.height = canvas.height;
      const sx = skyLayer.getContext("2d")!;
      const t = now / 1000 + 36_000;
      sky.render(sx, skySpec(foe), t);
      if (foe.sky.theme === "storm") {
        const flash = lightningAt(skySpec(foe).seed, t);
        if (flash.k > 0.5 && flash.which !== thunder) {
          thunder = flash.which;
          Sfx.thunder(0.6 + Math.random() * 1.2);
        }
      }
    }
    step(dt, now);
    const x = canvas.getContext("2d")!;
    x.setTransform(1, 0, 0, 1, 0, 0);
    x.drawImage(skyLayer, 0, 0);
    const l = light();
    shake = Math.max(0, shake - dt * 6);
    const jolt = shake * cell * 0.06 * Math.sin(now / 18);
    x.setTransform(dpr, 0, 0, dpr, jolt * dpr, 0);
    // A veil over the sky inside the maze, so its paths read.
    x.fillStyle = css(l.zenith, 0.3);
    x.fillRect(ox, oy, cell * maze.cols, cell * maze.rows);
    x.setTransform(1, 0, 0, 1, jolt * dpr, 0);
    x.drawImage(walls, 0, 0);
    x.setTransform(dpr, 0, 0, dpr, jolt * dpr, 0);
    const pulse = 0.5 + 0.5 * Math.sin(now / 420);

    // Where you are.
    const [ax, ay] = centre(you.at);
    const [bx, by] = centre(you.to);
    const px = ax + (bx - ax) * you.k;
    const py = ay + (by - ay) * you.k;
    if (you.at !== you.to && Math.random() < 0.6) {
      motes.push({ x: px, y: py, vx: (Math.random() - 0.5) * cell * 0.6, vy: (Math.random() - 0.5) * cell * 0.6, life: 0.7, age: 0, size: cell * 0.07, color: "#fff3c4" });
    }

    // The rival: on his way, or waiting at home.
    const end = maze.cols * maze.rows - 1;
    let cx: number, cy: number;
    if (saved.race && phase !== "count") {
      const along = Math.min(route.length - 1, elapsed * foe.maze);
      const k = Math.floor(along);
      const [x1, y1] = centre(route[k]);
      const [x2, y2] = centre(route[Math.min(route.length - 1, k + 1)]);
      cx = x1 + (x2 - x1) * (along - k);
      cy = y1 + (y2 - y1) * (along - k);
      if (phase === "play" && Math.random() < 0.25) {
        motes.push({ x: cx, y: cy + cell * 0.2, vx: (Math.random() - 0.5) * cell * 0.4, vy: cell * 0.2, life: 0.9, age: 0, size: cell * 0.09, color: `${foe.color}b3` });
      }
    } else {
      [cx, cy] = centre(saved.race ? 0 : end);
    }
    face.lookX = Math.max(-1, Math.min(1, (px - cx) / (cell * 4)));
    face.lookY = Math.max(-1, Math.min(1, (cy - py) / (cell * 4)));
    face.update(dt);

    // The stars left to find.
    for (const s of stars) {
      const [sx, sy] = centre(s);
      glow(x, sx, sy, cell * 0.55, [[0, "rgba(255,243,196,0.7)"], [1, "rgba(255,243,196,0)"]]);
      sparkle(x, sx, sy, cell * (0.26 + 0.04 * pulse), now / 1400, "#fff6d6");
    }

    if (saved.night) {
      fog ??= document.createElement("canvas");
      if (fog.width !== canvas.width || fog.height !== canvas.height) {
        fog.width = canvas.width;
        fog.height = canvas.height;
      }
      const f = fog.getContext("2d")!;
      f.setTransform(dpr, 0, 0, dpr, 0, 0);
      f.globalCompositeOperation = "source-over";
      f.fillStyle = css(mix(l.zenith, hex("#04050a"), 0.75), 0.96);
      f.fillRect(0, 0, w, hgt);
      f.globalCompositeOperation = "destination-out";
      glow(f, px, py, cell * 3.4, [[0, "rgba(0,0,0,1)"], [0.55, "rgba(0,0,0,0.9)"], [1, "rgba(0,0,0,0)"]]);
      x.setTransform(1, 0, 0, 1, 0, 0);
      x.drawImage(fog, 0, 0);
      x.setTransform(dpr, 0, 0, dpr, jolt * dpr, 0);
    }

    // Home: a little sun (a moon in the night sky), seen even at night.
    const [mx, my] = centre(end);
    const night = foe.sky.theme === "night";
    glow(x, mx, my, cell * (1.15 + 0.15 * pulse), [[0, night ? "rgba(225,232,255,0.9)" : "rgba(255,246,214,0.95)"], [0.3, night ? "rgba(200,212,255,0.45)" : css(l.sunColor, 0.5)], [1, "rgba(255,240,200,0)"]]);
    if (!night) {
      x.strokeStyle = "#f2a33c";
      x.lineWidth = Math.max(1.5, cell * 0.05);
      x.lineCap = "round";
      for (let k = 0; k < 8; k++) {
        const a = now / 3000 + (k * Math.PI) / 4;
        x.beginPath();
        x.moveTo(mx + Math.cos(a) * cell * 0.36, my + Math.sin(a) * cell * 0.36);
        x.lineTo(mx + Math.cos(a) * cell * 0.5, my + Math.sin(a) * cell * 0.5);
        x.stroke();
      }
    }
    x.fillStyle = night ? "#e9eeff" : "#ffcf6b";
    x.beginPath();
    x.arc(mx, my, cell * 0.24, 0, Math.PI * 2);
    x.fill();

    // The motes: your trail, his puffs, the bursts.
    for (let k = motes.length - 1; k >= 0; k--) {
      const m = motes[k];
      m.age += dt;
      if (m.age >= m.life) {
        motes.splice(k, 1);
        continue;
      }
      m.x += m.vx * dt;
      m.y += m.vy * dt;
      m.vx *= 0.94;
      m.vy *= 0.94;
      x.globalAlpha = 1 - m.age / m.life;
      x.fillStyle = m.color;
      x.beginPath();
      x.arc(m.x, m.y, m.size, 0, Math.PI * 2);
      x.fill();
    }
    x.globalAlpha = 1;

    // The rival, over the dark when he is near enough to be seen.
    const seen = !saved.night || Math.hypot(cx - px, cy - py) < cell * 3 || phase === "won" || phase === "lost";
    if (seen) {
      const box = cell * 1.3;
      x.save();
      x.translate(cx - box / 2, cy - box * 0.58 + Math.sin(now / 300) * cell * 0.03);
      face.draw(x, box, box);
      x.restore();
    }

    // You: a spark of light.
    glow(x, px, py, cell * 0.8, [[0, "rgba(255,255,245,1)"], [0.2, css(mix(l.sunColor, hex("#fff3c4"), 0.6), 0.9)], [0.55, css(l.sunColor, 0.3)], [1, "rgba(255,200,120,0)"]]);
    sparkle(x, px, py, cell * 0.3, now / 700, "#ffffff");

    // The countdown.
    if (phase === "count") {
      const left = Math.max(1, Math.ceil(COUNTDOWN - (now - clockStart) / 1000));
      const k = ((now - clockStart) / 1000) % 1;
      x.save();
      x.globalAlpha = 1 - k * 0.6;
      x.fillStyle = "#ffffff";
      x.shadowColor = "rgba(0,0,0,0.35)";
      x.shadowBlur = 18;
      x.font = `800 ${Math.round(Math.min(w, hgt) * (0.2 + 0.05 * (1 - k)))}px ${getComputedStyle(el).fontFamily}`;
      x.textAlign = "center";
      x.textBaseline = "middle";
      x.fillText(String(left), w / 2, hgt / 2);
      x.restore();
    }
    timeText.textContent = clock(elapsed);
  }

  let frame = 0;
  let last = performance.now();
  const loop = (now: number) => {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    if (el.isConnected && document.visibilityState === "visible") draw(dt, now);
    frame = requestAnimationFrame(loop);
  };

  const keydown = (e: KeyboardEvent) => {
    if (!el.isConnected) return;
    const d = KEYS[e.key] ?? KEYS[e.key.toLowerCase()];
    if (d) {
      e.preventDefault();
      held.delete(d);
      held.add(d);
      if (!e.repeat) wanted = d;
    } else if ((e.key === " " || e.key === "Enter") && (phase === "won" || phase === "lost")) {
      e.preventDefault();
      start();
    }
  };
  const keyup = (e: KeyboardEvent) => {
    const d = KEYS[e.key] ?? KEYS[e.key.toLowerCase()];
    if (d) held.delete(d);
  };
  const blur = () => held.clear();
  window.addEventListener("keydown", keydown);
  window.addEventListener("keyup", keyup);
  window.addEventListener("blur", blur);

  void Sound.preload(Bridge);
  start();
  frame = requestAnimationFrame(loop);

  return {
    el,
    leave: () => {
      cancelAnimationFrame(frame);
      Sfx.quiet();
      window.removeEventListener("keydown", keydown);
      window.removeEventListener("keyup", keyup);
      window.removeEventListener("blur", blur);
    },
  };
}
