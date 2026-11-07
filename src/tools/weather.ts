// Tools → Weather: now, the next hours and the week where you are, from
// Open-Meteo (free, no account). Nothing is asked until a place is picked
// here, and then only for that place (src-tauri/src/weather.rs).

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import type { Place, Weather } from "../core/tools";
import { SKY_TEXT, rainLine, skyOf, stale, temp, unitsFor, wind } from "../core/weather";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import { twelveHours } from "../views/clockformat";
import type { ViewActions } from "../views/views";
import { language, t, tl } from "../i18n/i18n";
import { Tools } from "./store";
import { ago, roundButton, toolSwitch, typingField } from "./ui";
import { weatherIcon } from "./weather-icon";
import type { ToolPanel } from "./view";

/** °C or °F for the settings and the system's language. */
export function weatherUnits(): "c" | "f" {
  const langs = typeof navigator !== "undefined" ? [...(navigator.languages ?? []), navigator.language ?? ""] : [];
  return unitsFor(Tools.prefs.weatherUnits, langs);
}

export function buildWeather(actions: ViewActions): ToolPanel {
  // ── Picking a place ──
  const search = typingField(h("input", {
    class: "tool-input",
    type: "search",
    spellcheck: "false",
    placeholder: tl("Your town or city"),
  }) as HTMLInputElement);
  const results = h("div", { class: "tool-list weather-places" });
  const cancelPick = h("button", { class: "tool-link", text: tl("Cancel"), onclick: () => {
    picking = false;
    State.notify();
  } });
  const picker = h("div", { class: "weather-pick" },
    h("div", { class: "tool-intro-title", text: tl("The weather where you are") }),
    h("div", { class: "tool-intro-text", text: tl("From Open-Meteo, free and without an account. Only the place you pick is ever sent.") }),
    h("div", { class: "tool-row" }, search, cancelPick),
    results,
  );
  let picking = false;
  let searchTimer = 0;
  let searchSeq = 0;
  search.addEventListener("input", () => {
    window.clearTimeout(searchTimer);
    const q = search.value.trim();
    if (q.length < 2) {
      results.replaceChildren();
      return;
    }
    searchTimer = window.setTimeout(() => {
      const seq = ++searchSeq;
      results.replaceChildren(h("div", { class: "tool-hint", text: t("Looking…") }));
      Bridge.weatherSearch(q, language())
        .then((places) => {
          if (seq !== searchSeq) return;
          results.replaceChildren(...(places.length ? places.map(placeRow) : [h("div", { class: "tool-hint", text: t("No place by that name.") })]));
        })
        .catch(() => {
          if (seq === searchSeq) results.replaceChildren(h("div", { class: "tool-hint bad", text: t("Open-Meteo can't be reached right now.") }));
        });
    }, 350);
  });

  function placeRow(p: Place): HTMLElement {
    return h("button", {
      class: "weather-place",
      onclick: () => {
        actions.blip();
        picking = false;
        search.value = "";
        results.replaceChildren();
        Tools.setPrefs({ weatherPlace: p.detail ? `${p.name}, ${p.detail.split(", ").pop()}` : p.name, weatherLat: p.lat, weatherLon: p.lon });
        Tools.weather = null;
        window.setTimeout(() => void Tools.refreshWeather(true), 300);
      },
    }, h("b", { text: p.name }), h("span", { text: p.detail }));
  }

  // ── The forecast ──
  const nowIcon = h("div", { class: "weather-now-icon" });
  const nowTemp = h("div", { class: "weather-temp" });
  const nowSky = h("div", { class: "weather-sky" });
  const nowMore = h("div", { class: "weather-more" });
  const rainBanner = h("div", { class: "weather-rain" });
  const hours = h("div", { class: "weather-hours" });
  const days = h("div", { class: "weather-days" });
  const place = h("button", { class: "tool-link weather-place-name", title: tl("Change the place"), onclick: () => {
    picking = true;
    State.notify();
    window.setTimeout(() => search.focus(), 50);
  } });
  const updated = h("span", { class: "tool-hint" });
  const credit = h("button", { class: "tool-link dim", text: "Open-Meteo", title: tl("Weather data by Open-Meteo.com (CC BY 4.0)"), onclick: () => void Bridge.openUrl("https://open-meteo.com/") });
  const unitsBtn = h("button", { class: "tool-chip small", title: tl("Units"), onclick: () => {
    actions.blip();
    Tools.setPrefs({ weatherUnits: weatherUnits() === "c" ? "f" : "c" });
  } });
  const rainSwitch = toolSwitch((on) => Tools.setPrefs({ rainWarnings: on }));
  const skySwitch = toolSwitch((on) => Tools.setPrefs({ skyWeather: on }));
  const refresh = roundButton(ICONS.arrowClockwise, tl("Refresh"), () => {
    actions.blip();
    void Tools.refreshWeather(true);
  });
  const foot = h("div", { class: "weather-foot" },
    place, updated, refresh, h("div", { class: "grow" }),
    h("label", { class: "tool-toggle", title: tl("A card when rain is about to start") }, h("span", { text: tl("Rain alerts") }), rainSwitch.el),
    h("label", { class: "tool-toggle", title: tl("Ciel's sky wallpaper wears the real weather") }, h("span", { text: tl("Sky") }), skySwitch.el),
    unitsBtn, credit,
  );
  const forecast = h("div", { class: "weather-forecast" },
    h("div", { class: "weather-top" },
      h("div", { class: "weather-now" }, nowIcon, h("div", {}, nowTemp, nowSky, nowMore)),
      h("div", { class: "weather-right" }, rainBanner, hours),
    ),
    days,
    foot,
  );
  const waiting = h("div", { class: "tool-empty", text: tl("Asking Open-Meteo…") });
  const el = h("div", { class: "tool tool-weather" }, picker, forecast, waiting);

  function hourText(ms: number): string {
    const lang = language();
    return new Intl.DateTimeFormat(lang, { hour: "numeric", hour12: twelveHours(State.settings.clockFormat, lang) }).format(new Date(ms));
  }

  let key = "";
  function paint(w: Weather) {
    const units = weatherUnits();
    const now = Date.now();
    const sky = skyOf(w.now.code);
    nowIcon.replaceChildren(weatherIcon(sky, w.now.isDay, 46));
    nowTemp.textContent = `${temp(w.now.temp, units)}°`;
    nowSky.textContent = t(SKY_TEXT[sky]);
    const wd = wind(w.now.wind, units);
    nowMore.textContent = t("Feels {temp}° · wind {wind} {unit}", { temp: temp(w.now.feels, units), wind: wd.value, unit: wd.unit });
    const rain = rainLine(w, now);
    rainBanner.style.display = rain ? "" : "none";
    if (rain) rainBanner.replaceChildren(svg(ICONS.cloudSun, 11, { stroke: 2 }), h("span", { text: t(rain.key, { minutes: rain.minutes }) }));
    hours.replaceChildren(...w.hours.slice(0, 12).map((x, i) =>
      h("div", { class: "weather-hour" },
        h("span", { class: "weather-hour-t", text: i === 0 ? t("Now") : hourText(x.t) }),
        weatherIcon(skyOf(x.code), x.isDay, 20),
        h("b", { text: `${temp(x.temp, units)}°` }),
        h("span", { class: "weather-pop", text: x.pop >= 20 ? `${Math.round(x.pop)}%` : "" }),
      ),
    ));
    const lo = Math.min(...w.days.map((d) => d.min));
    const hi = Math.max(...w.days.map((d) => d.max));
    const span = Math.max(1, hi - lo);
    const dayFmt = new Intl.DateTimeFormat(language(), { weekday: "short" });
    days.replaceChildren(...w.days.slice(0, 7).map((d, i) => {
      const bar = h("i", { class: "weather-bar" });
      bar.style.left = `${((d.min - lo) / span) * 100}%`;
      bar.style.right = `${((hi - d.max) / span) * 100}%`;
      return h("div", { class: "weather-day" },
        h("span", { class: "weather-day-t", text: i === 0 ? t("Today") : dayFmt.format(new Date(d.t + 12 * 3_600_000)) }),
        weatherIcon(skyOf(d.code), true, 16),
        h("span", { class: "weather-lo", text: `${temp(d.min, units)}°` }),
        h("span", { class: "weather-track" }, bar),
        h("span", { class: "weather-hi", text: `${temp(d.max, units)}°` }),
      );
    }));
    place.textContent = w.place;
    updated.textContent = ago(w.at, language());
    updated.classList.toggle("bad", stale(w, now));
    unitsBtn.textContent = units === "c" ? "°C" : "°F";
  }

  return {
    el,
    shown() {
      void Tools.refreshWeather(false);
    },
    sync() {
      const has = !!Tools.prefs.weatherPlace;
      const w = Tools.weather;
      const showPicker = !has || picking;
      picker.style.display = showPicker ? "" : "none";
      cancelPick.style.display = has ? "" : "none";
      forecast.style.display = !showPicker && w ? "" : "none";
      waiting.style.display = !showPicker && !w ? "" : "none";
      rainSwitch.set(Tools.prefs.rainWarnings);
      skySwitch.set(Tools.prefs.skyWeather);
      if (!w || showPicker) return;
      const next = `${w.at}|${weatherUnits()}|${language()}|${Math.floor(Date.now() / 60_000)}`;
      if (next === key) return;
      key = next;
      paint(w);
    },
    focus() {
      if (!Tools.prefs.weatherPlace || picking) search.focus();
    },
  };
}
