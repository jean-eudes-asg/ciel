// The Tools tab of the open island: timers, the clipboard, the shelf, quick
// controls and the weather, one at a time beside a small Ciel, with their
// own little tabs. Each tool is a panel (tools/*.ts); what they know comes
// from tools/store.ts.

import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import type { ViewActions, ViewHost } from "../views/views";
import { State } from "../core/state";
import type { ToolId } from "../core/tools";
import { tl, type Msg } from "../i18n/i18n";
import { Tools } from "./store";
import { buildTimers } from "./timers";
import { buildClipboard } from "./clipboard";
import { buildShelf } from "./shelf";
import { buildControls } from "./controls";
import { buildWeather } from "./weather";
import { buildCalendar } from "./calendar";

export interface ToolPanel {
  el: HTMLElement;
  sync(): void;
  /** Came on screen: refresh what it shows. */
  shown?(): void;
  /** Every frame while on screen; true while it animates. */
  tick?(nowMs: number): boolean;
  /** Ctrl+F / typing goes to its search field, if it has one. */
  focus?(): void;
}

const TABS: readonly { id: ToolId; icon: string; label: Msg }[] = [
  { id: "timers", icon: ICONS.timer, label: tl("Timers") },
  { id: "clipboard", icon: ICONS.clipboard, label: tl("Clipboard") },
  { id: "shelf", icon: ICONS.shelf, label: tl("Shelf") },
  { id: "controls", icon: ICONS.sliders, label: tl("Controls") },
  { id: "calendar", icon: ICONS.calendar, label: tl("Calendar") },
  { id: "weather", icon: ICONS.cloudSun, label: tl("Weather") },
];

/** The timer icon is a filled one; the others are drawn as lines. */
const FILLED = new Set<ToolId>(["timers"]);

/** `resize`: the island takes its height again (Controls is taller). */
export function buildTools(actions: ViewActions, resize: () => void = () => {}): ViewHost {
  const panels: Record<ToolId, ToolPanel> = {
    timers: buildTimers(actions),
    clipboard: buildClipboard(actions),
    shelf: buildShelf(actions),
    controls: buildControls(actions),
    weather: buildWeather(actions),
    calendar: buildCalendar(actions),
  };
  const holder = h("div", { class: "tools-panel" });
  const buttons = new Map<ToolId, HTMLElement>();
  const tabs = h("div", { class: "tools-tabs", role: "tablist" });
  for (const tab of TABS) {
    const b = h(
      "button",
      {
        class: "tools-tab",
        role: "tab",
        title: tab.label,
        onclick: () => {
          if (Tools.tab === tab.id) return;
          actions.blip();
          Tools.tab = tab.id;
          State.notify();
        },
      },
      svg(tab.icon, 12, FILLED.has(tab.id) ? {} : { stroke: 2 }),
      h("span", { text: tab.label }),
    );
    buttons.set(tab.id, b);
    tabs.append(b);
  }
  const el = h("div", { class: "view tools" }, h("div", { class: "card" }, h("div", { class: "tools-body" }, tabs, holder)));

  let current: ToolId | null = null;
  let wasOn = false;

  return {
    el,
    sync() {
      const on = State.view === "tools" && State.mode === "expanded";
      if (Tools.wanted) {
        Tools.tab = Tools.wanted;
        Tools.wanted = null;
      }
      const id = Tools.tab;
      for (const [tab, b] of buttons) {
        b.classList.toggle("on", tab === id);
        b.setAttribute("aria-selected", String(tab === id));
      }
      const panel = panels[id];
      if (current !== id) {
        current = id;
        holder.replaceChildren(panel.el);
        if (on) {
          panel.shown?.();
          resize();
        }
      } else if (on && !wasOn) {
        panel.shown?.();
      }
      wasOn = on;
      panel.sync();
    },
    tick(nowMs: number) {
      return current ? panels[current].tick?.(nowMs) === true : false;
    },
    focus() {
      if (current) panels[current].focus?.();
    },
  };
}
