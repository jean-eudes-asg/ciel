// Today: Ciel's own card, the sun in the island's header, and what the island
// opens on with nothing running. A greeting, then a grid of tiles that each
// say one thing at a glance and open the tool behind it: the week with today
// lit and a dot on the days that hold something, the weather, the timer on
// its ring, the agents as their little clouds, what comes next, and what
// waits on the shelf or in the clipboard. For someone who doesn't code, it is
// the island's home.

import type { ToolId } from "../core/tools";
import { busyDays, dayKey, firstDayOfWeek, isoWeek, nextEvent, weekDays } from "../core/calendar";
import { Sessions, countSessions, type SessionRow } from "../core/sessions";
import { State, type AgentTask } from "../core/state";
import { msLeft, clock, ordered, progress } from "../core/timers";
import { SKY_TEXT, rainLine, skyOf, temp } from "../core/weather";
import { createMiniBot, pruneMiniBots } from "../cloud/minibots";
import { h, svg } from "./dom";
import { ICONS } from "./icons";
import type { ViewActions, ViewHost } from "./views";
import { N_, language, t, tl, tn } from "../i18n/i18n";
import { Tools } from "../tools/store";
import { events as calendarEvents } from "../tools/calendar";
import { dueText, timerName } from "../tools/timers";
import { weatherUnits } from "../tools/weather";
import { weatherIcon } from "../tools/weather-icon";

/** The greeting for the hour: an English key, shown with t(). */
export function greetingFor(hour: number): string {
  if (hour >= 5 && hour < 12) return N_("Good morning");
  if (hour >= 12 && hour < 18) return N_("Good afternoon");
  if (hour >= 18 && hour < 23) return N_("Good evening");
  return N_("Still up?");
}

/** The agents' tile in one line: who needs you first, then who works. */
export function agentsLine(rows: readonly SessionRow[]): string {
  const c = countSessions(rows);
  if (c.asking) return tn("{count} needs you", "{count} need you", c.asking);
  if (c.working) return t("{count} working", { count: c.working });
  if (c.error) return t("{count} stopped", { count: c.error });
  if (c.done) return t("{count} done", { count: c.done });
  return rows.length ? t("All quiet") : t("None running");
}

const RING_R = 13;
const RING_LEN = 2 * Math.PI * RING_R;

/** A ring that fills as a timer runs, with room inside for an icon. */
function ring(): { el: SVGSVGElement; set(part: number): void } {
  const ns = "http://www.w3.org/2000/svg";
  const el = document.createElementNS(ns, "svg");
  el.setAttribute("viewBox", "0 0 32 32");
  el.setAttribute("class", "today-ring");
  const circle = (cls: string) => {
    const c = document.createElementNS(ns, "circle");
    c.setAttribute("cx", "16");
    c.setAttribute("cy", "16");
    c.setAttribute("r", String(RING_R));
    c.setAttribute("class", cls);
    el.append(c);
    return c;
  };
  circle("track");
  const arc = circle("arc");
  arc.setAttribute("stroke-dasharray", String(RING_LEN));
  return {
    el,
    set(part) {
      arc.setAttribute("stroke-dashoffset", String(RING_LEN * (1 - Math.max(0, Math.min(1, part)))));
    },
  };
}

export function buildToday(actions: ViewActions): ViewHost {
  const hello = h("div", { class: "today-hello" });
  const open = (tool: ToolId) => {
    actions.blip();
    Tools.want(tool);
    actions.setView("tools");
  };
  // The date opens the month, as the desktop's clock does.
  const date = h("button", { class: "today-date", title: tl("Calendar"), onclick: () => open("calendar") });
  const ask = h("button", { class: "btn primary", onclick: () => actions.setView("prompt") }, h("span", { text: tl("Ask") }));
  const grid = h("div", { class: "today-grid" });
  const el = h("div", { class: "view today" },
    h("div", { class: "card" },
      h("div", { class: "today-body" },
        h("div", { class: "today-head" }, h("div", {}, hello, date), h("div", { class: "grow" }), h("div", { class: "today-buttons" }, ask)),
        grid,
      ),
    ),
  );

  /** A tile: something to look at, a line, a smaller line, and a door into its tool. */
  function tile(kind: string, go: () => void, visual: Node | null, main: string, sub = ""): HTMLElement {
    return h("button", { class: `today-tile ${kind}`, onclick: go },
      visual ? h("span", { class: "today-visual" }, visual) : null,
      h("span", { class: "today-text" }, h("b", { text: main }), sub ? h("span", { text: sub }) : null),
    );
  }

  /** The week: seven days, today lit, a dot under the days that hold something. */
  function weekTile(now: Date, lang: string): HTMLElement {
    const busy = busyDays(calendarEvents());
    const todayKey = dayKey(now);
    const letter = new Intl.DateTimeFormat(lang, { weekday: "narrow" });
    const days = weekDays(now, firstDayOfWeek(lang)).map((d) =>
      h("span", { class: `today-day${dayKey(d) === todayKey ? " on" : ""}${busy.has(dayKey(d)) ? " busy" : ""}` },
        h("i", { text: letter.format(d) }),
        h("b", { text: String(d.getDate()) }),
      ),
    );
    const month = new Intl.DateTimeFormat(lang, { month: "long" }).format(now);
    return h("button", { class: "today-tile week", onclick: () => open("calendar") },
      h("span", { class: "today-week-head" },
        h("b", { text: month }),
        h("span", { text: t("Week {n}", { n: isoWeek(now) }) }),
      ),
      h("span", { class: "today-days" }, ...days),
    );
  }

  // The timer's tile is kept: its ring and its time move every second.
  const timerRing = ring();
  const timerIcon = h("span", { class: "today-ring-icon" });
  const timerMain = h("b");
  const timerSub = h("span");
  const timerTile = h("button", { class: "today-tile timers", onclick: () => open("timers") },
    h("span", { class: "today-visual" }, timerRing.el, timerIcon),
    h("span", { class: "today-text" }, timerMain, timerSub),
  );
  let timerIconKey = "";
  let running = false;

  function syncTimer(nowMs: number) {
    const timers = ordered(Tools.timers, nowMs);
    const lead = timers[0];
    running = false;
    const iconKey = !lead ? "none" : lead.kind === "reminder" ? "bell" : "timer";
    if (iconKey !== timerIconKey) {
      timerIconKey = iconKey;
      timerIcon.replaceChildren(
        iconKey === "bell" ? svg(ICONS.bell2, 11, { stroke: 2 }) : iconKey === "timer" ? svg(ICONS.timer, 11) : h("span", { text: "+" }),
      );
    }
    timerTile.classList.toggle("ringing", !!lead?.ringing);
    if (!lead) {
      timerRing.set(0);
      timerMain.textContent = t("Timer");
      timerSub.textContent = t("Start");
      return;
    }
    const more = timers.length > 1 ? tn("and one more", "and {count} more", timers.length - 1) : "";
    timerRing.set(lead.ringing ? 1 : progress(lead, nowMs));
    if (lead.ringing) timerMain.textContent = t("Time's up");
    else if (lead.kind === "reminder") timerMain.textContent = dueText(lead.due);
    else {
      timerMain.textContent = clock(msLeft(lead, nowMs));
      running = lead.left == null;
    }
    timerSub.textContent = [timerName(lead), more].filter(Boolean).join(" · ");
  }

  /** The agents: their little clouds, each in its mood, and who needs you. */
  function agentsTile(rows: SessionRow[]): HTMLElement {
    const moods = { asking: "approval", working: "working", done: "finished", error: "error", idle: "idle" } as const;
    const clouds = h("span", { class: "today-clouds" });
    for (const row of rows.slice(0, 3)) {
      clouds.append(createMiniBot({
        id: `today:${row.key}`, name: row.agent, color: row.color, state: moods[row.phase],
        stepIndex: 0, steps: [], source: "agent", isIntegration: false,
      } as AgentTask, 17));
    }
    if (!rows.length) clouds.append(svg(ICONS.prompt, 15, { stroke: 2 }));
    return tile("agents", () => {
      actions.blip();
      actions.setView("agents");
    }, clouds, t("Agents"), agentsLine(rows));
  }

  let key = "";
  let lastSecond = -1;

  return {
    el,
    tick() {
      if (!running) return false;
      const second = Math.floor(Date.now() / 1000);
      if (second !== lastSecond) {
        lastSecond = second;
        syncTimer(Date.now());
      }
      return true;
    },
    sync() {
      const now = new Date();
      const nowMs = now.getTime();
      const lang = language();
      const minute = Math.floor(nowMs / 60_000);
      const w = Tools.weather;
      const waiting = State.pendingApproval;
      const rows = Sessions.rows(waiting ? { pillId: waiting.pillId, sessionId: waiting.sessionId } : null, nowMs);
      const list = calendarEvents();
      const next = nextEvent(list, nowMs);
      syncTimer(nowMs);

      const sig = [
        minute, lang, w?.at, Tools.prefs.weatherPlace, weatherUnits(),
        Tools.clip.items.length, Tools.prefs.clipboard, Tools.shelf.length,
        rows.map((r) => `${r.key}:${r.phase}:${r.color}`).join(","),
        list.length, next ? `${next.start}:${next.title}` : "",
      ].join("|");
      if (sig === key) return;
      key = sig;

      hello.textContent = t(greetingFor(now.getHours()));
      date.textContent = new Intl.DateTimeFormat(lang, { weekday: "long", day: "numeric", month: "long" }).format(now);

      const tiles: HTMLElement[] = [weekTile(now, lang)];

      if (w) {
        const sky = skyOf(w.now.code);
        const rain = rainLine(w, nowMs);
        const day = w.days[0];
        const range = day ? `${temp(day.max, weatherUnits())}° / ${temp(day.min, weatherUnits())}°` : w.place;
        tiles.push(tile("weather", () => open("weather"), weatherIcon(sky, w.now.isDay, 26),
          `${temp(w.now.temp, weatherUnits())}° · ${t(SKY_TEXT[sky])}`,
          rain ? t(rain.key, { minutes: rain.minutes }) : range));
      } else {
        tiles.push(tile("weather", () => open("weather"), weatherIcon("partly", true, 26), t("Weather"), t("Add your town")));
      }

      tiles.push(timerTile);
      tiles.push(agentsTile(rows));

      tiles.push(next
        ? tile("next", () => open("calendar"), svg(ICONS.calendar, 15, { stroke: 2 }), next.title || t("Event"), dueText(next.start))
        : tile("next", () => open("calendar"), svg(ICONS.calendar, 15, { stroke: 2 }), t("Nothing planned"), t("Your day is clear")));

      // What waits: the files on the shelf first, else what was copied, else
      // the shelf's own invitation.
      if (Tools.shelf.length) {
        tiles.push(tile("shelf", () => open("shelf"), svg(ICONS.shelf, 15, { stroke: 2 }), tn("One file on the shelf", "{count} files on the shelf", Tools.shelf.length), t("Drag it into any app")));
      } else if (Tools.prefs.clipboard && Tools.clip.items.length) {
        const pins = Tools.clip.items.filter((i) => i.pinned).length;
        tiles.push(tile("clipboard", () => open("clipboard"), svg(ICONS.clipboard, 15, { stroke: 2 }),
          tn("One thing copied", "{count} things copied", Tools.clip.items.length), pins ? tn("one pinned", "{count} pinned", pins) : ""));
      } else {
        tiles.push(tile("shelf", () => open("shelf"), svg(ICONS.shelf, 15, { stroke: 2 }), t("Shelf"), t("Drop files on Ciel")));
      }

      grid.replaceChildren(...tiles);
      pruneMiniBots();
    },
  };
}
