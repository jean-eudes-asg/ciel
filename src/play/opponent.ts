// The rival across the board, for chess and draughts: the row of five to pick
// from (cast.ts), and him in his own living sky with his name, his face for
// the game, his habits and a line from the brain now and then. The sky draws
// at 20 frames a second while the page is on screen.

import { h } from "../views/dom";
import { t } from "../i18n/i18n";
import { Bridge } from "../core/bridge";
import { Sound, type SoundName } from "../core/sound";
import { BotEngine, hexToRGB } from "../cloud/engine";
import type { BotEmoteName } from "../core/layout";
import { liveCloud } from "../hub/hero";
import { SkyRenderer } from "../sky/render";
import { RIVALS, boardColors, rival, skySpec, type Rival, type RivalId } from "./cast";

const SKY_FPS = 20;

/** How long a person would look before playing, in seconds: a forced move is quick, the opening and a recapture fairly quick. */
export function thinkingTime(r: Rival, legal: number, opening: boolean, recapture: boolean, random = Math.random): number {
  const [lo, hi] = r.think;
  if (legal === 1) return 0.7 + random() * 0.5;
  if (opening) return lo * 0.6 + random() * lo;
  if (recapture) return lo * 0.7 + random() * (hi - lo) * 0.5;
  return lo + random() * (hi - lo);
}

export interface Opponent {
  /** The five rivals, his own lit. */
  picker: HTMLElement;
  /** Him in his sky, his name and his bubble. */
  card: HTMLElement;
  readonly foe: Rival;
  /** Thinking: his face says so, and his eyes go to the board. */
  setThinking(on: boolean): void;
  /** A reaction: an emote, and one of Ciel's sounds in his voice. */
  react(emote: BotEmoteName | null, sound?: SoundName): void;
  face: BotEngine;
  say(line: string | null): void;
  /**
   * Asks the brain for his line: who he is, then `facts`. Said only if
   * `current()` still holds when it comes (the game hasn't moved on).
   */
  talk(facts: string[], current?: () => boolean): void;
  /** Where your hand is over the board, for his eyes. */
  look(at: { x: number; y: number } | null): void;
  leave(): void;
}

/** `el` is the game, which takes his board colours; `picked` is told you chose another rival. */
export function opponent(start: RivalId, el: () => HTMLElement, picked: (r: Rival) => void): Opponent {
  let foe = rival(start);
  let thinking = false;
  let pointer: { x: number; y: number } | null = null;

  const skyCanvas = h("canvas", { class: "cb-sky" }) as HTMLCanvasElement;
  const sky = new SkyRenderer();
  const face = new BotEngine();
  const foeName = h("b", {});
  const foeLine = h("span", {});
  const bubble = h("div", { class: "cb-bubble" });
  const card = h("div", { class: "cb-face" }, skyCanvas, h("div", { class: "cb-who" }, foeName, foeLine), bubble);

  const row = h("div", { class: "cb-rivals" });
  const portraits = new Map<RivalId, HTMLElement>();
  for (const r of RIVALS) {
    const canvas = h("canvas", {}) as HTMLCanvasElement;
    liveCloud(canvas, r.color);
    const b = h("button", { class: "cb-rival", title: r.tagline(), onclick: () => choose(r) }, canvas, h("span", { text: r.name }));
    portraits.set(r.id, b);
    row.append(b);
  }
  const picker = h("div", { class: "cb-pick" }, h("span", { class: "cb-label", text: t("Your opponent") }), row);

  function dress() {
    const colors = boardColors(foe);
    const root = el();
    root.style.setProperty("--cb-light", colors.light);
    root.style.setProperty("--cb-dark", colors.dark);
    root.style.setProperty("--cb-mark", colors.mark);
    root.style.setProperty("--cb-edge", colors.edge);
    face.bodyColor = hexToRGB(foe.color);
    face.setState("idle", true);
    foeName.textContent = foe.name;
    foeLine.textContent = foe.tagline();
    for (const [id, b] of portraits) b.classList.toggle("on", id === foe.id);
    sky.reset();
  }

  function choose(r: Rival) {
    if (r.id === foe.id) return;
    foe = r;
    dress();
    picked(r);
  }

  let bubbleTimer: number | null = null;
  function say(line: string | null) {
    bubble.textContent = line ?? "";
    bubble.classList.toggle("on", !!line);
    if (bubbleTimer !== null) window.clearTimeout(bubbleTimer);
    bubbleTimer = line ? window.setTimeout(() => bubble.classList.remove("on"), 9000) : null;
    if (line) Sound.play("blip", foe.pitch);
  }

  let frame = 0;
  let last = 0;
  let nextQuirk = 0;
  const loop = (now: number) => {
    frame = requestAnimationFrame(loop);
    if (!card.isConnected || document.visibilityState !== "visible" || now - last < 1000 / SKY_FPS) return;
    const dt = Math.min(0.1, (now - last) / 1000);
    last = now;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = skyCanvas.clientWidth;
    const hgt = skyCanvas.clientHeight;
    if (!w || !hgt) return;
    if (skyCanvas.width !== Math.round(w * dpr) || skyCanvas.height !== Math.round(hgt * dpr)) {
      skyCanvas.width = Math.round(w * dpr);
      skyCanvas.height = Math.round(hgt * dpr);
    }
    const x = skyCanvas.getContext("2d");
    if (!x) return;
    sky.render(x, skySpec(foe), now / 1000 + 36_000);
    // He looks at the board, and at your hand over it.
    const box = skyCanvas.getBoundingClientRect();
    if (pointer) {
      face.lookX = Math.max(-1, Math.min(1, (pointer.x - (box.left + w / 2)) / 400));
      face.lookY = Math.max(-1, Math.min(1, ((box.top + hgt / 2) - pointer.y) / 400));
    } else {
      face.lookX = thinking ? -0.6 + 0.25 * Math.sin(now / 900) : -0.4;
      face.lookY = thinking ? -0.3 : -0.1;
    }
    // His habit, now and then: Cirrus dozes, Cumulus loves, Nimbus crackles.
    if (now > nextQuirk) {
      if (nextQuirk) face.emit(foe.quirk, foe.quirk === "spark" && thinking ? 4 : 1);
      nextQuirk = now + 5000 + Math.random() * 6000;
    }
    face.update(dt);
    const size = Math.min(hgt * 0.8, w * 0.5);
    x.setTransform(dpr, 0, 0, dpr, 0, 0);
    x.save();
    x.translate(w * 0.64 - size / 2, hgt - size * 0.95 + Math.sin(now / 1100) * 3);
    face.draw(x, size, size);
    x.restore();
  };

  void Sound.preload(Bridge);
  queueMicrotask(dress);
  frame = requestAnimationFrame(loop);

  return {
    picker,
    card,
    get foe() {
      return foe;
    },
    face,
    setThinking(on) {
      thinking = on;
      face.setState(on ? "thinking" : "idle");
    },
    react(emote, sound) {
      if (emote) face.triggerEmote(emote, 2.5);
      if (sound) Sound.play(sound, foe.pitch);
    },
    say,
    talk(facts, current = () => true) {
      const against = foe.id;
      void Bridge.gameLine([foe.persona, ...facts].join("\n")).then((line) => {
        if (line && foe.id === against && current()) say(line);
      });
    },
    look(at) {
      pointer = at;
    },
    leave() {
      cancelAnimationFrame(frame);
    },
  };
}
