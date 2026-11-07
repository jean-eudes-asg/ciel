// Tools → Controls: the sound, every microphone at once, the screens'
// brightness, Wi-Fi, Bluetooth, night light (GNOME), and keeping the computer
// awake — Ciel's eyes stay wide open meanwhile. Read when the panel comes
// on screen (src-tauri/src/controls.rs); nothing polls.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import type { Controls } from "../core/tools";
import { SHORTCUTS, displayKeys, effective } from "../core/shortcuts";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import type { ViewActions } from "../views/views";
import { t, tl, type Msg } from "../i18n/i18n";
import { Tools } from "./store";
import { dueText } from "./timers";
import { throttle, toolSlider } from "./ui";
import type { ToolPanel } from "./view";

interface Tile {
  el: HTMLElement;
  sub: HTMLElement;
}

function tile(icon: string, label: Msg, onClick: () => void): Tile {
  const sub = h("span", { class: "ctl-sub" });
  const el = h("button", { class: "ctl-tile", onclick: onClick },
    h("span", { class: "ctl-icon" }, svg(icon, 14, { stroke: 2 })),
    h("span", { class: "ctl-text" }, h("span", { class: "ctl-label", text: label }), sub),
  );
  return { el, sub };
}

const AWAKE_STEPS = [30, 60, 120, -1] as const;

export function buildControls(actions: ViewActions): ToolPanel {
  const apply = (c: Controls | null) => {
    if (c) {
      Tools.controls = c;
      State.notify();
    }
  };
  const set = (what: string, value: number) => void Bridge.controlSet(what, value).then(apply);

  // Sound.
  const volumeIcon = h("button", { class: "ctl-knob", title: tl("Mute"), onclick: () => {
    actions.blip();
    set("muted", Tools.controls?.muted ? 0 : 1);
  } });
  const volume = toolSlider(throttle(60, (v: number) => set("volume", v)));
  const volumeRow = h("div", { class: "ctl-slider-row" }, volumeIcon, volume.el, h("span", { class: "ctl-value" }));
  // Brightness: DDC/CI is slow, so fewer steps while dragging.
  const brightIcon = h("span", { class: "ctl-knob static" }, svg(ICONS.sun, 13, { stroke: 2 }));
  const brightness = toolSlider(throttle(300, (v: number) => set("brightness", v)), (v: number) => set("brightness", v));
  const brightRow = h("div", { class: "ctl-slider-row" }, brightIcon, brightness.el, h("span", { class: "ctl-value" }));

  const mic = tile(ICONS.mic, tl("Microphones"), () => {
    actions.blip();
    void Bridge.micToggle().then(() => Tools.refreshControls());
  });
  mic.el.classList.add("mic");
  const wifi = tile(ICONS.wifi, tl("Wi-Fi"), () => {
    actions.blip();
    set("wifi", Tools.controls?.wifi ? 0 : 1);
  });
  const bt = tile(ICONS.bluetooth, tl("Bluetooth"), () => {
    actions.blip();
    set("bluetooth", Tools.controls?.bluetooth ? 0 : 1);
  });
  const night = tile(ICONS.moon, tl("Night light"), () => {
    actions.blip();
    set("nightLight", Tools.controls?.nightLight ? 0 : 1);
  });
  const awake = tile(ICONS.coffee, tl("Keep awake"), () => {
    actions.blip();
    // Off → 30 min → 1 h → 2 h → until turned off → off.
    const c = Tools.controls;
    let step = -1;
    if (c?.awake) {
      const left = c.awakeUntil == null ? Infinity : (c.awakeUntil - Date.now()) / 60_000;
      step = left === Infinity ? 3 : left > 61 ? 2 : left > 31 ? 1 : 0;
    }
    const next = step + 1;
    void Bridge.awakeSet(next >= AWAKE_STEPS.length ? 0 : AWAKE_STEPS[next]).then(apply);
  });
  awake.el.classList.add("awake");

  const tiles = h("div", { class: "ctl-tiles" }, mic.el, wifi.el, bt.el, night.el, awake.el);
  const el = h("div", { class: "tool tool-controls" }, h("div", { class: "ctl-sliders" }, volumeRow, brightRow), tiles);

  function onOff(on: boolean | null): string {
    return on == null ? t("Not here") : on ? t("On") : t("Off");
  }

  function micKeys(): string {
    const def = SHORTCUTS.find((d) => d.id === "micToggle");
    if (!def) return "";
    const b = effective(def, State.settings.shortcuts);
    return b.enabled && b.keys ? displayKeys(b.keys) : "";
  }

  return {
    el,
    shown() {
      void Tools.refreshControls();
    },
    sync() {
      const c = Tools.controls;
      const pct = (v: number | null | undefined) => (v == null ? "" : `${Math.round(v * 100)}`);
      volume.set(c?.volume ?? null);
      volumeIcon.replaceChildren(svg(c?.muted || c?.volume === 0 ? ICONS.volume0 : (c?.volume ?? 0) < 0.34 ? ICONS.volume1 : (c?.volume ?? 0) < 0.67 ? ICONS.volume2 : ICONS.volume3, 14, { stroke: 2 }));
      volumeRow.classList.toggle("muted", !!c?.muted);
      (volumeRow.lastElementChild as HTMLElement).textContent = c?.muted ? t("Muted") : pct(volume.active ? null : c?.volume);
      brightRow.style.display = c?.brightness == null && c ? "none" : "";
      brightness.set(c?.brightness ?? null);
      (brightRow.lastElementChild as HTMLElement).textContent = pct(c?.brightness);

      const muted = !!c?.micMuted;
      mic.el.classList.toggle("on", muted);
      mic.el.querySelector(".ctl-icon")!.replaceChildren(svg(muted ? ICONS.micOff : ICONS.mic, 14, { stroke: 2 }));
      const keys = micKeys();
      mic.sub.textContent = c && c.mics === 0 ? t("None plugged in") : muted ? t("All muted") : keys ? t("On · {keys}", { keys }) : t("On");
      mic.el.title = keys ? t("Mute every microphone at once ({keys})", { keys }) : t("Mute every microphone at once");

      wifi.el.classList.toggle("on", !!c?.wifi);
      wifi.el.toggleAttribute("disabled", c != null && c.wifi == null);
      wifi.sub.textContent = c ? onOff(c.wifi) : "";
      bt.el.classList.toggle("on", !!c?.bluetooth);
      bt.el.toggleAttribute("disabled", c != null && c.bluetooth == null);
      bt.sub.textContent = c ? onOff(c.bluetooth) : "";
      night.el.style.display = c?.nightLight == null ? "none" : "";
      night.el.classList.toggle("on", !!c?.nightLight);
      night.sub.textContent = c ? onOff(c.nightLight) : "";

      awake.el.classList.toggle("on", !!c?.awake);
      awake.sub.textContent = !c?.awake ? t("Off") : c.awakeUntil == null ? t("Until I say") : t("Until {time}", { time: dueText(c.awakeUntil) });
    },
  };
}
