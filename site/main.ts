// Ciel's website. The hero is the visitor's own sky, right now (the real
// sun for their time zone); Ciel floats in it, follows the pointer and
// minds being poked. Scrolling down, he stays in view while each feature
// sets his mood and the sky, which crossfades from scene to scene, until he
// flies off home to the top of the screen. Everything is drawn by the app's
// own code: the sky by src/sky (in a worker), Ciel by src/cloud.
//
// ?time=HH:MM shows the sky at that time of day (for checking the page).

import { BotEngine } from "../src/cloud/engine";
import type { BotEmoteName, BotStateName } from "../src/core/layout";
import { CielPainter } from "../src/sky/ciel";
import { frameFor, type Frame } from "../src/sky/draw";
import { css, lightFor, luma, mix, type Light, type RGB } from "../src/sky/palette";
import { DEFAULT_WALLPAPER, type Phase, type Theme } from "../src/sky/prefs";
import { chooseScene, realSky, sampleSky, sceneSpec, secondsOfDay, type CielPlacement, type SceneSpec } from "../src/sky/scenes";
import { placeFor } from "../src/sky/sun";
import type { StageEvent, StageMessage, StageView } from "./sky-stage";

const $ = <T extends Element = HTMLElement>(sel: string) => document.querySelector(sel) as T;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const smooth = (e0: number, e1: number, x: number) => {
  const t = clamp((x - e0) / (e1 - e0), 0, 1);
  return t * t * (3 - 2 * t);
};
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

const params = new URLSearchParams(location.search);
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

// ── Time and place ────────────────────────────────────────────────────────────

/** ?time=HH:MM moves the clock to that time today (and lets it run on). */
const clockOffset = (() => {
  const m = /^(\d{1,2}):(\d{2})$/.exec(params.get("time") ?? "");
  if (!m) return 0;
  const d = new Date();
  d.setHours(clamp(Number(m[1]), 0, 23), clamp(Number(m[2]), 0, 59), 0, 0);
  return d.getTime() - Date.now();
})();
const pageNow = () => new Date(Date.now() + clockOffset);

// ── The scenes ────────────────────────────────────────────────────────────────

type SceneName = "real" | "clear" | "golden" | "cloudy" | "sunset" | "night" | "space";
const ORDER: SceneName[] = ["real", "clear", "golden", "cloudy", "sunset", "night", "space"];

/** The journey's skies: a believable sky for each mood, whatever the time. */
const JOURNEY: Record<Exclude<SceneName, "real">, [Theme, Phase]> = {
  clear: ["clear", "day"],
  golden: ["golden", "golden"],
  cloudy: ["cloudy", "day"],
  sunset: ["sunset", "sunset"],
  night: ["night", "night"],
  space: ["space", "night"],
};

interface Sky {
  specs: Record<SceneName, SceneSpec>;
  phase: Phase;
  sunElevation: number;
  moon: { up: boolean; phase: number };
  zone: string;
}

function computeSky(now: Date): Sky {
  const place = placeFor(null, null, now);
  const sky = realSky(now, place);
  const choice = chooseScene(now, "idle", DEFAULT_WALLPAPER, place);
  const specs = {
    real: sceneSpec(now, "idle", DEFAULT_WALLPAPER, { place, choice, sky, noCiel: true }),
  } as Record<SceneName, SceneSpec>;
  for (const [name, [theme, phase]] of Object.entries(JOURNEY) as [Exclude<SceneName, "real">, [Theme, Phase]][]) {
    const sample = sampleSky(phase);
    // Tonight's moon, in the journey's night.
    sample.moon = { ...sample.moon, phase: sky.moon.phase };
    specs[name] = sceneSpec(now, "idle", DEFAULT_WALLPAPER, { place, choice: { theme, phase }, sky: sample, noCiel: true });
  }
  return {
    specs,
    phase: choice.phase,
    sunElevation: sky.sun.elevation,
    moon: { up: sky.moon.elevation > 2, phase: sky.moon.phase },
    zone: place.zone,
  };
}

const lightOf = (spec: SceneSpec): Light => lightFor(spec.theme, spec.sun.elevation, spec.afterglow);

let sky = computeSky(pageNow());

// ── Words on the sky ──────────────────────────────────────────────────────────

const PHASE_WORDS: Record<Phase, string> = {
  night: "night",
  dawn: "dawn",
  day: "broad daylight",
  golden: "golden hour",
  sunset: "sunset",
  dusk: "dusk",
};

function moonWords(phase: number): string {
  const names = [
    "a new moon", "a waxing crescent", "a first-quarter moon", "a waxing gibbous moon",
    "a full moon", "a waning gibbous moon", "a last-quarter moon", "a waning crescent",
  ];
  return names[Math.round(phase * 8) % 8];
}

/** How bright the sky is behind the words (the lower half, where they sit). */
const brightness = (l: Light) => luma(mix(mix(l.mid, l.horizon, 0.55), l.cmid, 0.25));

/** Text and buttons on the sky: dark ink on a bright sky, white on a dark one. */
function setTone(bright: number) {
  const root = document.documentElement;
  const tone = root.dataset.tone === "dark" ? (bright > 146 ? "light" : "dark") : bright < 130 ? "dark" : "light";
  if (root.dataset.tone !== tone) root.dataset.tone = tone;
}

function applyTone() {
  const spec = sky.specs.real;
  const l = lightOf(spec);
  document.documentElement.dataset.tone = brightness(l) > 138 ? "light" : "dark";
  // Until the first frame: the same sky as a gradient.
  $(".stage").style.setProperty(
    "--stage-bg",
    `linear-gradient(180deg, ${css(l.zenith)} 0%, ${css(l.upper)} 30%, ${css(l.mid)} 60%, ${css(l.horizon)} 80%, ${css(l.cmid)} 100%)`,
  );
  $<HTMLMetaElement>('meta[name="theme-color"]').content = css(l.zenith);

  let words = PHASE_WORDS[sky.phase];
  if (sky.phase === "night" && sky.moon.up) words = `night, under ${moonWords(sky.moon.phase)}`;
  $(".sky-phase").textContent = words;
  const e = Math.round(sky.sunElevation);
  const where = sky.zone ? ` (${sky.zone.replace(/_/g, " ")})` : "";
  $(".sky-detail").textContent =
    `The sun is ${Math.abs(e)}° ${e >= 0 ? "above" : "below"} the horizon, worked out in your browser from your time zone${where}.`;
  const glyph: RGB = sky.phase === "night" || sky.phase === "dusk" ? [226, 232, 255] : mix(l.sunColor, [255, 196, 70], 0.7);
  $(".sky-glyph").style.setProperty("--glyph", css(glyph));
}

applyTone();

// ── The sky stage (a worker when it can) ──────────────────────────────────────

const stageEl = $(".stage");
let skyCanvas = $<HTMLCanvasElement>(".stage .sky");
const cielCanvas = $<HTMLCanvasElement>(".ciel-layer");
const journey = $(".journey");

let sendQueue: StageMessage[] | null = [];
let send: (msg: StageMessage) => void = (msg) => sendQueue?.push(msg);

function stageCss(): { w: number; h: number } {
  return { w: Math.max(1, stageEl.clientWidth), h: Math.max(1, stageEl.clientHeight) };
}

/** The sky's resolution: soft gradients don't need every pixel, phones even less. */
function skyPixels(w: number, h: number): { width: number; height: number } {
  const small = matchMedia("(pointer: coarse)").matches || w < 760;
  const budget = small ? 0.5e6 : 1.3e6;
  const k = Math.min(window.devicePixelRatio || 1, 1.25, Math.sqrt(budget / (w * h)));
  return { width: Math.round(w * k), height: Math.round(h * k) };
}

let firstFrame = false;
function onStageEvent(e: StageEvent) {
  if (e.type !== "drawn" || firstFrame) return;
  firstFrame = true;
  skyCanvas.classList.add("on");
  cielCanvas.classList.add("on");
  $(".sky-note").classList.add("on");
  if (!still) setTimeout(() => engine.greet(), 500);
}

function startStage(view: StageView) {
  const { w, h } = stageCss();
  const init: Extract<StageMessage, { type: "init" }> = {
    type: "init",
    ...skyPixels(w, h),
    scenes: { ...sky.specs },
    order: ORDER,
    view,
    still,
    t: secondsOfDay(pageNow()),
    prebuild: true,
  };
  const flush = (to: (m: StageMessage) => void) => {
    send = to;
    const queued = sendQueue ?? [];
    sendQueue = null;
    for (const m of queued) to(m);
  };
  const onMainThread = async (canvas: HTMLCanvasElement) => {
    const { SkyStage } = await import("./sky-stage");
    // Here every build would hold up the page: each scene is built when first shown.
    const now = stageCss();
    const stage = new SkyStage(canvas, { ...init, ...skyPixels(now.w, now.h), view: sentView, prebuild: false }, onStageEvent);
    flush((m) => stage.handle(m));
    stage.handle({ type: "run", running });
  };
  if ("transferControlToOffscreen" in skyCanvas && typeof Worker === "function") {
    try {
      const worker = new Worker(new URL("./sky-worker.ts", import.meta.url), { type: "module" });
      const offscreen = skyCanvas.transferControlToOffscreen();
      worker.onmessage = (e: MessageEvent<StageEvent>) => onStageEvent(e.data);
      worker.onerror = () => {
        // The worker didn't start: a fresh canvas, drawn on the page.
        if (firstFrame) return;
        worker.terminate();
        const fresh = skyCanvas.cloneNode() as HTMLCanvasElement;
        skyCanvas.replaceWith(fresh);
        skyCanvas = fresh;
        sendQueue = [];
        send = (msg) => sendQueue?.push(msg);
        void onMainThread(fresh);
      };
      worker.postMessage({ ...init, canvas: offscreen }, [offscreen]);
      flush((m) => worker.postMessage(m));
      send({ type: "run", running });
      return;
    } catch {
      /* no worker: below */
    }
  }
  void onMainThread(skyCanvas);
}

// ── Ciel ──────────────────────────────────────────────────────────────────────

/** The engine, with a slow breath at rest (as in the wallpaper) scaled from his base. */
class SiteCiel extends BotEngine {
  breath = 0;

  override draw(x: CanvasRenderingContext2D, W: number, H: number) {
    if (Math.abs(this.breath) < 1e-4) {
      super.draw(x, W, H);
      return;
    }
    const R = W * 0.3;
    const base = H / 2 + this.particleOverhang / 2 + R * 0.94;
    x.save();
    x.translate(W / 2, base);
    x.scale(1 - this.breath * 0.57, 1 + this.breath);
    x.translate(-W / 2, -base);
    super.draw(x, W, H);
    x.restore();
  }
}

const engine = new SiteCiel();
engine.setState("idle", true);
const painter = new CielPainter();
const cielCtx = cielCanvas.getContext("2d")!;
const poke = $<HTMLButtonElement>(".poke");

/** Scene frames at the Ciel layer's size, for the light on him. */
let frames = {} as Record<SceneName, Frame>;

function sizeCiel() {
  const { w, h } = stageCss();
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const cielScale = Math.min(dpr, Math.sqrt(4.2e6 / (w * h)));
  cielCanvas.width = Math.round(w * cielScale);
  cielCanvas.height = Math.round(h * cielScale);
  makeFrames();
}

/** Each scene's brightness behind the words, for the tone of the text on it. */
let brights = {} as Record<SceneName, number>;

function makeFrames() {
  frames = {} as Record<SceneName, Frame>;
  for (const name of ORDER) {
    const spec = { ...sky.specs[name] };
    frames[name] = frameFor(spec, lightOf(spec), cielCanvas.width, cielCanvas.height);
    brights[name] = brightness(frames[name].light);
  }
}

function mixLight(a: Light, b: Light, k: number): Light {
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(a) as (keyof Light)[]) {
    const va = a[key];
    const vb = b[key];
    if (Array.isArray(va)) out[key] = mix(va as RGB, vb as RGB, k);
    else if (typeof va === "number") out[key] = lerp(va, vb as number, k);
    else out[key] = k < 0.5 ? va : vb;
  }
  return out as unknown as Light;
}

/** The light on Ciel between two scenes. */
function blendFrame(view: StageView, place: CielPlacement): Frame {
  const a = frames[view.a as SceneName];
  const b = frames[view.b as SceneName];
  const k = view.a === view.b ? 0 : view.k;
  const base = k < 0.5 ? a : b;
  const spec = { ...base.spec, ciel: place, mood: engine.state };
  if (k <= 0.001) return { ...a, spec };
  if (k >= 0.999) return { ...b, spec };
  return {
    ...base,
    spec,
    light: mixLight(a.light, b.light, k),
    lightAt: { x: lerp(a.lightAt.x, b.lightAt.x, k), y: lerp(a.lightAt.y, b.lightAt.y, k) },
    lightAmt: lerp(a.lightAmt, b.lightAmt, k),
  };
}

// ── The steps of the journey ──────────────────────────────────────────────────

type Kind = "hero" | "feature" | "away";

interface Step {
  el: HTMLElement;
  scene: SceneName;
  mood: BotStateName;
  kind: Kind;
  /** Its middle, in page pixels. */
  center: number;
}

const steps: Step[] = [...document.querySelectorAll<HTMLElement>(".step")].map((el) => ({
  el,
  scene: (el.dataset.scene ?? "real") as SceneName,
  mood: (el.dataset.mood ?? "idle") as BotStateName,
  kind: el.classList.contains("hero") ? "hero" : el.hasAttribute("data-away") ? "away" : "feature",
  center: 0,
}));

/** Where Ciel floats for each kind of step, as fractions of the stage. */
const places: Record<Kind, CielPlacement> = {
  hero: { x: 0.5, y: 0.36, size: 0.3 },
  feature: { x: 0.25, y: 0.5, size: 0.3 },
  away: { x: 0.5, y: -0.4, size: 0.24 },
};

let narrow = false;

function measure() {
  const scrollY = window.scrollY;
  for (const s of steps) {
    // A feature's moment is when its card is in the middle of the screen.
    const r = (s.el.querySelector(".card") ?? s.el).getBoundingClientRect();
    s.center = r.top + scrollY + r.height / 2;
  }
  const { w, h } = stageCss();
  narrow = w < 900;

  // Hero: big, in the middle of the room left above the words.
  const hero = steps[0].el;
  const copy = hero.querySelector<HTMLElement>(".hero-copy")!;
  const top = 64;
  const bottom = copy.getBoundingClientRect().top - hero.getBoundingClientRect().top - 8;
  const room = Math.max(120, bottom - top);
  const body = clamp(Math.min(room * 1.08, w * 0.62, h * 0.44), 90, 520);
  places.hero = { x: 0.5, y: (top + room * 0.54) / h, size: body / h };

  // Features: beside the cards on a wide screen, above them on a narrow one.
  if (narrow) {
    const b = Math.min(w * 0.44, h * 0.2);
    places.feature = { x: 0.5, y: 0.22, size: b / h };
  } else {
    const card = document.querySelector<HTMLElement>(".feature .card")!;
    const left = card.getBoundingClientRect().left;
    const b = Math.min(left * 0.6, h * 0.36, 440);
    places.feature = { x: left / 2 / w, y: 0.5, size: b / h };
  }
  // Flying home, up to the top of the screen where the island lives.
  places.away = { x: 0.5, y: -0.45, size: places.feature.size * 0.8 };
}

interface ScrollState {
  view: StageView;
  place: CielPlacement;
  mood: BotStateName;
  step: Step;
}

/** Where the scroll is: which scenes, how far between them, where Ciel is. */
function scrollState(): ScrollState {
  const c = window.scrollY + window.innerHeight / 2;
  let i = 0;
  while (i < steps.length - 1 && steps[i + 1].center <= c) i++;
  const a = steps[i];
  const b = steps[Math.min(steps.length - 1, i + 1)];
  const t = a === b ? 0 : clamp((c - a.center) / (b.center - a.center), 0, 1);
  const k = smooth(0.3, 0.7, t);
  const m = smooth(0.08, 0.92, t);
  const pa = places[a.kind];
  const pb = places[b.kind];
  return {
    view: { a: a.scene, b: b.scene, k: a.scene === b.scene ? 0 : k },
    place: { x: lerp(pa.x, pb.x, m), y: lerp(pa.y, pb.y, m), size: lerp(pa.size, pb.size, m) },
    mood: t < 0.5 ? a.mood : b.mood,
    step: t < 0.5 ? a : b,
  };
}

// ── Pointer, pokes and moods ──────────────────────────────────────────────────

let pointer: { x: number; y: number; at: number; touch: boolean } | null = null;
const notePointer = (e: PointerEvent) => {
  pointer = { x: e.clientX, y: e.clientY, at: performance.now(), touch: e.pointerType !== "mouse" };
};
window.addEventListener("pointermove", notePointer, { passive: true });
window.addEventListener("pointerdown", notePointer, { passive: true });
document.addEventListener("mouseout", (e) => {
  if (!e.relatedTarget) pointer = null;
});

let dizzyUntil = 0;
let mood: BotStateName = "idle";
let activeStep: Step = steps[0];
let lastPoke = 0;

poke.addEventListener("click", () => {
  lastPoke = performance.now();
  engine.slap();
});

engine.onDizzy = () => {
  dizzyUntil = performance.now() + 3300;
  engine.setState("dizzy");
};

let lastReaction = 0;
for (const el of document.querySelectorAll<HTMLElement>("[data-react]")) {
  el.addEventListener("pointerenter", () => {
    const now = performance.now();
    if (now - lastReaction < 2500 || engine.state === "dizzy" || activeStep.kind !== "hero") return;
    lastReaction = now;
    engine.triggerEmote(el.dataset.react as BotEmoteName, 1.6);
  });
}

function setMood(next: BotStateName, step: Step) {
  const prev = activeStep;
  activeStep = step;
  if (next === mood && prev.kind === step.kind) return;
  mood = next;
  if (engine.state === "dizzy") return;
  engine.setState(next);
  if (still) return;
  if (step.kind === "away" && prev.kind !== "away") engine.greet(); // waves goodbye as he flies home
  else if (step.kind === "hero" && prev.kind !== "hero") engine.triggerEmote("happy");
}

let nextIdle = performance.now() + 9000;
function idleLife(now: number) {
  if (still || activeStep.kind !== "hero" || engine.state !== "idle" || now < nextIdle) return;
  nextIdle = now + 9000 + Math.random() * 7000;
  if (now - lastPoke < 6000) return;
  const pick = Math.random();
  if (pick < 0.4) engine.triggerEmote("wink", 1.4);
  else if (pick < 0.75) engine.triggerEmote("happy", 1.6);
  else engine.greet();
}

// ── The frame loop ────────────────────────────────────────────────────────────

let running = false;
let raf = 0;
let last = performance.now();
let lastDraw = 0;
let sentView: StageView = { a: "real", b: "real", k: 0 };
/** How far the sky has spun into space while he is dizzy (0…1). */
let dizzySky = 0;
let pokeShown = true;

function look(cx: number, cy: number, body: number, now: number) {
  const fresh = pointer && (!pointer.touch || now - pointer.at < 2500);
  if (fresh && pointer) {
    engine.lookX = clamp((pointer.x - cx) / (body * 1.4), -1, 1);
    engine.lookY = clamp((pointer.y - cy) / (body * 1.1), -1, 1);
    return;
  }
  const s = now / 1000;
  const drift = still ? 0 : 1;
  if (activeStep.kind === "feature") {
    // Watching the card.
    engine.lookX = (narrow ? 0 : 0.55) + Math.sin(s * 0.4) * 0.08 * drift;
    engine.lookY = (narrow ? 0.6 : 0.12) + Math.sin(s * 0.31) * 0.06 * drift;
  } else {
    engine.lookX = (Math.sin(s * 0.33) * 0.45 + Math.sin(s * 0.13 + 1) * 0.25) * drift;
    engine.lookY = (Math.sin(s * 0.27 + 2) * 0.25 - 0.05) * drift;
  }
}

function frame(now: number) {
  raf = 0;
  if (!running) return;
  raf = requestAnimationFrame(frame);
  // 60 frames a second is plenty, even on a 120 Hz screen.
  if (now - lastDraw < 1000 / 62) return;
  lastDraw = now;
  const dt = Math.min(0.05, (now - last) / 1000);
  last = now;

  const st = scrollState();
  setMood(st.mood, st.step);

  if (dizzyUntil && now > dizzyUntil) {
    dizzyUntil = 0;
    engine.setState(mood);
    engine.triggerEmote("happy");
  }
  dizzySky = clamp(dizzySky + (dizzyUntil ? dt / 0.7 : -dt / 1.2), 0, 1);
  let view = st.view;
  if (dizzySky > 0.001) {
    const here = (view.k < 0.5 ? view.a : view.b) as SceneName;
    if (here !== "space") view = { a: here, b: "space", k: smooth(0, 1, dizzySky) };
  }
  if (view.a !== sentView.a || view.b !== sentView.b || Math.abs(view.k - sentView.k) > 0.002) {
    sentView = view;
    send({ type: "view", view });
    setTone(lerp(brights[view.a as SceneName], brights[view.b as SceneName], view.a === view.b ? 0 : view.k));
  }

  const W = cielCanvas.width;
  const H = cielCanvas.height;
  const { w: cw, h: ch } = stageCss();
  const place = st.place;
  const body = place.size * ch;
  const t = now / 1000;
  const floatY = still ? 0 : Math.sin(t * 0.55) * 0.012 * ch * Math.min(1, place.size / 0.2);
  const driftX = still ? 0 : Math.sin(t * 0.17) * 0.004 * ch;
  const cx = place.x * cw + driftX;
  const cy = place.y * ch + floatY;
  const visible = cy + body > -20 && cy - body < ch + 20;

  look(cx, cy, body, now);
  idleLife(now);
  engine.breath = still || engine.state === "sleeping" || engine.state === "dizzy" ? 0 : Math.sin(t * 1.6) * 0.012;
  engine.update(dt);

  cielCtx.setTransform(1, 0, 0, 1, 0, 0);
  cielCtx.clearRect(0, 0, W, H);
  if (visible) painter.draw(cielCtx, blendFrame(view, place), t, still, engine);

  if (visible !== pokeShown) {
    pokeShown = visible;
    poke.hidden = !visible;
  }
  if (visible) {
    const pw = body * 0.98;
    const ph = body * 0.78;
    poke.style.width = `${pw}px`;
    poke.style.height = `${ph}px`;
    poke.style.transform = `translate(${cx - pw / 2}px, ${cy - ph / 2 + body * 0.04}px)`;
  }
}

function setRunning(on: boolean) {
  if (on === running) return;
  running = on;
  send({ type: "run", running: on });
  if (on && !raf) {
    last = performance.now();
    raf = requestAnimationFrame(frame);
  }
}

let journeyVisible = true;
const updateRunning = () => setRunning(journeyVisible && document.visibilityState === "visible");
new IntersectionObserver(([e]) => {
  journeyVisible = e.isIntersecting;
  updateRunning();
}).observe(journey);
document.addEventListener("visibilitychange", updateRunning);

// ── Sizes ─────────────────────────────────────────────────────────────────────

let sized = "";
function resize() {
  const { w, h } = stageCss();
  const key = `${w}x${h}`;
  measure();
  if (key === sized) return;
  sized = key;
  sizeCiel();
  send({ type: "size", ...skyPixels(w, h) });
}

let resizeTimer = 0;
const onResize = () => {
  window.clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(resize, 150);
};
const watchSizes = new ResizeObserver(onResize);
for (const el of [stageEl, journey, $(".hero-copy")]) watchSizes.observe(el);
void document.fonts?.ready.then(() => measure());

// The real sky moves: every few minutes, the hero's sky follows the sun.
setInterval(() => {
  const next = computeSky(pageNow());
  if (JSON.stringify(next.specs) === JSON.stringify(sky.specs)) return;
  sky = next;
  applyTone();
  makeFrames();
  send({ type: "scenes", scenes: { ...sky.specs } });
}, 5 * 60_000);

// ── Screenshots ───────────────────────────────────────────────────────────────

// The screenshots in site/shots/ (built in when they exist). A missing one
// keeps its frame's little drawn island instead, and is never requested.
const shots = import.meta.glob("./shots/*.{png,jpg,jpeg,webp,avif}", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;
const shotUrl = new Map(Object.entries(shots).map(([path, url]) => [path.replace(/^.*\/|\.\w+$/g, ""), url]));

for (const img of document.querySelectorAll<HTMLImageElement>("img[data-shot]")) {
  const figure = img.closest<HTMLElement>(".shot")!;
  const url = shotUrl.get(img.dataset.shot ?? "");
  const empty = () => {
    figure.classList.add("empty");
    figure.setAttribute("role", "img");
    figure.setAttribute("aria-label", img.alt);
  };
  if (!url) {
    img.removeAttribute("src");
    empty();
    continue;
  }
  const loaded = () => img.classList.add("loaded");
  if (img.getAttribute("src") !== url) img.src = url;
  if (img.complete && img.naturalWidth > 0) loaded();
  else {
    img.addEventListener("load", loaded, { once: true });
    img.addEventListener("error", empty, { once: true });
  }
}

// ── Try it ────────────────────────────────────────────────────────────────────

const demo = $(".demo");
const demoBox = $(".demo-box");
const demoFrame = $<HTMLIFrameElement>(".demo-frame");
const DEMO_URL = import.meta.env.DEV ? "/demo.html" : "./demo.html";

new ResizeObserver(() => {
  demoBox.style.setProperty("--demo-scale", String(demoBox.clientWidth / 900));
}).observe(demoBox);

const demoWatch = new IntersectionObserver(
  ([e]) => {
    if (!e.isIntersecting) return;
    demoWatch.disconnect();
    void (async () => {
      let ok = false;
      try {
        const res = await fetch(DEMO_URL, { method: "HEAD", cache: "no-store" });
        ok = res.ok && (res.headers.get("content-type") ?? "text/html").includes("html");
      } catch {
        ok = false;
      }
      if (!ok) {
        demo.dataset.state = "missing";
        return;
      }
      demoFrame.addEventListener(
        "load",
        () => {
          demo.dataset.state = "ready";
          demoFrame.removeAttribute("tabindex");
        },
        { once: true },
      );
      demoFrame.src = DEMO_URL;
      setTimeout(() => {
        if (demo.dataset.state === "waiting") demo.dataset.state = "missing";
      }, 15000);
    })();
  },
  { rootMargin: "800px 0px" },
);
demoWatch.observe(demo);

// ── A little Ciel napping in the footer ───────────────────────────────────────

// He sleeps until the pointer comes near, watches it, and nods off again
// once it has been still for a while. Drawn only while the footer is in view.
function footerCiel(canvas: HTMLCanvasElement) {
  const napper = new BotEngine();
  napper.setState("sleeping", true);
  const ctx = canvas.getContext("2d")!;
  const S = 100;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  canvas.width = S * dpr;
  canvas.height = S * dpr;
  let awakeUntil = 0;
  let dizzyTill = 0;
  let shown = false;
  let loop = 0;
  let prev = performance.now();
  napper.onDizzy = () => {
    dizzyTill = performance.now() + 2600;
    napper.setState("dizzy");
  };
  canvas.addEventListener("click", () => {
    awakeUntil = performance.now() + 6000;
    if (napper.state === "sleeping") napper.setState("idle");
    napper.slap();
  });
  const tick = (now: number) => {
    loop = 0;
    if (!shown) return;
    loop = requestAnimationFrame(tick);
    const dt = Math.min(0.05, (now - prev) / 1000);
    prev = now;
    const r = canvas.getBoundingClientRect();
    const cx = r.left + r.width / 2;
    const cy = r.top + r.height * 0.6;
    const near = pointer && Math.hypot(pointer.x - cx, pointer.y - cy) < 260 && now - pointer.at < 400;
    if (near) {
      if (napper.state === "sleeping") {
        napper.setState("idle");
        napper.triggerEmote("surprised", 0.9);
      }
      awakeUntil = now + 4500;
    }
    if (dizzyTill && now > dizzyTill) {
      dizzyTill = 0;
      napper.setState("idle");
      napper.triggerEmote("happy");
    }
    if (!dizzyTill && napper.state === "idle" && now > awakeUntil) napper.setState("sleeping");
    if (pointer) {
      napper.lookX = clamp((pointer.x - cx) / 220, -1, 1);
      napper.lookY = clamp((pointer.y - cy) / 160, -1, 1);
    }
    napper.update(dt);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, S, S);
    ctx.translate(11, 0);
    napper.particleOverhang = 22;
    napper.draw(ctx, S - 22, S);
  };
  new IntersectionObserver(([e]) => {
    shown = e.isIntersecting && document.visibilityState === "visible";
    if (shown && !loop) {
      prev = performance.now();
      loop = requestAnimationFrame(tick);
    }
  }).observe(canvas);
}

const napCanvas = document.querySelector<HTMLCanvasElement>(".foot-ciel");
if (napCanvas) footerCiel(napCanvas);

// ── Go ────────────────────────────────────────────────────────────────────────

resize();
const initial = scrollState();
sentView = initial.view;
mood = initial.mood;
activeStep = initial.step;
engine.setState(mood, true);
startStage(initial.view);
updateRunning();
// Should the sky be slow to come, Ciel doesn't wait for it.
setTimeout(() => cielCanvas.classList.add("on"), 2500);
