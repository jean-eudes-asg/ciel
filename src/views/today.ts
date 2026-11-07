// The open island with nothing running (the "empty" view): Ciel's Today
// card. A greeting and the date, then what the Tools know at a glance — the
// weather, the next timer or reminder, what waits on the shelf or in the
// clipboard — each a door into its tool. For someone who doesn't code, it is
// the island's home.

import type { ToolId } from "../core/tools";
import { msLeft, clock, ordered } from "../core/timers";
import { SKY_TEXT, rainLine, skyOf, temp } from "../core/weather";
import { h, svg } from "./dom";
import { ICONS } from "./icons";
import type { ViewActions, ViewHost } from "./views";
import { N_, language, t, tl, tn } from "../i18n/i18n";
import { Tools } from "../tools/store";
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

export function buildToday(actions: ViewActions): ViewHost {
  const hello = h("div", { class: "today-hello" });
  const date = h("div", { class: "today-date" });
  const glances = h("div", { class: "today-glances" });
  const open = (tool: ToolId) => {
    actions.blip();
    Tools.want(tool);
    actions.setView("tools");
  };
  const buttons = h("div", { class: "today-buttons" },
    h("button", { class: "btn secondary", onclick: () => open("timers") }, svg(ICONS.timer, 11), h("span", { text: tl("Timer") })),
    h("button", { class: "btn primary", onclick: () => actions.setView("prompt") }, h("span", { text: tl("Ask") })),
  );
  const el = h("div", { class: "view today" },
    h("div", { class: "card" },
      h("div", { class: "today-body" },
        h("div", { class: "today-head" }, h("div", {}, hello, date), h("div", { class: "grow" }), buttons),
        glances,
      ),
    ),
  );

  function glance(tool: ToolId, icon: Node, main: string, sub = ""): HTMLElement {
    return h("button", { class: `today-glance ${tool}`, onclick: () => open(tool) },
      h("span", { class: "today-glance-icon" }, icon),
      h("span", { class: "today-glance-text" }, h("b", { text: main }), sub ? h("span", { text: sub }) : null),
    );
  }

  let key = "";

  return {
    el,
    sync() {
      const now = new Date();
      const lang = language();
      const minute = Math.floor(now.getTime() / 60_000);
      const w = Tools.weather;
      const timers = ordered(Tools.timers, now.getTime());
      const next = [
        minute, lang, w?.at, timers.map((x) => `${x.id}:${x.ringing}:${x.left != null}`).join(","),
        Tools.clip.items.length, Tools.prefs.clipboard, Tools.shelf.length, Tools.prefs.weatherPlace, weatherUnits(),
      ].join("|");
      if (next === key) return;
      key = next;
      hello.textContent = t(greetingFor(now.getHours()));
      date.textContent = new Intl.DateTimeFormat(lang, { weekday: "long", day: "numeric", month: "long" }).format(now);

      const list: HTMLElement[] = [];
      if (w) {
        const sky = skyOf(w.now.code);
        const rain = rainLine(w, now.getTime());
        list.push(glance("weather", weatherIcon(sky, w.now.isDay, 22), `${temp(w.now.temp, weatherUnits())}° · ${t(SKY_TEXT[sky])}`, rain ? t(rain.key, { minutes: rain.minutes }) : w.place));
      } else if (!Tools.prefs.weatherPlace) {
        list.push(glance("weather", weatherIcon("partly", true, 22), t("Weather"), t("Add your town")));
      }
      const lead = timers[0];
      if (lead) {
        const when = lead.ringing
          ? t("Time's up")
          : lead.kind === "reminder"
            ? dueText(lead.due)
            : clock(msLeft(lead, now.getTime()));
        const more = timers.length > 1 ? tn("and one more", "and {count} more", timers.length - 1) : "";
        list.push(glance("timers", svg(lead.kind === "reminder" ? ICONS.bell2 : ICONS.timer, 15, lead.kind === "reminder" ? { stroke: 2 } : {}), `${timerName(lead)} · ${when}`, more));
      }
      if (Tools.shelf.length) {
        list.push(glance("shelf", svg(ICONS.shelf, 15, { stroke: 2 }), tn("One file on the shelf", "{count} files on the shelf", Tools.shelf.length), t("Drag it into any app")));
      }
      if (Tools.prefs.clipboard && Tools.clip.items.length) {
        const pins = Tools.clip.items.filter((i) => i.pinned).length;
        list.push(glance("clipboard", svg(ICONS.clipboard, 15, { stroke: 2 }), tn("One thing copied", "{count} things copied", Tools.clip.items.length), pins ? tn("one pinned", "{count} pinned", pins) : ""));
      }
      if (!list.length || list.length < 2) {
        list.push(glance("controls", svg(ICONS.sliders, 15, { stroke: 2 }), t("Controls"), t("Sound, microphones, screen")));
      }
      glances.replaceChildren(...list.slice(0, 4));
    },
  };
}
