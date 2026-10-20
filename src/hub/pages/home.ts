// Home, the page the Ciel window opens on: your sky, right now, with Ciel in
// it. The real one (src/sky): the sun and the moon where they are for you,
// today's clouds drifting, the light of the hour, the same as his wallpaper.
// Over it, the greeting of the hour and what the sky does next, a line from
// sunrise to sunset with the sun on it, and the two things you do most here.
// Under it, your day: what you dictated, the time saved, the streak, the last
// dictations and the to-dos your calls left.
//
// The sky draws only while the page is on screen and the window visible, at
// a calm 24 frames a second; reduced motion keeps it still.

import { h, clear } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { Bridge } from "../../core/bridge";
import { FlowBridge, type FlowEntry, type FlowStats, type Note, type Todo } from "../../core/flow";
import { SkyRenderer } from "../../sky/render";
import { toScreen } from "../../sky/draw";
import { lightFor, luma, mix } from "../../sky/palette";
import { DEFAULT_WALLPAPER, type WallpaperPrefs } from "../../sky/prefs";
import { sceneSpec, secondsOfDay, type SceneSpec } from "../../sky/scenes";
import { moonPhase, placeFor, sunTimes, type Place } from "../../sky/sun";
import type { HubContext } from "../shell";
import { icon, iconButton, keyLabel, number, shortCount } from "../ui";
import { duration } from "./insights";

const FPS = 24;
/** Where Ciel floats: right of the words, above the sea of clouds. */
const CIEL_SPOTS = [0.64, 0.84];

/** Ciel keeps his distance from the sun (or the moon), wherever it is in the day. */
function cielFor(spec: SceneSpec): SceneSpec["ciel"] {
  const light = spec.sun.elevation > -4 ? spec.sun : spec.moon;
  const lx = light ? toScreen(light.elevation, light.azimuth, spec.facing).x : 0;
  const x = CIEL_SPOTS.reduce((best, c) => (Math.abs(c - lx) > Math.abs(best - lx) ? c : best));
  return { size: 0.3, x, y: 0.52 };
}

const lang = () => document.documentElement.lang || undefined;
const hhmm = (d: Date) => d.toLocaleTimeString(lang(), { hour: "numeric", minute: "2-digit" });
const todayKey = (d = new Date()) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

function greeting(hour: number): string {
  if (hour >= 5 && hour < 12) return t("Good morning");
  if (hour >= 12 && hour < 18) return t("Good afternoon");
  if (hour >= 18 && hour < 23) return t("Good evening");
  return t("Good night");
}

/** What the sky does next, in a sentence. */
function skyLine(now: Date, place: Place): string {
  const { sunrise, sunset } = sunTimes(now, place.lat, place.lon);
  if (!sunrise || !sunset) return "";
  if (now < sunrise) return t("The sun rises at {0}.", { 0: hhmm(sunrise) });
  if (now < sunset) {
    const left = (sunset.getTime() - now.getTime()) / 60_000;
    return left <= 75
      ? t("Golden hour: the sun sets at {0}.", { 0: hhmm(sunset) })
      : t("The sun sets at {0}.", { 0: hhmm(sunset) });
  }
  const lit = Math.round(((1 - Math.cos(2 * Math.PI * moonPhase(now))) / 2) * 100);
  return t("Tonight the moon is {0}% lit.", { 0: lit });
}

/** Sunrise, the sun now, sunset: where the day stands. */
function dayLine(now: Date, place: Place): HTMLElement | null {
  const { sunrise, sunset } = sunTimes(now, place.lat, place.lon);
  if (!sunrise || !sunset) return null;
  const k = (now.getTime() - sunrise.getTime()) / (sunset.getTime() - sunrise.getTime());
  const sun = h("i", { class: k < 0 || k > 1 ? "home-sun night" : "home-sun" });
  sun.style.left = `${Math.min(1, Math.max(0, k)) * 100}%`;
  const done = h("i", { class: "home-day-done" });
  done.style.width = `${Math.min(1, Math.max(0, k)) * 100}%`;
  return h("div", { class: "home-day" },
    h("span", { class: "home-day-time", text: hhmm(sunrise) }),
    h("div", { class: "home-day-track" }, done, sun),
    h("span", { class: "home-day-time", text: hhmm(sunset) }),
  );
}

// ── The sky ───────────────────────────────────────────────────────────────────

interface Sky {
  el: HTMLElement;
  stop(): void;
}

function buildSky(prefs: WallpaperPrefs, ctx: HubContext, onTone: (dark: boolean) => void): Sky {
  const canvas = h("canvas", { class: "home-sky-canvas" }) as HTMLCanvasElement;
  const renderer = new SkyRenderer();
  const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  let spec: SceneSpec;
  let place: Place;
  let specAt = 0;
  let frame: number | null = null;
  let last = 0;
  let stopped = false;

  const refresh = (now: Date) => {
    place = placeFor(prefs.lat, prefs.lon, now);
    const base = sceneSpec(now, "idle", prefs, { place });
    spec = { ...base, ciel: cielFor(base) };
    specAt = now.getTime();
    const l = lightFor(spec.theme, spec.sun.elevation, spec.afterglow);
    onTone(luma(mix(mix(l.upper, l.mid, 0.5), l.zenith, 0.3)) < 150);
  };
  refresh(new Date());

  const fit = () => {
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = Math.round(canvas.clientWidth * dpr);
    const hgt = Math.round(canvas.clientHeight * dpr);
    if (w > 1 && hgt > 1 && (canvas.width !== w || canvas.height !== hgt)) {
      canvas.width = w;
      canvas.height = hgt;
    }
  };

  const draw = (now: number) => {
    frame = null;
    if (stopped || !canvas.isConnected) return;
    if (document.visibilityState === "visible") {
      if (now - last >= 1000 / FPS || still) {
        last = now;
        const date = new Date();
        // The sun moves: a new sky every minute.
        if (date.getTime() - specAt > 60_000) refresh(date);
        fit();
        const c = canvas.getContext("2d");
        if (c) renderer.render(c, spec, secondsOfDay(date), { still });
      }
      if (!still) frame = requestAnimationFrame(draw);
    }
  };
  const wake = () => {
    if (!stopped && frame === null && document.visibilityState === "visible") frame = requestAnimationFrame(draw);
  };
  // A resize rebuilds the sky's still layers: once, when it settles.
  let settle: number | null = null;
  const resized = new ResizeObserver(() => {
    if (settle != null) window.clearTimeout(settle);
    settle = window.setTimeout(() => {
      settle = null;
      if (still) wake();
    }, 120);
  });
  resized.observe(canvas);
  document.addEventListener("visibilitychange", wake);
  // Built before it is in the window: the first frame waits for it.
  requestAnimationFrame(wake);

  const words = h("div", { class: "home-words" },
    h("div", { class: "home-date", text: new Date().toLocaleDateString(lang(), { weekday: "long", day: "numeric", month: "long" }) }),
    h("h1", { class: "home-hello", text: greeting(new Date().getHours()) }),
    h("p", { class: "home-skyline", text: skyLine(new Date(), place!) }),
  );
  const status = ctx.status;
  const key = keyLabel(status.keys === "wayland" ? "wayland" : status.prefs.key, status.platform);
  const dictate = h("button", { class: "home-glass", onclick: () => ctx.navigate("dictation") },
    icon("mic"),
    h("span", {
      text: !status.prefs.enabled
        ? t("Turn on dictation")
        : status.models.parakeet.state !== "ready"
          ? t("Download the voice model to dictate")
          : t("Hold {0} to dictate", { 0: key }),
    }),
  );
  const record = h("button", {
    class: "home-glass",
    onclick: () => void FlowBridge.startNote().then(() => ctx.navigate("notes"), () => ctx.navigate("notes")),
  }, icon("record"), h("span", { text: t("Record a call") }));

  const el = h("section", { class: "home-sky" },
    canvas,
    h("div", { class: "home-sky-shade" }),
    words,
    h("div", { class: "home-sky-foot" }, h("div", { class: "home-actions" }, dictate, record), dayLine(new Date(), place!)),
  );
  return {
    el,
    stop() {
      stopped = true;
      if (frame != null) cancelAnimationFrame(frame);
      resized.disconnect();
      document.removeEventListener("visibilitychange", wake);
    },
  };
}

// ── Your day ──────────────────────────────────────────────────────────────────

function tile(cls: string, value: string, caption: string, extra?: HTMLElement): HTMLElement {
  return h("div", { class: `home-tile ${cls}` },
    h("b", { class: "home-tile-value", text: value }),
    h("span", { class: "home-tile-caption", text: caption }),
    ...(extra ? [extra] : []),
  );
}

/** The last seven days of words, today brightest. */
function spark(stats: FlowStats | null): HTMLElement {
  const days = (stats?.daily ?? []).slice(-7);
  const max = Math.max(1, ...days.map(([, w]) => w));
  const bars = h("div", { class: "home-spark" });
  days.forEach(([key, w], i) => {
    const bar = h("i", { class: i === days.length - 1 ? "today" : "", title: `${key} · ${number(w)}` });
    bar.style.height = `${Math.max(w ? 10 : 4, (w / max) * 100)}%`;
    bars.append(bar);
  });
  return bars;
}

function recent(entries: FlowEntry[]): HTMLElement {
  const list = h("div", { class: "home-list" });
  for (const e of entries.slice(0, 3)) {
    const when = new Date(e.date);
    const day = todayKey(when) === todayKey() ? hhmm(when) : when.toLocaleDateString(lang(), { day: "numeric", month: "short" });
    list.append(h("div", { class: "home-item" },
      h("div", { class: "home-item-text", text: e.text }),
      h("div", { class: "home-item-meta" },
        h("span", { text: [e.app, day].filter(Boolean).join(" · ") }),
        iconButton("copy", t("Copy"), () => void FlowBridge.copy(e.text)),
      ),
    ));
  }
  if (!entries.length) list.append(h("div", { class: "home-empty", text: t("Dictate in an app and it shows here.") }));
  return list;
}

function todos(notes: Note[]): HTMLElement {
  const open: [Note, Todo][] = [];
  for (const n of notes) for (const td of n.todos) if (!td.done) open.push([n, td]);
  const list = h("div", { class: "home-list" });
  for (const [note, td] of open.slice(0, 5)) {
    const box = h("input", { type: "checkbox" }) as HTMLInputElement;
    const item = h("label", { class: "home-todo" },
      box,
      h("span", { class: "home-todo-text" },
        h("span", { text: td.text }),
        h("small", { text: [note.title, td.owner, td.due].filter(Boolean).join(" · ") }),
      ),
    );
    box.addEventListener("change", () => {
      item.classList.add("done");
      void FlowBridge.toggleTodo(note.id, td.id);
    });
    list.append(item);
  }
  if (!open.length) list.append(h("div", { class: "home-empty", text: t("Nothing left to do from your calls.") }));
  return list;
}

function panel(title: string, more: () => void, body: HTMLElement): HTMLElement {
  return h("div", { class: "home-panel" },
    h("div", { class: "home-panel-head" },
      h("span", { text: title }),
      h("button", { class: "home-more", text: "→", title, onclick: more }),
    ),
    body,
  );
}

export async function homePage(page: HTMLElement, ctx: HubContext) {
  page.classList.add("home");
  const [boot, stats, history, notes] = await Promise.all([
    Bridge.boot().catch(() => null),
    FlowBridge.stats(),
    FlowBridge.history(),
    FlowBridge.notes(),
  ]);
  const prefs = boot?.settings?.wallpaper ?? DEFAULT_WALLPAPER;
  const sky = buildSky(prefs, ctx, (dark) => page.classList.toggle("home-dark-sky", dark));
  ctx.onLeave = () => sky.stop();

  const today = (stats?.daily ?? []).find(([key]) => key === todayKey())?.[1] ?? 0;
  const tiles = h("div", { class: "home-tiles" },
    tile("words", shortCount(today), t("words dictated today"), spark(stats)),
    tile("saved", duration(stats?.week.minutesSaved ?? 0), t("saved this week")),
    tile("streak", number(stats?.streak ?? 0), t("days in a row")),
  );
  const panels = h("div", { class: "home-panels" },
    panel(t("Recent dictations"), () => ctx.navigate("dictation"), recent(history ?? [])),
    panel(t("To-dos from your calls"), () => ctx.navigate("notes"), todos((notes?.notes ?? []).filter((n) => n.status === "ready"))),
  );
  clear(page);
  page.append(sky.el, tiles, panels);
}
