// The dock's bar, drawn from what it is told (src/dock/main.ts tells it from
// Rust and the island; dev/dock-preview.ts from made-up apps). What the
// Windows taskbar holds, in Ciel's look: Start (which searches too), task view
// and widgets;
// Ciel with his agents; the apps (a dot under the open ones, a longer one
// under the one in front); what plays (its cover, its buttons, how far it has
// gone and the line being sung); the hidden taskbar's arrow; the sound,
// the network and the battery; the time; show desktop.
//
// Under the pointer the icons grow, the others making room (core/dock.ts
// magnifyLayout), the name of the one pointed at above it. An app with
// windows shows them after a moment (Rust draws them); a right click opens
// its menu (Rust's too); dragged, it moves among the pins, or is pinned.

import {
  CREW_MAX, EMPTY_SNAPSHOT, NO_SYSTEM, agentsLine, crewNow, dockLyric, dropIndex, hour12, magnifyLayout, magnifyScale, playedAt, volumeLevel,
  type DockApp, type DockPanel, type DockPlaying, type DockPrefs, type DockSnapshot, type DockSource, type DockState, type MagnifyItem, type SystemIcons,
} from "../core/dock";
import type { Settings } from "../core/state";
import { BASE, PUFFS } from "../cloud/shape";
import { Spring } from "../core/anim";
import { dominantPct, effectivePct } from "../core/plan";
import { t } from "../i18n/i18n";
import { ICONS } from "../views/icons";
import { h, svg } from "../views/dom";

/** What a click asks for. */
export interface DockActions {
  start(): void;
  panel(what: DockPanel): void;
  /** Ciel's tile: `waiting` while a session waits for an answer. */
  agents(waiting: boolean): void;
  /** The pointer rests on Ciel's tile, or has left it; "close": clicked, the window over it goes at once (GNOME). */
  hoverAgents(on: boolean | "close"): void;
  open(id: string, newWindow: boolean): void;
  /** A right click: an app's menu, or the bar's (null). */
  menu(id: string | null): void;
  /** An app's windows above its icon (x: across the window, CSS pixels), or null for them to go. */
  preview(id: string | null, x: number): void;
  /** An app dropped at `to` among the pins. */
  pinMove(id: string, to: number): void;
  /** The wheel on the sound, by notches (up is positive). */
  volume(notches: number): void;
  /** A button of what plays, or its cover and title ("open": the player's window). */
  music(action: "playPause" | "next" | "previous" | "open", source: DockSource): void;
  taskbar(): void;
  /** The pointer came onto the bar, or has left it and the icons are back at rest. */
  hover(on: boolean): void;
}

export type DockLook = Pick<DockPrefs, "magnify" | "taskView" | "widgets">;

export interface DockBar {
  /** The whole window's content: the bar at its bottom, the room above it. */
  root: HTMLElement;
  /** The bar at rest, whose width the window takes. */
  el: HTMLElement;
  setState(s: DockState): void;
  /** The sound, the network and the battery, changed. */
  setSystem(s: SystemIcons): void;
  setSnapshot(s: DockSnapshot): void;
  /** Slid away under a window (GNOME's auto-hide), or back. */
  setAway(away: boolean): void;
  /** The cover of what plays (a data: URL), or null. */
  setArt(url: string | null): void;
  setClockFormat(f: Settings["clockFormat"]): void;
  setLook(look: DockLook): void;
  /** Texts again in the language now set. */
  relabel(): void;
  /** The clock, now. */
  tick(now?: Date): void;
}

const NS = "http://www.w3.org/2000/svg";

function make(tag: string, attrs: Record<string, string | number>): SVGElement {
  const el = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, String(v));
  return el;
}

/** Ciel's outline (cloud/shape.ts), with his two eyes. */
function cielMark(): SVGSVGElement {
  const el = make("svg", { viewBox: "-1.1 -0.9 2.2 1.75", class: "ciel", "aria-hidden": "true" }) as SVGSVGElement;
  const body = make("g", { class: "body" });
  for (const [cx, cy, r] of PUFFS) body.append(make("circle", { cx, cy, r }));
  body.append(make("ellipse", { cx: 0, cy: BASE.cy, rx: BASE.ax, ry: BASE.ay }));
  el.append(body);
  const eyes = make("g", { class: "eyes" });
  for (const cx of [-0.26, 0.26]) eyes.append(make("ellipse", { class: "eye", cx, cy: 0.12, rx: 0.085, ry: 0.13 }));
  el.append(eyes);
  return el;
}

/** A battery filled as charged, with a bolt while it charges. */
function batteryIcon(percent: number, charging: boolean): SVGSVGElement {
  const el = make("svg", { viewBox: "0 0 24 24", width: 16, height: 16, "aria-hidden": "true" }) as SVGSVGElement;
  el.append(make("rect", { x: 2.6, y: 7.4, width: 17, height: 9.2, rx: 2.2, fill: "none", stroke: "currentColor", "stroke-width": 1.6 }));
  el.append(make("rect", { x: 20.4, y: 10.4, width: 1.8, height: 3.2, rx: 0.8, fill: "currentColor" }));
  const level = Math.max(0, Math.min(100, percent)) / 100;
  el.append(make("rect", { x: 4.4, y: 9.2, width: (13.4 * level).toFixed(2), height: 5.6, rx: 1, fill: "currentColor", opacity: charging ? 0.45 : 1 }));
  if (charging) el.append(make("path", { d: "M12.6 6.2 8.6 12.4h3l-1 5.4 4.2-6.6h-3.1z", fill: "currentColor" }));
  return el;
}

const icon = (path: string, size = 18) => svg(path, size, { stroke: 1.7 });

/** On GNOME, Start is the app grid, task view the overview, and there are no widgets. */
const GNOME = typeof navigator !== "undefined" && !/Windows/.test(navigator.userAgent ?? "");

/** The icons grow back to rest, or out, in this long (ms). */
/**
 * How the icons grow and shrink back: the island's spring (core/anim.ts),
 * quicker and damped enough never to overshoot, so they settle, not wobble.
 */
const GROW_RESPONSE = 0.3;
const GROW_DAMPING = 0.9;
/** The pointer rests on an app this long before its windows show. */
const PREVIEW_MS = 450;
/** A press moves this far before it is a drag. */
const DRAG_PX = 6;

export function buildDockBar(actions: DockActions): DockBar {
  let state: DockState = { apps: [], taskbarHidden: false, shown: false, system: NO_SYSTEM };
  let snapshot: DockSnapshot = EMPTY_SNAPSHOT;
  let clockFormat: Settings["clockFormat"] = "auto";
  let look: DockLook = { magnify: true, taskView: true, widgets: false };

  const bg = h("div", { class: "bg" });

  const start = h("button", { class: "hit start mag", onclick: () => actions.start() }, icon(ICONS.apps, 18));
  const taskView = h("button", { class: "hit start mag", onclick: () => actions.panel("taskView") }, icon(ICONS.taskView, 18));
  const widgets = h("button", { class: "hit start mag", onclick: () => actions.panel("widgets") }, icon(ICONS.widgets, 18));

  // Ciel, a dot under him for each session, and words only when one asks the user.
  const pips = h("span", { class: "pips" });
  const agentsTop = h("span", { class: "top" });
  const agentsMain = h("span", { class: "main" });
  const agentsText = h("span", { class: "txt", hidden: true }, agentsTop, agentsMain);
  // The Claude plan's usage, filling from the bottom as the limit comes near.
  const gauge = h("span", { class: "gauge", hidden: true }, h("i"));
  const agents = h("button", { class: "hit agents quiet mag" }, h("span", { class: "me" }, cielMark(), pips), gauge, agentsText);
  let agentsTimer = 0;
  agents.addEventListener("click", () => {
    window.clearTimeout(agentsTimer);
    actions.hoverAgents("close");
    actions.agents(snapshot.agents.asking > 0);
  });
  // Resting on Ciel opens the window of the agents, their requests and the plan over him.
  agents.addEventListener("pointerenter", () => {
    window.clearTimeout(agentsTimer);
    agentsTimer = window.setTimeout(() => actions.hoverAgents(true), PREVIEW_MS);
  });
  agents.addEventListener("pointerleave", () => {
    window.clearTimeout(agentsTimer);
    actions.hoverAgents(false);
  });

  const apps = h("div", { class: "apps" });
  const appsSep = h("div", { class: "sep" });
  const drop = h("div", { class: "drop", hidden: true });

  const mediaCover = h("span", { class: "cover" }, svg(ICONS.musicNote, 13));
  const mediaTitle = h("span", { class: "main" });
  // The artist, or the line being sung.
  const mediaLine = h("span", { class: "top" });
  const control = (action: "playPause" | "next" | "previous" | "open") => () => {
    const p = snapshot.playing;
    if (p && (action !== "open" || p.opens)) actions.music(action, p.source);
  };
  const mediaPrev = h("button", { class: "skip", "aria-label": t("Previous"), onclick: control("previous") }, svg(ICONS.backward, 11));
  const mediaPlay = h("button", { class: "play", onclick: control("playPause") });
  const mediaNext = h("button", { class: "skip", "aria-label": t("Next"), onclick: control("next") }, svg(ICONS.forward, 11));
  // How far the song has gone.
  const mediaGone = h("i", { class: "gone" });
  const media = h("div", { class: "hit media" },
    h("span", { class: "info", onclick: control("open") }, mediaCover, h("span", { class: "txt" }, mediaTitle, mediaLine)),
    mediaPrev, mediaPlay, mediaNext, mediaGone);

  const tray = h("button", { class: "hit tray", onclick: () => actions.taskbar() }, svg(ICONS.chevronUp, 14, { stroke: 2 }));

  const system = h("button", { class: "hit system", onclick: () => actions.panel("quickSettings") });
  system.addEventListener("wheel", (e) => {
    e.preventDefault();
    if (e.deltaY !== 0) actions.volume(e.deltaY < 0 ? 1 : -1);
  }, { passive: false });

  const clockDate = h("span", { class: "date" });
  const clockTime = h("span", { class: "time" });
  const clock = h("button", { class: "hit clock", onclick: () => actions.panel("notifications") }, clockDate, clockTime);

  const desktop = h("button", { class: "hit desktop", onclick: () => actions.panel("desktop") }, icon(ICONS.desktop, 15));

  const el = h("div", { class: "bar" },
    bg, start, taskView, widgets, agents, h("div", { class: "sep" }), apps, appsSep, media, tray, system, clock, desktop, drop);
  // Faded in and out (dock.css), never moved by a transition: it keeps up with the icons growing.
  const label = h("div", { class: "label" });
  const root = h("div", { class: GNOME ? "dock gnome" : "dock" }, label, el);

  /** What the label says over an item. */
  const labels = new Map<Element, () => string>();
  labels.set(start, () => GNOME ? t("Show Apps") : t("Start menu"));
  labels.set(taskView, () => GNOME ? t("Activities") : t("Task view"));
  labels.set(widgets, () => t("Widgets"));
  labels.set(tray, () => t("Show the Windows taskbar"));
  labels.set(desktop, () => t("Show desktop"));
  // Each session on its line: who, where, doing what.
  // and the plan's two windows under them.
  labels.set(agents, () => {
    const now = Date.now();
    const all = crewNow(snapshot.crew, now);
    const lines = all.length ? all.map((m) => {
      const doing = m.phase === "asking" ? t("Asks you a question") : m.line;
      return [m.agent, m.project, doing].filter(Boolean).join(" · ");
    }) : [t("No agent is running.")];
    const plan = snapshot.plan;
    if (plan && (plan.fiveHour || plan.sevenDay)) {
      const pct = (w: typeof plan.fiveHour) => (w ? `${Math.round(effectivePct(w, now))}%` : "-");
      lines.push(`${t("Claude plan usage")} · ${t("5 hours")} ${pct(plan.fiveHour)} · ${t("Week")} ${pct(plan.sevenDay)}`);
    }
    return lines.join("\n");
  });
  labels.set(media, () => {
    const p = snapshot.playing;
    return p ? (p.artist ? `${p.title} · ${p.artist}` : p.title) : "";
  });
  labels.set(system, () => systemLines().join("\n"));
  labels.set(clock, () => new Date().toLocaleDateString(document.documentElement.lang || undefined, { weekday: "long", day: "numeric", month: "long", year: "numeric" }));

  function paintAgents() {
    const line = agentsLine(snapshot);
    const all = crewNow(snapshot.crew, Date.now());
    // Kept, not drawn again: only a new one comes in, and a dot that changes eases to its new look.
    const phases = all.slice(0, CREW_MAX).map((m) => m.phase);
    while (pips.children.length < phases.length) pips.append(h("i"));
    while (pips.children.length > phases.length) pips.lastElementChild!.remove();
    phases.forEach((phase, i) => (pips.children[i] as HTMLElement).className = phase);
    const used = dominantPct(snapshot.plan);
    gauge.hidden = used == null;
    gauge.style.setProperty("--used", `${Math.max(0, Math.min(100, used ?? 0))}%`);
    // Near the limit it says so the dock's way: drawn over, not in colour.
    gauge.classList.toggle("high", (used ?? 0) >= 80);
    agents.classList.toggle("asking", line.kind === "asking");
    agents.classList.toggle("working", line.kind === "working");
    // Ciel alone grows; with words beside him the tile doesn't.
    agents.classList.toggle("quiet", line.kind !== "asking");
    agents.classList.toggle("mag", line.kind !== "asking");
    agentsText.hidden = line.kind !== "asking";
    if (line.kind === "asking") {
      agentsTop.textContent = line.agent || t("Agents");
      agentsMain.textContent = t("Asks you a question");
    }
    agents.setAttribute("aria-label", labels.get(agents)!().replace(/\n/g, ", "));
  }

  let goneTimer = 0;
  let singTimer = 0;
  /** Slid away: what moves in the bar rests, as when the dock is off screen. */
  let away = false;
  const seen = () => state.shown && !away;

  function paintMedia() {
    const p = snapshot.playing;
    media.hidden = !p;
    window.clearTimeout(singTimer);
    window.clearTimeout(goneTimer);
    if (!p) return;
    mediaTitle.textContent = p.title;
    mediaPlay.replaceChildren(svg(p.playing ? ICONS.pause : ICONS.play, 14));
    mediaPlay.setAttribute("aria-label", p.playing ? t("Pause") : t("Play"));
    media.classList.toggle("opens", p.opens);
    // With words the tile keeps one width: a new line never moves the dock.
    media.classList.toggle("words", p.lyrics.length > 0);
    runGone(p);
    sing(p);
  }

  /** A step a second, about a pixel: a transition the length of the song would draw every frame of it. */
  function runGone(p: DockPlaying) {
    window.clearTimeout(goneTimer);
    mediaGone.hidden = !(p.duration > 0);
    if (mediaGone.hidden) return;
    mediaGone.style.transform = `scaleX(${playedAt(p, Date.now()) / p.duration})`;
    if (p.playing && seen()) goneTimer = window.setTimeout(() => runGone(p), 1000);
  }

  /** The line sung now, in place of the artist, until the next one is due. */
  function sing(p: DockPlaying) {
    window.clearTimeout(singTimer);
    const { text, nextIn } = dockLyric(p, Date.now());
    const line = text || p.artist;
    if (mediaLine.textContent !== line) {
      mediaLine.textContent = line;
      mediaLine.classList.toggle("sung", !!text);
      // Each new line comes up into place, as in the island.
      mediaLine.classList.remove("rise");
      if (text) {
        void mediaLine.offsetWidth;
        mediaLine.classList.add("rise");
      }
    }
    if (nextIn != null && seen()) singTimer = window.setTimeout(() => sing(p), nextIn + 20);
  }

  function systemLines(): string[] {
    const s = state.system;
    const lines: string[] = [];
    lines.push(!s.network ? t("Not connected")
      : s.network === "wifi" ? (s.networkName || t("Wi-Fi"))
      : s.network === "mobile" ? t("Mobile") : t("Wired"));
    if (s.volume !== null) lines.push(s.muted ? t("Muted") : t("Volume {0}%", { 0: Math.round(s.volume * 100) }));
    if (s.battery !== null) lines.push(t("Battery {0} %", { 0: Math.round(s.battery) }));
    return lines;
  }

  function paintSystem() {
    const s = state.system;
    const parts: Element[] = [];
    const net = s.network === "wired" ? icon(ICONS.network, 16) : icon(ICONS.wifi, 16);
    if (!s.network) net.classList.add("off");
    parts.push(net);
    if (s.volume !== null) {
      const level = volumeLevel(s.volume, s.muted);
      parts.push(icon([ICONS.volume0, ICONS.volume1, ICONS.volume2, ICONS.volume3][level], 16));
    }
    if (s.battery !== null) parts.push(batteryIcon(s.battery, s.charging));
    system.replaceChildren(...parts);
    system.setAttribute("aria-label", systemLines().join(", "));
  }

  // ── The apps ───────────────────────────────────────────────────────────────

  /** One button per app, kept by its id: a focus change only moves the dot. */
  const buttons = new Map<string, { b: HTMLButtonElement; app: DockApp; face: string }>();
  /** A drag just ended on this press: its click does nothing. */
  let swallowClick = false;

  function appButton(id: string): HTMLButtonElement {
    const b = h("button", { class: "hit app mag" });
    // A kept button is always the same app: its id is all the handlers need.
    b.addEventListener("click", () => {
      if (swallowClick) {
        swallowClick = false;
        return;
      }
      endPreview();
      actions.open(id, false);
    });
    // A middle click starts another window, as on the taskbar.
    b.addEventListener("auxclick", (e) => {
      if (e.button === 1) actions.open(id, true);
    });
    b.addEventListener("mousedown", (e) => {
      if (e.button === 1) e.preventDefault();
    });
    b.addEventListener("pointerenter", () => wantPreview(id));
    b.addEventListener("pointerleave", () => leavePreview(id));
    b.addEventListener("pointerdown", (e) => pressed(e, id, b));
    labels.set(b, () => buttons.get(id)?.app.name ?? "");
    return b;
  }

  function paintButton(app: DockApp): HTMLButtonElement {
    let kept = buttons.get(app.id);
    if (!kept) {
      kept = { b: appButton(app.id), app, face: "" };
      buttons.set(app.id, kept);
      // Its icon comes in once (dock.css), not each time the row is put in order again.
      const b = kept.b;
      b.classList.add("born");
      b.addEventListener("animationend", () => b.classList.remove("born"), { once: true });
    }
    const { b } = kept;
    const windowsChanged = kept.app.windows !== app.windows;
    kept.app = app;
    const face = `${app.icon ?? ""}|${app.name}`;
    if (face !== kept.face) {
      kept.face = face;
      b.replaceChildren(app.icon
        ? h("img", { src: app.icon, alt: "", draggable: "false" })
        : h("span", { class: "letter", text: (app.name.trim()[0] ?? "?").toUpperCase() }));
    }
    b.setAttribute("aria-label", app.name);
    b.classList.toggle("active", app.active);
    b.classList.toggle("attention", app.attention);
    b.classList.toggle("pinned", app.pinned);
    const dot = b.querySelector(".dot");
    if (app.windows > 0 && !dot) b.append(h("span", { class: "dot" }));
    else if (app.windows === 0) dot?.remove();
    if (windowsChanged && previewFor === app.id) askPreview(app.id);
    return b;
  }

  function paintApps() {
    const row = state.apps.map(paintButton);
    for (const [id, kept] of buttons) {
      if (!state.apps.some((a) => a.id === id)) {
        labels.delete(kept.b);
        buttons.delete(id);
      }
    }
    // Moved only when the order changed: a button put back loses its transition.
    const same = row.length === apps.children.length && row.every((b, i) => apps.children[i] === b);
    if (!same) apps.replaceChildren(...row);
    appsSep.hidden = state.apps.length === 0;
    tray.hidden = !state.taskbarHidden;
    paintSystem();
  }

  // ── Previews of an app's windows (drawn by Rust) ────────────────────────────

  let previewTimer = 0;
  /** A preview shows, or was asked for: the next app shows its at once. */
  let previewing = false;
  let previewFor: string | null = null;

  function askPreview(id: string) {
    const kept = buttons.get(id);
    if (!kept || kept.app.windows === 0 || dragging) {
      if (previewFor) actions.preview(null, 0);
      previewFor = null;
      return;
    }
    const r = kept.b.getBoundingClientRect();
    previewFor = id;
    previewing = true;
    actions.preview(id, r.left + r.width / 2);
  }

  function wantPreview(id: string) {
    window.clearTimeout(previewTimer);
    if (dragging) return;
    if (previewing) askPreview(id);
    else previewTimer = window.setTimeout(() => askPreview(id), PREVIEW_MS);
  }

  function leavePreview(id: string) {
    window.clearTimeout(previewTimer);
    if (previewFor === id) {
      previewFor = null;
      actions.preview(null, 0);
    }
  }

  function endPreview() {
    window.clearTimeout(previewTimer);
    previewing = false;
    if (previewFor) actions.preview(null, 0);
    previewFor = null;
  }

  // ── Growing under the pointer ──────────────────────────────────────────────

  const grow = new Spring(0, GROW_RESPONSE, GROW_DAMPING);
  let pointerX = 0;
  let frame = 0;
  let lastTime = 0;
  let hovered = false;
  /** What the window was last told: its room above the bar takes clicks. */
  let roomTakesClicks = false;
  let pointedAt: Element | null = null;

  /** The bar's items as laid out at rest, the apps' among them. */
  function items(): HTMLElement[] {
    const out: HTMLElement[] = [];
    for (const child of Array.from(el.children) as HTMLElement[]) {
      if (child === bg || child === drop) continue;
      if (child === apps) out.push(...(Array.from(apps.children) as HTMLElement[]));
      else out.push(child);
    }
    // Left to right as drawn, whatever the page's direction.
    return out.filter((it) => !it.hidden && it.offsetWidth > 0).sort((a, b) => a.offsetLeft - b.offsetLeft);
  }

  function apply() {
    const list = items();
    const amount = look.magnify && !dragging ? Math.max(0, grow.value) : 0;
    if (amount <= 0.001) {
      for (const it of list) if (it !== dragged) it.style.transform = "";
      bg.style.left = bg.style.right = "";
      placeLabel();
      return;
    }
    const laid: MagnifyItem[] = list.map((it) => {
      const centre = it.offsetLeft + it.offsetWidth / 2;
      const s = it.classList.contains("mag") ? magnifyScale(pointerX - centre) : 1;
      return { left: it.offsetLeft, width: it.offsetWidth, scale: 1 + (s - 1) * amount };
    });
    const { shifts, growLeft, growRight } = magnifyLayout(laid, pointerX);
    list.forEach((it, i) => {
      const s = laid[i].scale;
      it.style.transform = s !== 1 ? `translateX(${shifts[i].toFixed(2)}px) scale(${s.toFixed(4)})` : `translateX(${shifts[i].toFixed(2)}px)`;
    });
    bg.style.left = `${(-growLeft).toFixed(2)}px`;
    bg.style.right = `${(-growRight).toFixed(2)}px`;
    placeLabel();
  }

  function step(time: number) {
    frame = 0;
    const dt = lastTime ? Math.min(64, time - lastTime) : 16;
    lastTime = time;
    grow.step(dt / 1000);
    if (grow.settled) grow.set(grow.target);
    apply();
    if (grow.value !== grow.target) {
      frame = requestAnimationFrame(step);
      return;
    }
    lastTime = 0;
    // At rest again and the pointer gone: only the bar takes clicks.
    if (grow.value === 0 && !hovered && roomTakesClicks) {
      roomTakesClicks = false;
      actions.hover(false);
    }
  }

  function kick() {
    if (!frame) frame = requestAnimationFrame(step);
  }

  function placeLabel() {
    // On GNOME the window over Ciel says it all: no label in its way.
    const text = pointedAt && !dragging && !(GNOME && pointedAt === agents) ? labels.get(pointedAt)?.() ?? "" : "";
    if (!text || !pointedAt) {
      label.classList.remove("on");
      return;
    }
    if (label.textContent !== text) label.textContent = text;
    label.classList.add("on");
    const r = pointedAt.getBoundingClientRect();
    const box = root.getBoundingClientRect();
    const w = label.offsetWidth;
    const x = Math.max(4, Math.min(box.width - w - 4, r.left - box.left + r.width / 2 - w / 2));
    label.style.left = `${x.toFixed(1)}px`;
    label.style.top = `${Math.max(2, r.top - box.top - label.offsetHeight - 6).toFixed(1)}px`;
  }

  // A click opens a menu of GNOME's or the overview, which keeps the pointer
  // to itself: the dock never hears it leave, and the label would stay up.
  el.addEventListener("click", () => {
    pointedAt = null;
    placeLabel();
  });
  el.addEventListener("pointerenter", () => {
    hovered = true;
    if (!roomTakesClicks) {
      roomTakesClicks = true;
      actions.hover(true);
    }
    grow.target = 1;
    kick();
  });
  el.addEventListener("pointermove", (e) => {
    pointerX = e.clientX - el.getBoundingClientRect().left;
    const over = (e.target as Element).closest(".bar > *, .apps > *");
    pointedAt = over && labels.has(over) ? over : null;
    if (dragging) dragTo(e);
    kick();
  });
  el.addEventListener("pointerleave", () => {
    if (dragging) return;
    hovered = false;
    pointedAt = null;
    grow.target = 0;
    endPreview();
    kick();
  });

  // ── Right click ────────────────────────────────────────────────────────────

  el.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    const b = (e.target as Element).closest(".app");
    const id = b ? [...buttons].find(([, k]) => k.b === b)?.[0] ?? null : null;
    endPreview();
    actions.menu(id);
  });

  // ── Dragging an app among the pins ─────────────────────────────────────────

  let press: { id: string; b: HTMLButtonElement; x: number; y: number; pointer: number } | null = null;
  let dragging = false;
  let dragged: HTMLElement | null = null;
  let dropAt = -1;

  function pressed(e: PointerEvent, id: string, b: HTMLButtonElement) {
    if (e.button !== 0) return;
    press = { id, b, x: e.clientX, y: e.clientY, pointer: e.pointerId };
  }

  /** The pins' middles at rest (bar's pixels), the dragged one left out. */
  function pinCentres(): number[] {
    return state.apps
      .filter((a) => a.pinned && a.id !== press?.id)
      .map((a) => buttons.get(a.id)!.b)
      .map((b) => b.offsetLeft + b.offsetWidth / 2);
  }

  function dragTo(e: PointerEvent) {
    if (!press || !dragged) return;
    dragged.style.transform = `translateX(${(e.clientX - press.x).toFixed(1)}px)`;
    const centres = pinCentres();
    const app = buttons.get(press.id)?.app;
    const pins = state.apps.filter((a) => a.pinned && a.id !== press!.id).map((a) => buttons.get(a.id)!.b);
    const last = pins[pins.length - 1];
    // An app not pinned is pinned only when dropped among the pins.
    const inPins = app?.pinned || !last || pointerX <= last.offsetLeft + last.offsetWidth + 20;
    dropAt = inPins ? dropIndex(centres, pointerX) : -1;
    drop.hidden = dropAt < 0;
    if (dropAt >= 0) {
      const x = dropAt < pins.length ? pins[dropAt].offsetLeft - 2 : last ? last.offsetLeft + last.offsetWidth + 1 : apps.offsetLeft;
      drop.style.left = `${x}px`;
    }
  }

  window.addEventListener("pointermove", (e) => {
    if (!press || dragging || e.pointerId !== press.pointer) return;
    if (Math.abs(e.clientX - press.x) < DRAG_PX && Math.abs(e.clientY - press.y) < DRAG_PX) return;
    dragging = true;
    dragged = press.b;
    dragged.classList.add("dragging");
    dragged.setPointerCapture(press.pointer);
    endPreview();
    apply();
    dragTo(e);
  });

  window.addEventListener("pointerup", (e) => {
    if (!press || e.pointerId !== press.pointer) return;
    const was = press;
    press = null;
    if (!dragging) return;
    dragging = false;
    swallowClick = true;
    // A click that the drag swallowed comes right after: none might.
    window.setTimeout(() => (swallowClick = false), 0);
    drop.hidden = true;
    dragged?.classList.remove("dragging");
    if (dragged) dragged.style.transform = "";
    dragged = null;
    const app = buttons.get(was.id)?.app;
    if (app && dropAt >= 0) {
      const pins = state.apps.filter((a) => a.pinned);
      const now = pins.findIndex((a) => a.id === was.id);
      if (now !== dropAt) actions.pinMove(was.id, dropAt);
    }
    dropAt = -1;
    if (!el.matches(":hover")) {
      hovered = false;
      grow.target = 0;
    }
    kick();
  });

  // ── The rest ───────────────────────────────────────────────────────────────

  function tick(now = new Date()) {
    const lang = document.documentElement.lang || undefined;
    clockTime.textContent = now.toLocaleTimeString(lang, { hour: "numeric", minute: "2-digit", hour12: hour12(clockFormat) });
    clockDate.textContent = now.toLocaleDateString(lang, { weekday: "short", day: "numeric", month: "short" });
    // The finished ones leave after their minute.
    // and a window of the plan that has reset empties the gauge.
    if (snapshot.plan || snapshot.crew.some((m) => m.phase === "done" || m.phase === "error")) {
      paintAgents();
      kick();
    }
  }

  function paintLook() {
    taskView.hidden = !look.taskView;
    widgets.hidden = !look.widgets || GNOME;
    root.classList.toggle("magnify", look.magnify);
    kick();
  }

  function relabel() {
    for (const [it, text] of labels) {
      if (it !== agents && !(it as HTMLElement).classList.contains("app")) it.setAttribute("aria-label", text());
    }
    paintAgents();
    paintMedia();
    paintApps();
    tick();
    placeLabel();
  }

  paintLook();
  relabel();
  return {
    root,
    el,
    setState(s) {
      const shown = state.shown;
      state = s;
      paintApps();
      // Off screen the song's line and its words rest.
      if (s.shown !== shown) paintMedia();
      kick();
    },
    setSystem(s) {
      state = { ...state, system: s };
      paintSystem();
      placeLabel();
    },
    setSnapshot(s) {
      snapshot = s;
      paintAgents();
      paintMedia();
      kick();
    },
    setAway(a) {
      if (a === away) return;
      away = a;
      root.classList.toggle("away", a);
      paintMedia();
    },
    setArt(url) {
      mediaCover.style.backgroundImage = url ? `url("${url}")` : "";
      mediaCover.classList.toggle("art", !!url);
    },
    setClockFormat(f) {
      clockFormat = f;
      tick();
    },
    setLook(l) {
      look = l;
      paintLook();
    },
    relabel,
    tick,
  };
}
