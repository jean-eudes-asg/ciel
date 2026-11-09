// The folded island's live activity (core/live.ts picks which): in place of
// the clock, a timer's ring counting down, "mic off" in red, rain about to
// start, a cup while the computer is kept awake. A click opens its tool.

import { pickLive, ticks, type Live } from "../core/live";
import { clock, progress } from "../core/timers";
import type { ToolId } from "../core/tools";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import { t } from "../i18n/i18n";
import { Tools } from "../tools/store";
import { timerName } from "../tools/timers";
import { Lyrics } from "../tools/lyrics";
import { Spotify } from "../core/spotify";
import { YtMusic } from "../core/ytmusic";

export interface LiveChip {
  el: HTMLElement;
  /** What shows now, or null. */
  readonly live: Live | null;
  /** Shows it (or not) and keeps its countdown going while it does. */
  sync(on: boolean): void;
}

const RING = 2 * Math.PI * 7;

export function buildLiveChip(open: (tool: ToolId) => void): LiveChip {
  const ring = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  ring.setAttribute("viewBox", "0 0 18 18");
  ring.setAttribute("width", "16");
  ring.setAttribute("height", "16");
  ring.classList.add("live-ring");
  ring.innerHTML =
    `<circle cx="9" cy="9" r="7" fill="none" stroke="rgba(255,255,255,0.18)" stroke-width="2.4"/>` +
    `<circle class="arc" cx="9" cy="9" r="7" fill="none" stroke-width="2.4" stroke-linecap="round" transform="rotate(-90 9 9)" stroke-dasharray="${RING}" stroke-dashoffset="${RING}"/>`;
  const arc = ring.querySelector(".arc") as SVGCircleElement;
  const icon = h("span", { class: "live-icon" });
  const text = h("span", { class: "live-text" });
  const el = h("button", { id: "island-live", class: "live-chip" }, icon, text);
  let current: Live | null = null;
  let timer = 0;
  let shown = false;

  el.addEventListener("click", (e) => {
    e.stopPropagation();
    if (!current) return;
    if (current.kind === "lyric") return;
    open(current.kind === "mic" || current.kind === "awake" ? "controls" : current.kind === "rain" ? "weather" : "timers");
  });

  function paint(live: Live) {
    el.className = `live-chip ${live.kind}${live.kind === "timer" && live.timer.kind === "focus" ? ` ${live.timer.phase}` : ""}`;
    switch (live.kind) {
      case "mic":
        icon.replaceChildren(svg(ICONS.micOff, 12, { stroke: 2.2 }));
        text.textContent = t("Mic off");
        break;
      case "rang":
        icon.replaceChildren(svg(live.timer.kind === "reminder" ? ICONS.bell2 : ICONS.timer, 12, live.timer.kind === "reminder" ? { stroke: 2.2 } : {}));
        text.textContent = live.timer.kind === "reminder" ? timerName(live.timer) : t("Time's up");
        break;
      case "timer": {
        const now = Date.now();
        arc.setAttribute("stroke-dashoffset", String(RING * (1 - progress(live.timer, now))));
        icon.replaceChildren(ring);
        const left = clock(Math.max(0, live.timer.left ?? live.timer.due - now));
        text.textContent = live.timer.left != null ? `${left} ‖` : left;
        break;
      }
      case "rain":
        icon.replaceChildren(svg(ICONS.cloudSun, 12, { stroke: 2 }));
        text.textContent = live.minutes <= 1 ? t("Rain now") : t("Rain in {minutes} min", { minutes: live.minutes });
        break;
      case "awake":
        icon.replaceChildren(svg(ICONS.coffee, 12, { stroke: 2 }));
        text.textContent = t("Awake");
        break;
      case "lyric":
        icon.replaceChildren(svg(ICONS.musicNote, 11));
        if (text.textContent !== live.text) text.textContent = live.text;
        break;
    }
    el.title = live.kind === "timer" || live.kind === "rang" ? timerName(live.timer) : "";
  }

  function sync(on: boolean) {
    const playing = Spotify.state.playing ? Spotify.state : YtMusic.state.playing ? YtMusic.state : null;
    // The next song's words are asked here too: the music card that asks
    // for them is only there while the island is open.
    if (playing && on) Lyrics.want(playing);
    const lyric = playing ? Lyrics.current(playing) : null;
    current = on ? pickLive({ timers: Tools.timers, controls: Tools.controls, weather: Tools.weather, now: Date.now(), lyric }) : null;
    const visible = current != null;
    if (visible) paint(current!);
    // Out of sight, no class keeps an animation (the ringing shake) going.
    else el.className = "live-chip";
    if (visible !== shown) {
      shown = visible;
      el.classList.toggle("on", visible);
    }
    el.style.opacity = visible ? "1" : "0";
    el.style.pointerEvents = visible ? "auto" : "none";
    // A countdown moves each second, and only while it shows.
    window.clearTimeout(timer);
    // Between two lines (a song's intro, a solo) nothing shows, but the next line will.
    if ((visible && current?.kind === "lyric") || (on && !visible && lyric === "")) timer = window.setTimeout(() => sync(true), 400);
    else if (visible && ticks(current)) timer = window.setTimeout(() => sync(true), 1000 - (Date.now() % 1000) + 5);
    else if (visible && current?.kind === "rain") timer = window.setTimeout(() => sync(true), 60_000);
  }

  return {
    el,
    get live() {
      return current;
    },
    sync,
  };
}
