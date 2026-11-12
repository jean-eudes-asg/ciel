// Tools → Calendar, as the desktop's clock menu has it: the month with a dot
// on each day that has something, a day's events (the Calendar pill's iCal
// links, read by src-tauri/src/calendar), and world clocks, the cities
// searched on Open-Meteo as the weather's place is, for their time zone.
// The clocks change on the minute while the panel is on screen, never else.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import type { Place, WorldClock } from "../core/tools";
import { busyDays, dayKey, eventsOn, firstDayOfWeek, hoursApart, monthGrid, validZone, type CalendarEvent } from "../core/calendar";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import type { ViewActions } from "../views/views";
import { twelveHours } from "../views/clockformat";
import { language, t, tl } from "../i18n/i18n";
import { Tools } from "./store";
import type { ToolPanel } from "./view";

const CALENDAR_ID = "integration_calendar";

/** The Calendar pill's events for the month around today, as it last read them. */
export function events(): CalendarEvent[] {
  const month = State.integrations[CALENDAR_ID]?.data?.month;
  if (!Array.isArray(month)) return [];
  return month
    .map((e) => e as Record<string, unknown>)
    .map((e) => ({ title: String(e.title ?? ""), start: Number(e.start), end: Number(e.end), allDay: e.allDay === true }))
    .filter((e) => Number.isFinite(e.start) && Number.isFinite(e.end));
}

export function buildCalendar(actions: ViewActions): ToolPanel {
  const today = () => new Date();
  let shown = new Date(today().getFullYear(), today().getMonth(), 1);
  let picked = today();

  // ── The month ──
  const weekday = h("div", { class: "cal-weekday" });
  const fullDate = h("div", { class: "cal-full" });
  const monthName = h("button", { class: "cal-month", title: tl("Today"), onclick: () => {
    actions.blip();
    picked = today();
    shown = new Date(picked.getFullYear(), picked.getMonth(), 1);
    State.notify();
  } });
  const step = (by: number) => () => {
    actions.blip();
    shown = new Date(shown.getFullYear(), shown.getMonth() + by, 1);
    State.notify();
  };
  const head = h("div", { class: "cal-head" },
    h("button", { class: "ctl-round small", title: tl("Back"), onclick: step(-1) }, svg(ICONS.chevronLeft, 12, { stroke: 2.2 })),
    monthName,
    h("button", { class: "ctl-round small", title: tl("More"), onclick: step(1) }, svg(ICONS.chevronRight, 12, { stroke: 2.2 })),
  );
  const grid = h("div", { class: "cal-grid" });
  const month = h("div", { class: "cal-month-box" }, h("div", { class: "cal-top" }, weekday, fullDate), head, grid);

  // ── The day ──
  const dayTitle = h("div", { class: "cal-card-title" });
  const dayList = h("div", { class: "cal-events" });
  const day = h("div", { class: "cal-card" }, dayTitle, dayList);

  // ── World clocks ──
  const clockRows = h("div", { class: "cal-clocks" });
  const search = h("input", { class: "cal-search", type: "search", placeholder: t("Search a city"), spellcheck: "false" }) as HTMLInputElement;
  const found = h("div", { class: "cal-found" });
  const adding = h("div", { class: "cal-adding" }, search, found);
  let isAdding = false;
  const addButton = h("button", { class: "ctl-round small", title: tl("Add a city"), onclick: () => {
    actions.blip();
    isAdding = !isAdding;
    search.value = "";
    found.replaceChildren();
    State.notify();
    if (isAdding) window.setTimeout(() => search.focus(), 30);
  } }, svg(ICONS.plus, 11));
  const clocks = h("div", { class: "cal-card" },
    h("div", { class: "cal-card-title" }, h("span", { text: tl("World clocks") }), h("span", { class: "grow" }), addButton),
    adding, clockRows,
  );

  const side = h("div", { class: "cal-side" }, day, clocks);
  const el = h("div", { class: "tool tool-calendar" }, month, side);

  const setClocks = (list: WorldClock[]) => Tools.setPrefs({ worldClocks: list });

  let timer = 0;
  let seq = 0;
  search.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      isAdding = false;
      State.notify();
    }
  });
  search.addEventListener("input", () => {
    window.clearTimeout(timer);
    const query = search.value.trim();
    if (query.length < 2) {
      found.replaceChildren();
      return;
    }
    const mine = ++seq;
    timer = window.setTimeout(() => {
      Bridge.weatherSearch(query, language())
        .then((places: Place[]) => {
          if (mine !== seq) return;
          const usable = places.filter((p) => validZone(p.timezone));
          found.replaceChildren(...(usable.length ? usable.slice(0, 4).map((p) =>
            h("button", { class: "cal-place", onclick: () => {
              actions.blip();
              isAdding = false;
              setClocks([...Tools.prefs.worldClocks.filter((c) => c.name !== p.name || c.tz !== p.timezone), { name: p.name, tz: p.timezone }]);
            } }, h("b", { text: p.name }), h("span", { text: p.detail })))
            : [h("div", { class: "tool-hint", text: t("No place by that name.") })]));
        })
        .catch(() => {
          if (mine === seq) found.replaceChildren(h("div", { class: "tool-hint bad", text: t("Open-Meteo can't be reached right now.") }));
        });
    }, 350);
  });

  const lang = () => language();
  const hour12 = () => twelveHours(State.settings.clockFormat, lang());
  const timeText = (ms: number, tz?: string) =>
    new Intl.DateTimeFormat(lang(), { hour: "numeric", minute: "2-digit", hour12: hour12(), timeZone: tz }).format(new Date(ms));

  function paintMonth(busy: Set<string>) {
    const now = today();
    weekday.textContent = new Intl.DateTimeFormat(lang(), { weekday: "long" }).format(now);
    fullDate.textContent = new Intl.DateTimeFormat(lang(), { day: "numeric", month: "long", year: "numeric" }).format(now);
    monthName.textContent = new Intl.DateTimeFormat(lang(), { month: "long", year: shown.getFullYear() === now.getFullYear() ? undefined : "numeric" }).format(shown);
    const first = firstDayOfWeek(navigator.language || lang());
    const days = monthGrid(shown.getFullYear(), shown.getMonth(), first);
    const initial = new Intl.DateTimeFormat(lang(), { weekday: "narrow" });
    const names = days.slice(0, 7).map((d) => h("span", { class: "cal-dow", text: initial.format(d) }));
    const todayKey = dayKey(now);
    const pickedKey = dayKey(picked);
    grid.replaceChildren(...names, ...days.map((d) => {
      const key = dayKey(d);
      const weekend = d.getDay() === 0 || d.getDay() === 6;
      const cls = ["cal-day",
        d.getMonth() !== shown.getMonth() ? "out" : "",
        weekend ? "weekend" : "",
        key === todayKey ? "today" : "",
        key === pickedKey ? "picked" : "",
        busy.has(key) ? "busy" : ""].filter(Boolean).join(" ");
      return h("button", { class: cls, onclick: () => {
        actions.blip();
        picked = d;
        if (d.getMonth() !== shown.getMonth()) shown = new Date(d.getFullYear(), d.getMonth(), 1);
        State.notify();
      } }, h("span", { text: String(d.getDate()) }));
    }));
  }

  function paintDay(list: CalendarEvent[]) {
    const now = today();
    dayTitle.textContent = dayKey(picked) === dayKey(now)
      ? t("Today")
      : new Intl.DateTimeFormat(lang(), { weekday: "long", day: "numeric", month: "long" }).format(picked);
    const info = State.integrations[CALENDAR_ID];
    const on = State.settings.activeIntegrations?.includes(CALENDAR_ID) && info?.configured;
    const items = eventsOn(list, picked);
    if (!items.length) {
      dayList.replaceChildren(
        h("div", { class: "cal-none", text: t("No events") }),
        ...(on ? [] : [h("button", { class: "tool-link", text: t("Add your calendars"), onclick: () => {
          actions.blip();
          void Bridge.openSettingsWindow("integrations");
        } })]),
      );
      return;
    }
    const range = new Intl.DateTimeFormat(lang(), { hour: "numeric", minute: "2-digit", hour12: hour12() });
    dayList.replaceChildren(...items.map((e) => {
      const past = !e.allDay && e.end < now.getTime();
      const when = e.allDay ? t("All day") : range.formatRange(new Date(e.start), new Date(Math.max(e.end, e.start)));
      return h("div", { class: `cal-event${past ? " past" : ""}` },
        h("i", {}),
        h("span", { class: "cal-event-title", text: e.title || t("Meeting") }),
        h("span", { class: "cal-event-time", text: when }));
    }));
  }

  function paintClocks() {
    const now = Date.now();
    adding.style.display = isAdding ? "" : "none";
    addButton.classList.toggle("on", isAdding);
    const list = Tools.prefs.worldClocks.filter((c) => validZone(c.tz));
    clockRows.replaceChildren(...(list.length || isAdding ? list.map((c) => {
      const apart = hoursApart(c.tz, now);
      const otherDay = new Intl.DateTimeFormat("en-CA", { timeZone: c.tz }).format(now) !== dayKey(new Date(now));
      const weekday = otherDay ? `${new Intl.DateTimeFormat(lang(), { weekday: "short", timeZone: c.tz }).format(now)} ` : "";
      return h("div", { class: "cal-clock" },
        h("span", { class: "cal-clock-name", text: c.name }),
        h("span", { class: "cal-clock-time", text: `${weekday}${timeText(now, c.tz)}` }),
        h("span", { class: "cal-clock-apart", text: apart }),
        h("button", { class: "cal-clock-remove", title: tl("Remove"), onclick: () => {
          actions.blip();
          setClocks(Tools.prefs.worldClocks.filter((x) => x !== c && !(x.name === c.name && x.tz === c.tz)));
        } }, svg(ICONS.xmark, 9)));
    }) : [h("div", { class: "cal-none", text: t("Add the cities you work with.") })]));
  }

  let minuteTimer = 0;
  let key = "";
  return {
    el,
    shown() {
      picked = today();
      shown = new Date(picked.getFullYear(), picked.getMonth(), 1);
      isAdding = false;
    },
    sync() {
      const on = State.view === "tools" && State.mode === "expanded" && Tools.tab === "calendar";
      const list = events();
      const next = JSON.stringify([dayKey(shown), dayKey(picked), Math.floor(Date.now() / 60_000), lang(), State.settings.clockFormat,
        Tools.prefs.worldClocks, isAdding, list.length, list[0]?.start, list[list.length - 1]?.start, State.integrations[CALENDAR_ID]?.configured]);
      if (next !== key) {
        key = next;
        paintMonth(busyDays(list));
        paintDay(list);
        paintClocks();
      }
      // The next minute, while on screen: the clocks move on.
      window.clearTimeout(minuteTimer);
      if (on) minuteTimer = window.setTimeout(() => State.notify(), 60_000 - (Date.now() % 60_000) + 50);
    },
    focus() {
      if (isAdding) search.focus();
    },
  };
}
