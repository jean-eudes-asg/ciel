// Dictation in the island — where it shows by default (Ciel window →
// Dictation → Where dictation shows): while you dictate, the folded island is
// the pill. Beside Ciel, the dots of your voice; hands-free's ✕ and ✓ around
// them; a call's red dot, clock and stop; a short message. What « OK Ciel »
// heard and answered, or a message with its button, shows under the island.
//
// The same state and dots as the pill (src-tauri/src/flow/pill.rs sends them
// here when dictation shows in the island) and the same rules (core/flow.ts).
// The pill's page still runs, hidden: it speaks « OK Ciel »'s answers. The
// dots move only while something shows; nothing runs otherwise.

import { onEvent } from "../core/bridge";
import { FlowBridge, clock, dotGoal, islandFlowActive, pillLook, pillMessage, type PillState } from "../core/flow";
import { HUB_ICONS, keyLabel } from "../hub/ui";
import { t } from "../i18n/i18n";

const BANDS = 9;
const IDLE: PillState = {
  phase: "idle", edge: "island", key: "right_ctrl", noteStartedAt: null, announcement: null, enabled: false, voice: null,
};

export interface FlowStrip {
  /** The strip, beside Ciel in the folded island. */
  el: HTMLElement;
  /** What « OK Ciel » heard and answered, or a message, under the island. */
  pop: HTMLElement;
  /** Dictation shows in the island and has something to show. */
  readonly active: boolean;
  /** The part of the pop that takes clicks (a message's buttons), in island pixels; null when none. */
  hitBox(): { w: number; h: number } | null;
  /** The island's mode changed: the strip and its pop show only while it is folded. */
  sync(compact: boolean): void;
}

/** Where the state and the dots come from: Rust's events, or a preview page. */
export interface FlowSource {
  onState(fn: (s: PillState) => void): void;
  onBands(fn: (bands: number[]) => void): void;
  initial(): Promise<PillState | null>;
}

const FROM_RUST: FlowSource = {
  onState: (fn) => void onEvent<PillState>("pill-state", fn),
  onBands: (fn) => void onEvent<number[]>("pill-bands", fn),
  initial: () => FlowBridge.pillState(),
};

function icon(name: string): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", HUB_ICONS[name] ?? "");
  svg.append(path);
  return svg;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

/** A button of the strip or the pop: its press stays its own (a press on the island opens it). */
function button(cls: string, title: string, action: () => void, ...children: Node[]): HTMLButtonElement {
  const b = el("button", cls);
  b.title = title;
  b.append(...children);
  b.addEventListener("mousedown", (e) => e.stopPropagation());
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    action();
  });
  return b;
}

const platform: "windows" | "linux" = navigator.userAgent.includes("Windows") ? "windows" : "linux";

export function buildFlowStrip(onChange: () => void, source: FlowSource = FROM_RUST): FlowStrip {
  let state: PillState = IDLE;
  const voice = new Array<number>(BANDS).fill(0);
  const levels = new Array<number>(BANDS).fill(0);
  let shown = false;
  let frame: number | null = null;
  let tickTimer: number | null = null;
  let last = performance.now();
  let lastKey = "";

  const el_ = el("div", "flow-strip");
  const pop = el("div", "flow-pop");
  pop.addEventListener("mousedown", (e) => e.stopPropagation());
  const bars = el("span", "flow-bars");
  for (let i = 0; i < BANDS; i++) bars.append(el("i"));

  const active = () => islandFlowActive(state);

  function content(): HTMLElement[] {
    const key = keyLabel(state.key, platform);
    switch (pillLook(state)) {
      case "handsFree":
        return [
          button("flow-btn", t("Cancel"), () => void FlowBridge.pillAction("cancel"), icon("x")),
          bars,
          button("flow-btn filled", t("Paste · or press {0}", { 0: key }), () => void FlowBridge.pillAction("finish"), icon("check")),
        ];
      case "message":
        return [el("span", "flow-msg", pillMessage(state.phase) ?? "")];
      case "note": {
        const time = el("span", "flow-clock", clock((Date.now() - (state.noteStartedAt ?? Date.now())) / 1000));
        time.dataset.clock = "1";
        return [
          el("span", "flow-rec"),
          time,
          button("flow-btn stop", t("Stop"), () => void FlowBridge.pillAction("stopNote"), el("i")),
        ];
      }
      case "live":
      case "voice":
        return [bars];
      default:
        return [];
    }
  }

  function popContent(): HTMLElement[] {
    const a = state.announcement;
    if (a) {
      const name = { mic: "mic", lock: "lock", clipboard: "copy", download: "download", check: "check", error: "x" }[a.icon] ?? "check";
      const mark = el("span", "flow-pop-icon");
      mark.append(icon(name));
      mark.style.color = a.tint;
      const out: HTMLElement[] = [mark, el("span", "flow-pop-text", a.text)];
      if (a.action) out.push(button("flow-act", a.action, () => void FlowBridge.pillAction("announcement", a.id), document.createTextNode(a.action)));
      out.push(button("flow-btn close", t("Close"), () => void FlowBridge.pillAction("dismiss", a.id), icon("x")));
      return out;
    }
    const v = state.voice;
    if (v && (v.heard || v.answer || v.phase === "listening")) {
      const out: HTMLElement[] = [];
      if (v.heard) out.push(el("span", "flow-heard", v.heard));
      if (v.answer) out.push(el("span", "flow-answer", v.answer));
      else if (!v.heard) out.push(el("span", "flow-heard", t("Listening…")));
      return out;
    }
    return [];
  }

  function render() {
    const on = shown && active();
    el_.classList.toggle("on", on);
    const look = pillLook(state);
    const a = state.announcement;
    const key = `${on}|${look}|${state.phase}|${state.key}|${a?.id ?? ""}|${a?.text ?? ""}|${state.voice?.heard ?? ""}|${state.voice?.answer ?? ""}|${state.voice ? "v" : ""}`;
    if (key === lastKey) return;
    lastKey = key;
    el_.replaceChildren(...(on ? content() : []));
    const inPop = on ? popContent() : [];
    pop.replaceChildren(...inPop);
    pop.classList.toggle("on", inPop.length > 0);
    pop.classList.toggle("announce", on && a != null);
    pop.classList.toggle("voice", on && a == null && inPop.length > 0);
  }

  // ── The dots and the clock ──────────────────────────────────────────────────

  const running = () => shown && active();

  function tick(now: number) {
    frame = null;
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    for (let i = 0; i < BANDS; i++) {
      // The island can't hear the pill's voice: Ciel speaking is a lively wave.
      const goal = dotGoal(state, voice, now, i, state.voice?.phase === "speaking");
      levels[i] += (goal - levels[i]) * Math.min(1, dt * 14);
      (bars.children[i] as HTMLElement).style.height = `${3.4 + 16 * Math.max(0, levels[i])}px`;
    }
    const c = el_.querySelector<HTMLElement>("[data-clock]");
    if (c && state.noteStartedAt) c.textContent = clock((Date.now() - state.noteStartedAt) / 1000);
    schedule();
  }

  function schedule() {
    if (!running() || frame != null || tickTimer != null) return;
    // A call alone: a frame a second for its clock.
    if (pillLook(state) === "note") {
      tickTimer = window.setTimeout(() => {
        tickTimer = null;
        frame = requestAnimationFrame(tick);
      }, 1000);
    } else {
      frame = requestAnimationFrame(tick);
    }
  }

  function stop() {
    if (frame != null) cancelAnimationFrame(frame);
    if (tickTimer != null) window.clearTimeout(tickTimer);
    frame = null;
    tickTimer = null;
  }

  function changed() {
    render();
    if (running()) {
      last = performance.now();
      schedule();
    } else {
      stop();
    }
    onChange();
  }

  source.onState((s) => {
    const was = active();
    state = s;
    if (!active() && !was) {
      // Nothing to show, before or now: nothing to do (the pill has it, or all is quiet).
      render();
      return;
    }
    changed();
  });
  source.onBands((b) => {
    for (let i = 0; i < BANDS; i++) voice[i] = b[i] ?? 0;
  });
  void source.initial().then((s) => {
    if (s) {
      state = s;
      changed();
    }
  });

  return {
    el: el_,
    pop,
    get active() {
      return active();
    },
    hitBox() {
      if (!pop.classList.contains("on") || !pop.classList.contains("announce")) return null;
      return { w: pop.offsetWidth, h: pop.offsetTop + pop.offsetHeight };
    },
    sync(compact: boolean) {
      if (shown === compact) return;
      shown = compact;
      changed();
    },
  };
}
