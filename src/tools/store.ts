// What the Tools know, kept up to date from Rust's events: the clipboard
// history, the shelf, timers, controls and the weather. Views read it; a
// change marks the island dirty (State.notify), nothing polls.

import { Bridge, onEvent } from "../core/bridge";
import { State } from "../core/state";
import type { ClipState, Controls, ShelfFile, TimerItem, ToolId, Weather } from "../core/tools";
import { DEFAULT_TOOL_PREFS, type ToolPrefs } from "../core/tools";

class ToolStore {
  clip: ClipState = { on: false, watching: false, items: [] };
  shelf: ShelfFile[] = [];
  timers: TimerItem[] = [];
  controls: Controls | null = null;
  weather: Weather | null = null;
  /** The tab the Tools view opens on: the last one used. */
  tab: ToolId = "timers";
  /** A tool asked from elsewhere (the Today card, a shortcut) to open on. */
  wanted: ToolId | null = null;
  /** A file is being dragged out of the shelf: the island must not swallow it back. */
  draggingOut = false;

  /** A timer or reminder rang (the island makes Ciel look up and chime). */
  onRang: ((t: TimerItem) => void) | null = null;

  private started = false;

  get prefs(): ToolPrefs {
    return { ...DEFAULT_TOOL_PREFS, ...(State.settings.tools ?? {}) };
  }

  /** Saves a change to the tools' settings (Rust follows: clipboard watch, weather place…). */
  setPrefs(change: Partial<ToolPrefs>) {
    State.settings.tools = { ...this.prefs, ...change };
    void Bridge.saveSettings(State.settings);
    State.notify();
  }

  start() {
    if (this.started) return;
    this.started = true;
    void onEvent<ClipState>("clipboard", (s) => {
      this.clip = s;
      State.notify();
    });
    void onEvent<TimerItem[]>("timers", (list) => {
      this.timers = list;
      State.notify();
    });
    void onEvent<TimerItem>("timer-rang", (t) => this.onRang?.(t));
    void onEvent<Controls>("controls", (c) => {
      this.controls = c;
      State.notify();
    });
    void onEvent<Weather>("weather", (w) => {
      this.weather = w;
      State.notify();
    });
    void Bridge.timersList().then((list) => {
      if (list) this.timers = list;
      State.notify();
    });
    void Bridge.clipboardState().then((s) => {
      if (s) this.clip = s;
    });
    void this.refreshWeather(false);
    // The microphones' state matters at once (the folded island says "Mic off").
    void this.refreshControls(true);
  }

  /** `light`: the sound and the microphones only, not the slow screens and radios. */
  async refreshControls(light = false) {
    const c = await Bridge.controlsState(light);
    if (c) {
      this.controls = c;
      State.notify();
    }
  }

  async refreshShelf() {
    const list = await Bridge.shelfList();
    if (list) {
      this.shelf = list;
      State.notify();
    }
  }

  async refreshWeather(force: boolean) {
    if (!this.prefs.weatherPlace) {
      this.weather = null;
      return;
    }
    const w = await Bridge.weatherNow(force);
    if (w) {
      this.weather = w;
      State.notify();
    }
  }

  /** Opens the Tools view on `tool` (the island's own setView does the rest). */
  want(tool: ToolId) {
    this.wanted = tool;
    this.tab = tool;
  }
}

export const Tools = new ToolStore();
