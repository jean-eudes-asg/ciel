// Clouds on the Games page (clouds.ts): slide the clouds along their length
// until Ciel can float right, out to the sun. In the sky of the level's
// rival, like the maze, so the day goes by as you climb. Drag a cloud; a
// hint lights the next cloud of the shortest way. The puzzles are made in
// worker.ts.

import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { Bridge } from "../core/bridge";
import { Sound } from "../core/sound";
import { BotEngine } from "../cloud/engine";
import { button } from "../hub/ui";
import { SkyRenderer } from "../sky/render";
import { css, hex, mix } from "../sky/palette";
import { lightOf, mazeRival, skySpec } from "./cast";
import { EXIT_ROW, SIZE, room, solve, type Cloud, type Puzzle } from "./clouds";
import { Sfx } from "./sfx";

const KEY = "ciel.clouds";
const SKY_FPS = 20;

interface Saved {
  level: number;
  /** Fewest moves you took, by level. */
  best: Record<string, number>;
}

function load(): Saved {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "") as Saved;
    if (typeof s.level === "number") return { level: s.level, best: s.best ?? {} };
  } catch {
    // A first sky.
  }
  return { level: 1, best: {} };
}

export function cloudsGame(): { el: HTMLElement; leave(): void } {
  const saved = load();
  const keep = () => {
    try {
      localStorage.setItem(KEY, JSON.stringify(saved));
    } catch {
      // Not kept.
    }
  };

  const canvas = h("canvas", { class: "cl-canvas" }) as HTMLCanvasElement;
  const levelText = h("b", {});
  const movesText = h("span", { class: "mz-time" });
  const parText = h("span", { class: "mz-best" });
  const banner = h("div", { class: "mz-banner" });
  const undoBtn = button(t("Take back"), () => undo(), "", true);
  const bar = h("div", { class: "mz-bar" },
    levelText, movesText, parText, h("span", { class: "spacer" }),
    button(t("Hint"), () => hint(), "", true),
    undoBtn,
    button(t("Restart"), () => restart(), "", true),
  );
  const el = h("div", { class: "mz" }, bar, h("div", { class: "mz-stage cl-stage" }, canvas, banner),
    h("div", { class: "mz-help", text: t("Slide the clouds along their length so Ciel can reach the sun.") }));

  const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  const sky = new SkyRenderer();
  const ciel = new BotEngine();
  let puzzle: Puzzle | null = null;
  let clouds: Cloud[] = [];
  /** Where each cloud is drawn, gliding to where it is. */
  let shown: number[] = [];
  let history: Cloud[][] = [];
  let moves = 0;
  let solved = false;
  let hinted = -1;
  let asked = 0;
  let skyLayer: HTMLCanvasElement | null = null;
  let skyAt = 0;
  let geo = { x: 0, y: 0, cell: 40 };

  const along = (cl: Cloud) => (cl.v ? cl.r : cl.c);
  const foe = () => mazeRival(saved.level);

  function begin() {
    puzzle = null;
    solved = false;
    banner.classList.remove("on");
    ciel.setState("thinking");
    levelText.textContent = t("Level {0}", { 0: saved.level });
    movesText.textContent = "";
    parText.textContent = "";
    skyLayer = null;
    sky.reset();
    worker.postMessage({ game: "clouds", id: ++asked, level: saved.level });
  }

  worker.onmessage = (e: MessageEvent<{ id: number; puzzle: Puzzle }>) => {
    if (e.data.id !== asked) return;
    puzzle = e.data.puzzle;
    restart();
  };

  function restart() {
    if (!puzzle) return;
    clouds = puzzle.clouds.map((c) => ({ ...c }));
    shown = clouds.map(along);
    history = [];
    moves = 0;
    hinted = -1;
    solved = false;
    banner.classList.remove("on");
    ciel.setState("idle", true);
    counts();
  }

  function counts() {
    movesText.textContent = t("Moves: {0}", { 0: moves });
    parText.textContent = puzzle ? t("Fewest: {0}", { 0: puzzle.moves }) : "";
    undoBtn.toggleAttribute("disabled", !history.length || solved);
  }

  function moved(i: number, to: number) {
    history.push(clouds.map((c) => ({ ...c })));
    clouds[i] = clouds[i].v ? { ...clouds[i], r: to } : { ...clouds[i], c: to };
    moves++;
    hinted = -1;
    Sfx.step();
    counts();
    if (i === 0 && clouds[0].c === SIZE - 2) win();
  }

  function undo() {
    const prev = history.pop();
    if (!prev || solved) return;
    clouds = prev;
    moves = Math.max(0, moves - 1);
    Sfx.step();
    counts();
  }

  function hint() {
    if (solved || !clouds.length) return;
    const way = solve(clouds);
    if (!way || way.length < 2) return;
    hinted = way[1].findIndex((c, i) => along(c) !== along(clouds[i]));
    Sfx.pick();
  }

  function win() {
    solved = true;
    ciel.triggerEmote("love", 2.5);
    Sound.play("love");
    Sfx.tune("win");
    const best = saved.best[saved.level];
    if (!best || moves < best) saved.best[saved.level] = moves;
    clear(banner);
    banner.append(
      h("b", { text: moves <= (puzzle?.moves ?? 0) ? t("Perfect!") : t("Ciel is out!") }),
      h("span", { text: `${t("Moves: {0}", { 0: moves })}  ·  ${t("Fewest: {0}", { 0: puzzle?.moves ?? 0 })}` }),
      button(t("Next level"), () => {
        saved.level++;
        keep();
        begin();
      }, "primary", true),
    );
    keep();
    window.setTimeout(() => banner.classList.add("on"), 700);
    counts();
  }

  // ── Dragging a cloud ──
  const cellAt = (x: number, y: number) => {
    const r = canvas.getBoundingClientRect();
    return [(x - r.left - geo.x) / geo.cell, (y - r.top - geo.y) / geo.cell];
  };
  let drag: { i: number; start: number; grab: number; lo: number; hi: number } | null = null;
  canvas.addEventListener("pointerdown", (e) => {
    if (solved || !puzzle) return;
    const [c, r] = cellAt(e.clientX, e.clientY);
    const i = clouds.findIndex((cl) => {
      const w = cl.v ? 1 : cl.len;
      const hh = cl.v ? cl.len : 1;
      return c >= cl.c && c < cl.c + w && r >= cl.r && r < cl.r + hh;
    });
    if (i < 0) return;
    canvas.setPointerCapture(e.pointerId);
    const [lo, hi] = room(clouds, clouds.map(along), i);
    drag = { i, start: along(clouds[i]), grab: clouds[i].v ? r : c, lo, hi };
    Sfx.pick();
  });
  canvas.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const [c, r] = cellAt(e.clientX, e.clientY);
    const at = drag.start + ((clouds[drag.i].v ? r : c) - drag.grab);
    shown[drag.i] = Math.max(drag.lo, Math.min(drag.hi, at));
  });
  const drop = () => {
    if (!drag) return;
    const d = drag;
    drag = null;
    const to = Math.round(shown[d.i]);
    if (to !== d.start) moved(d.i, to);
  };
  canvas.addEventListener("pointerup", drop);
  canvas.addEventListener("pointercancel", drop);

  // ── Drawing ──
  /** A cloud: a soft pill with puffs along it, lit from above. */
  function cloudShape(x: CanvasRenderingContext2D, px: number, py: number, w: number, hh: number, lit: string, mid: string, shade: string, glow: boolean) {
    const pad = geo.cell * 0.08;
    const X = px + pad, Y = py + pad, W = w - pad * 2, H = hh - pad * 2;
    const puffs: [number, number, number][] = [];
    const long = Math.max(W, H);
    const n = Math.max(2, Math.round(long / (geo.cell * 0.55)));
    for (let k = 0; k < n; k++) {
      const f = (k + 0.5) / n;
      const rr = Math.min(W, H) * (0.36 + 0.08 * Math.sin(k * 2.1));
      puffs.push(W >= H ? [X + W * f, Y + H * 0.42, rr] : [X + W * (k % 2 ? 0.62 : 0.38), Y + H * f, rr * 1.12]);
    }
    const body = new Path2D();
    const rad = Math.min(W, H) * 0.42;
    body.roundRect(X, Y + (W >= H ? H * 0.25 : 0), W, W >= H ? H * 0.75 : H, rad);
    for (const [cx, cy, rr] of puffs) {
      body.moveTo(cx + rr, cy);
      body.arc(cx, cy, rr, 0, Math.PI * 2);
    }
    x.save();
    x.shadowColor = glow ? "rgba(255,214,120,0.9)" : "rgba(0,0,0,0.28)";
    x.shadowBlur = glow ? geo.cell * 0.5 : geo.cell * 0.18;
    x.shadowOffsetY = glow ? 0 : geo.cell * 0.08;
    const g = x.createLinearGradient(0, Y, 0, Y + H);
    g.addColorStop(0, lit);
    g.addColorStop(0.6, mid);
    g.addColorStop(1, shade);
    x.fillStyle = g;
    x.fill(body, "nonzero");
    x.restore();
  }

  function draw(dt: number, now: number) {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = canvas.clientWidth;
    const hgt = canvas.clientHeight;
    if (!w || !hgt) return;
    if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(hgt * dpr)) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(hgt * dpr);
      skyLayer = null;
      sky.reset();
    }
    const cell = Math.floor(Math.min((w - 140) / SIZE, (hgt - 40) / SIZE));
    geo = { x: Math.round((w - cell * SIZE) / 2) - cell * 0.4, y: Math.round((hgt - cell * SIZE) / 2), cell };
    if (!skyLayer || now - skyAt > 1000 / SKY_FPS) {
      skyAt = now;
      skyLayer ??= document.createElement("canvas");
      skyLayer.width = canvas.width;
      skyLayer.height = canvas.height;
      sky.render(skyLayer.getContext("2d")!, skySpec(foe()), now / 1000 + 36_000);
    }
    const x = canvas.getContext("2d")!;
    x.setTransform(1, 0, 0, 1, 0, 0);
    x.drawImage(skyLayer, 0, 0);
    x.setTransform(dpr, 0, 0, dpr, 0, 0);
    const l = lightOf(foe());
    const { x: bx, y: by } = geo;

    // The patch of sky: a pane, its grid of faint points, the opening to the sun.
    x.fillStyle = css(l.zenith, 0.38);
    x.strokeStyle = css(mix(l.lit, hex("#ffffff"), 0.4), 0.5);
    x.lineWidth = 2;
    x.beginPath();
    x.roundRect(bx - 8, by - 8, cell * SIZE + 16, cell * SIZE + 16, 16);
    x.fill();
    x.stroke();
    x.fillStyle = css(l.lit, 0.35);
    for (let r = 0; r < SIZE; r++) {
      for (let c = 0; c < SIZE; c++) {
        x.beginPath();
        x.arc(bx + (c + 0.5) * cell, by + (r + 0.5) * cell, 2, 0, Math.PI * 2);
        x.fill();
      }
    }
    const sunX = bx + cell * SIZE + cell * 0.9;
    const sunY = by + (EXIT_ROW + 0.5) * cell;
    x.fillStyle = css(l.zenith, 0.38);
    x.fillRect(bx + cell * SIZE, sunY - cell * 0.45, 10, cell * 0.9);
    const pulse = 0.5 + 0.5 * Math.sin(now / 500);
    const halo = x.createRadialGradient(sunX, sunY, 0, sunX, sunY, cell * (0.9 + 0.1 * pulse));
    halo.addColorStop(0, "rgba(255,240,200,0.95)");
    halo.addColorStop(0.35, "rgba(255,200,110,0.55)");
    halo.addColorStop(1, "rgba(255,200,110,0)");
    x.fillStyle = halo;
    x.beginPath();
    x.arc(sunX, sunY, cell, 0, Math.PI * 2);
    x.fill();
    x.fillStyle = "#ffcf6b";
    x.beginPath();
    x.arc(sunX, sunY, cell * 0.28, 0, Math.PI * 2);
    x.fill();

    if (!puzzle) {
      ciel.update(dt);
      x.save();
      x.translate(bx + cell * SIZE / 2 - cell, by + cell * SIZE / 2 - cell);
      ciel.draw(x, cell * 2, cell * 2);
      x.restore();
      return;
    }

    // The clouds glide to where they are; Ciel floats out once free.
    clouds.forEach((cl, i) => {
      if (drag?.i === i) return;
      const target = along(cl) + (i === 0 && solved ? 3 : 0);
      shown[i] += (target - shown[i]) * Math.min(1, dt * (i === 0 && solved ? 3 : 14));
    });
    const lit = css(mix(l.lit, hex("#ffffff"), 0.5));
    const mid = css(mix(l.cmid, l.lit, 0.4));
    const shade = css(mix(l.shade, l.cmid, 0.35));
    clouds.forEach((cl, i) => {
      if (i === 0) return;
      const px = bx + (cl.v ? cl.c : shown[i]) * cell;
      const py = by + (cl.v ? shown[i] : cl.r) * cell;
      cloudShape(x, px, py, (cl.v ? 1 : cl.len) * cell, (cl.v ? cl.len : 1) * cell, lit, mid, shade, i === hinted && Math.sin(now / 160) > -0.3);
    });
    // Ciel, in his two cells, on a warm glow that shows how wide he is.
    const cx = bx + shown[0] * cell;
    const cy = by + EXIT_ROW * cell;
    if (!solved) {
      x.save();
      x.shadowColor = "rgba(255,200,110,0.8)";
      x.shadowBlur = cell * 0.4;
      x.fillStyle = "rgba(255,226,160,0.32)";
      x.beginPath();
      x.roundRect(cx + cell * 0.1, cy + cell * 0.14, cell * 1.8, cell * 0.72, cell * 0.36);
      x.fill();
      x.restore();
    }
    ciel.lookX = solved ? 1 : Math.max(-1, Math.min(1, (sunX - cx) / (cell * 4)));
    ciel.lookY = 0;
    ciel.update(dt);
    x.save();
    x.globalAlpha = solved ? Math.max(0, 1 - (shown[0] - (SIZE - 2)) / 3) : 1;
    const box = cell * 1.5;
    x.translate(cx + cell - box / 2, cy + cell / 2 - box * 0.58 + Math.sin(now / 400) * 2);
    ciel.draw(x, box, box);
    x.restore();
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
    leave: () => {
      cancelAnimationFrame(frame);
      worker.terminate();
    },
  };
}
