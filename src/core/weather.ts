// The weather as Ciel says it (Tools → Weather, the Today card, the sky).
// Pure: WMO codes from Open-Meteo, units, and the lines about rain.

import { N_ } from "../i18n/i18n";
import type { Weather } from "./tools";

export type Sky = "clear" | "partly" | "cloudy" | "fog" | "drizzle" | "rain" | "snow" | "storm";

/** The WMO weather code's kind of sky. */
export function skyOf(code: number): Sky {
  if (code <= 1) return "clear";
  if (code === 2) return "partly";
  if (code === 3) return "cloudy";
  if (code === 45 || code === 48) return "fog";
  if (code >= 51 && code <= 57) return "drizzle";
  if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return "rain";
  if ((code >= 71 && code <= 77) || code === 85 || code === 86) return "snow";
  if (code >= 95) return "storm";
  return "cloudy";
}

/** English keys, shown with t(). */
export const SKY_TEXT: Record<Sky, string> = {
  clear: N_("Clear"),
  partly: N_("Partly cloudy"),
  cloudy: N_("Cloudy"),
  fog: N_("Fog"),
  drizzle: N_("Drizzle"),
  rain: N_("Rain"),
  snow: N_("Snow"),
  storm: N_("Thunderstorm"),
};

/** The places that write temperatures in °F. */
const FAHRENHEIT = new Set(["US", "LR", "MM", "BS", "BZ", "KY", "PW", "FM", "MH"]);

/** °C or °F: as picked, else as the region of the system's language writes them. */
export function unitsFor(pref: "auto" | "c" | "f", locales: readonly string[]): "c" | "f" {
  if (pref !== "auto") return pref;
  for (const tag of locales) {
    const region = /[-_]([A-Za-z]{2})\b/.exec(tag)?.[1]?.toUpperCase();
    if (region) return FAHRENHEIT.has(region) ? "f" : "c";
  }
  return "c";
}

export function temp(celsius: number, units: "c" | "f"): number {
  return Math.round(units === "f" ? celsius * 1.8 + 32 : celsius);
}

/** km/h, or mph with °F. */
export function wind(kmh: number, units: "c" | "f"): { value: number; unit: string } {
  return units === "f" ? { value: Math.round(kmh / 1.609), unit: "mph" } : { value: Math.round(kmh), unit: "km/h" };
}

/** What to say about rain, if anything: when it starts, or when it stops. */
export function rainLine(w: Weather, now: number): { key: string; minutes: number } | null {
  if (w.raining) {
    if (w.rainUntil == null) return { key: N_("Rain for a while"), minutes: 0 };
    return { key: N_("Rain stops in {minutes} min"), minutes: Math.max(1, Math.round((w.rainUntil - now) / 60_000)) };
  }
  if (w.rainAt != null && w.rainAt - now < 2 * 3_600_000) {
    const minutes = Math.round((w.rainAt - now) / 60_000);
    return minutes <= 1 ? { key: N_("Rain starting now"), minutes: 0 } : { key: N_("Rain in {minutes} min"), minutes };
  }
  return null;
}

/** It is old news: more than three hours since Open-Meteo answered. */
export function stale(w: Weather, now: number): boolean {
  return now - w.at > 3 * 3_600_000;
}

/**
 * The sky wallpaper's theme for the weather outside, when it is worth
 * showing: grey for cloud, fog, drizzle, light rain or snow, the storm for
 * heavy rain and thunder. A clear sky leaves the theme to Ciel's mood and
 * the hour (sky/scenes.ts).
 */
export function themeForWeather(code: number): "cloudy" | "storm" | null {
  const sky = skyOf(code);
  if (sky === "storm") return "storm";
  if (sky === "rain") return [63, 65, 66, 67, 81, 82].includes(code) ? "storm" : "cloudy";
  if (sky === "cloudy" || sky === "fog" || sky === "drizzle" || sky === "snow") return "cloudy";
  return null;
}
