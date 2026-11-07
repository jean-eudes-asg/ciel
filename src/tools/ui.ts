// Small pieces the tools share: a text field that may take the keyboard (the
// island normally never does), a switch, a slider, a round icon button.

import { Bridge } from "../core/bridge";
import { h, svg } from "../views/dom";
import type { Msg } from "../i18n/i18n";

/**
 * A text field in the island. The island never takes the keyboard on its
 * own (the app you work in keeps it); a click in the field asks for it, and
 * leaving the field gives it back.
 */
export function typingField(input: HTMLInputElement): HTMLInputElement {
  input.addEventListener("mousedown", () => void Bridge.focusWindow(true));
  input.addEventListener("focus", () => void Bridge.focusWindow(true));
  input.addEventListener("blur", () => void Bridge.focusWindow(false));
  // Escape leaves the field first; a second one closes the island as usual.
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && input.value) {
      input.value = "";
      input.dispatchEvent(new Event("input"));
      e.stopPropagation();
    }
  });
  return input;
}

export function roundButton(icon: string, title: string | Msg, onClick: (e: MouseEvent) => void, stroke = 2): HTMLButtonElement {
  return h(
    "button",
    {
      class: "tool-round",
      title,
      onclick: (e: Event) => {
        e.stopPropagation();
        onClick(e as MouseEvent);
      },
    },
    svg(icon, 12, stroke ? { stroke } : {}),
  ) as HTMLButtonElement;
}

export interface Switch {
  el: HTMLButtonElement;
  set(on: boolean | null): void;
}

/** A switch; null shows it greyed out (not on this computer). */
export function toolSwitch(onChange: (on: boolean) => void): Switch {
  let state: boolean | null = false;
  const el = h("button", { class: "tool-switch", role: "switch" }) as HTMLButtonElement;
  el.addEventListener("click", (e) => {
    e.stopPropagation();
    if (state == null) return;
    state = !state;
    paint();
    onChange(state);
  });
  function paint() {
    el.classList.toggle("on", state === true);
    el.disabled = state == null;
    el.setAttribute("aria-checked", String(state === true));
  }
  paint();
  return {
    el,
    set(on) {
      state = on;
      paint();
    },
  };
}

export interface Slider {
  el: HTMLElement;
  set(value: number | null): void;
  /** Being dragged: the value shown is the hand's, not the computer's. */
  readonly active: boolean;
}

/**
 * 0…1, drawn as a pill that fills. `onInput` follows the drag (the caller
 * throttles what is slow); `onCommit` comes once it is let go.
 */
export function toolSlider(onInput: (v: number) => void, onCommit: (v: number) => void = onInput): Slider {
  const fill = h("i", { class: "tool-slider-fill" });
  const el = h("div", { class: "tool-slider", role: "slider", tabindex: "0" }, fill);
  let value: number | null = 0;
  let dragging = false;
  const paint = () => {
    el.classList.toggle("off", value == null);
    fill.style.width = `${Math.round((value ?? 0) * 100)}%`;
    el.setAttribute("aria-valuenow", String(Math.round((value ?? 0) * 100)));
  };
  const at = (e: PointerEvent) => {
    const r = el.getBoundingClientRect();
    return Math.max(0, Math.min(1, (e.clientX - r.left) / Math.max(1, r.width)));
  };
  el.addEventListener("pointerdown", (e) => {
    if (value == null) return;
    e.stopPropagation();
    dragging = true;
    el.setPointerCapture(e.pointerId);
    value = at(e);
    paint();
    onInput(value);
  });
  el.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    value = at(e);
    paint();
    onInput(value);
  });
  const end = (e: PointerEvent) => {
    if (!dragging) return;
    dragging = false;
    value = at(e);
    paint();
    onCommit(value);
  };
  el.addEventListener("pointerup", end);
  el.addEventListener("pointercancel", end);
  el.addEventListener("wheel", (e) => {
    if (value == null) return;
    e.preventDefault();
    value = Math.max(0, Math.min(1, value + (e.deltaY < 0 ? 0.05 : -0.05)));
    paint();
    onCommit(value);
  }, { passive: false });
  paint();
  return {
    el,
    set(v) {
      if (dragging) return;
      value = v;
      paint();
    },
    get active() {
      return dragging;
    },
  };
}

/** Runs `f` at most every `ms`, and once more with the last value at the end. */
export function throttle<T>(ms: number, f: (v: T) => void): (v: T) => void {
  let last = 0;
  let pending: T | null = null;
  let timer: number | null = null;
  return (v: T) => {
    const now = performance.now();
    if (now - last >= ms) {
      last = now;
      f(v);
      return;
    }
    pending = v;
    if (timer == null) {
      timer = window.setTimeout(() => {
        timer = null;
        last = performance.now();
        if (pending != null) f(pending);
        pending = null;
      }, ms - (now - last));
    }
  };
}

/** "2 min ago", "yesterday", in the language. */
export function ago(at: number, lang: string | undefined, now = Date.now()): string {
  const rtf = new Intl.RelativeTimeFormat(lang, { numeric: "auto", style: "short" });
  const s = Math.round((at - now) / 1000);
  if (Math.abs(s) < 45) return rtf.format(0, "second");
  const m = Math.round(s / 60);
  if (Math.abs(m) < 60) return rtf.format(m, "minute");
  const hr = Math.round(m / 60);
  if (Math.abs(hr) < 24) return rtf.format(hr, "hour");
  return rtf.format(Math.round(hr / 24), "day");
}

/** "1.2 MB". */
export function size(bytes: number, lang: string | undefined): string {
  const units = ["byte", "kilobyte", "megabyte", "gigabyte"] as const;
  let v = bytes;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  return new Intl.NumberFormat(lang, { style: "unit", unit: units[i], unitDisplay: "short", maximumFractionDigits: v < 10 && i > 0 ? 1 : 0 }).format(v);
}
