// Where the sun (and, roughly, the moon) is: NOAA's solar position algorithm
// (the one behind its solar calculator spreadsheet), the times of sunrise and
// sunset, the phase of the day the wallpaper draws, and a place to compute
// it for when the user gave none (a guess from the time zone, never a lookup).

import type { Phase } from "./prefs";

const RAD = Math.PI / 180;
const DEG = 180 / Math.PI;
const MINUTE = 60_000;
const DAY = 86_400_000;

const mod = (a: number, n: number) => ((a % n) + n) % n;
const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
/** An angle folded into (-180, 180]. */
export const wrap180 = (deg: number) => 180 - mod(180 - deg, 360);

export function julianDay(ms: number): number {
  return ms / DAY + 2440587.5;
}

/** The sun's declination (degrees) and the equation of time (minutes) at `jd`. */
function solarCore(jd: number): { decl: number; eqTime: number } {
  const T = (jd - 2451545) / 36525;
  const L0 = mod(280.46646 + T * (36000.76983 + T * 0.0003032), 360);
  const M = (357.52911 + T * (35999.05029 - 0.0001537 * T)) * RAD;
  const e = 0.016708634 - T * (0.000042037 + 0.0000001267 * T);
  const C =
    Math.sin(M) * (1.914602 - T * (0.004817 + 0.000014 * T)) +
    Math.sin(2 * M) * (0.019993 - 0.000101 * T) +
    Math.sin(3 * M) * 0.000289;
  const omega = (125.04 - 1934.136 * T) * RAD;
  const lambda = (L0 + C - 0.00569 - 0.00478 * Math.sin(omega)) * RAD;
  const eps0 = 23 + (26 + (21.448 - T * (46.815 + T * (0.00059 - T * 0.001813))) / 60) / 60;
  const eps = (eps0 + 0.00256 * Math.cos(omega)) * RAD;
  const decl = Math.asin(Math.sin(eps) * Math.sin(lambda)) * DEG;
  const y = Math.tan(eps / 2) ** 2;
  const L = L0 * RAD;
  const eqTime =
    4 * DEG *
    (y * Math.sin(2 * L) - 2 * e * Math.sin(M) + 4 * e * y * Math.sin(M) * Math.cos(2 * L) -
      0.5 * y * y * Math.sin(4 * L) - 1.25 * e * e * Math.sin(2 * M));
  return { decl, eqTime };
}

/** Elevation and azimuth (from north, clockwise) for a declination and an hour angle. */
function horizontal(lat: number, decl: number, hourAngle: number): { elevation: number; azimuth: number } {
  const phi = lat * RAD;
  const d = decl * RAD;
  const h = hourAngle * RAD;
  const cosZ = clamp(Math.sin(phi) * Math.sin(d) + Math.cos(phi) * Math.cos(d) * Math.cos(h), -1, 1);
  const elevation = 90 - Math.acos(cosZ) * DEG;
  const az = Math.atan2(Math.sin(h), Math.cos(h) * Math.sin(phi) - Math.tan(d) * Math.cos(phi)) * DEG + 180;
  return { elevation, azimuth: mod(az, 360) };
}

/** Atmospheric refraction near the horizon, in degrees (NOAA's approximation). */
function refraction(elevation: number): number {
  if (elevation > 85) return 0;
  const te = Math.tan(elevation * RAD);
  let arcsec: number;
  if (elevation > 5) arcsec = 58.1 / te - 0.07 / te ** 3 + 0.000086 / te ** 5;
  else if (elevation > -0.575) {
    const e = elevation;
    arcsec = 1735 + e * (-518.2 + e * (103.4 + e * (-12.79 + e * 0.711)));
  } else arcsec = -20.774 / te;
  return arcsec / 3600;
}

export interface SunPosition {
  /** Degrees above the horizon, refraction included. */
  elevation: number;
  /** Degrees from north, clockwise. */
  azimuth: number;
  /** Degrees; negative in the morning, positive in the afternoon. */
  hourAngle: number;
}

export function solarPosition(date: Date, lat: number, lon: number): SunPosition {
  const ms = date.getTime();
  const { decl, eqTime } = solarCore(julianDay(ms));
  const trueSolarMinutes = mod(mod(ms / MINUTE, 1440) + eqTime + 4 * lon, 1440);
  const hourAngle = trueSolarMinutes / 4 - 180;
  const { elevation, azimuth } = horizontal(lat, decl, hourAngle);
  return { elevation: elevation + refraction(elevation), azimuth, hourAngle };
}

export interface SunTimes {
  sunrise: Date | null;
  sunset: Date | null;
  noon: Date;
  /** The sun never sets ("day") or never rises ("night") on that day. */
  polar: "day" | "night" | null;
}

/**
 * Sunrise, solar noon and sunset of the solar day `date` falls in at that
 * longitude (independent of the computer's time zone). Within a minute or so
 * of the published times away from the poles.
 */
export function sunTimes(date: Date, lat: number, lon: number): SunTimes {
  const local = new Date(date.getTime() + (lon / 15) * 3_600_000);
  const day = Date.UTC(local.getUTCFullYear(), local.getUTCMonth(), local.getUTCDate());
  const at = (minutes: number) => solarCore(julianDay(day + minutes * MINUTE));
  let noon = 720 - 4 * lon - at(720 - 4 * lon).eqTime;
  noon = 720 - 4 * lon - at(noon).eqTime;
  let polar: SunTimes["polar"] = null;

  const event = (rising: boolean): number | null => {
    let t = noon;
    for (let i = 0; i < 3; i++) {
      const { decl, eqTime } = at(t);
      const phi = lat * RAD;
      const d = decl * RAD;
      const cosH = Math.cos(90.833 * RAD) / (Math.cos(phi) * Math.cos(d)) - Math.tan(phi) * Math.tan(d);
      if (cosH > 1) {
        polar = "night";
        return null;
      }
      if (cosH < -1) {
        polar = "day";
        return null;
      }
      const H = Math.acos(cosH) * DEG;
      t = 720 - 4 * lon - eqTime + (rising ? -4 * H : 4 * H);
    }
    return t;
  };
  const rise = event(true);
  const set = event(false);
  return {
    sunrise: rise == null ? null : new Date(day + rise * MINUTE),
    sunset: set == null ? null : new Date(day + set * MINUTE),
    noon: new Date(day + noon * MINUTE),
    polar,
  };
}

// ── The phase of the day ──────────────────────────────────────────────────────

/** Sun elevations (degrees) where the phases change. */
export const PHASE_LIMITS = {
  /** At or above: day. */
  day: 8,
  /** At or above (and below day): golden hour. */
  golden: 3,
  /** Evening, at or above (and below golden): sunset; below it, dusk. */
  sunset: -4,
  /** Below: night. */
  night: -10,
} as const;

/** The phase for a sun elevation; `rising` tells dawn from sunset and dusk. */
export function phaseOf(elevation: number, rising: boolean): Phase {
  if (elevation >= PHASE_LIMITS.day) return "day";
  if (elevation >= PHASE_LIMITS.golden) return "golden";
  if (elevation < PHASE_LIMITS.night) return "night";
  if (rising) return "dawn";
  return elevation >= PHASE_LIMITS.sunset ? "sunset" : "dusk";
}

// ── The moon (roughly) ────────────────────────────────────────────────────────

const SYNODIC_MONTH = 29.530588853;
/** A new moon: 2000-01-06 18:14 UTC. */
const NEW_MOON_JD = 2451550.1;

/** 0 = new moon, 0.25 = first quarter, 0.5 = full, 0.75 = last quarter. */
export function moonPhase(date: Date): number {
  return mod((julianDay(date.getTime()) - NEW_MOON_JD) / SYNODIC_MONTH, 1);
}

/**
 * Where the moon is, near enough for a picture (a few degrees): its hour
 * angle trails the sun's by its elongation, and its declination is the sun's
 * on the day the sun stands where the moon is on the ecliptic.
 */
export function moonPosition(date: Date, lat: number, lon: number): SunPosition & { phase: number } {
  const phase = moonPhase(date);
  const sun = solarPosition(date, lat, lon);
  const hourAngle = wrap180(sun.hourAngle - phase * 360);
  const { decl } = solarCore(julianDay(date.getTime() + phase * 365.2422 * DAY));
  const { elevation, azimuth } = horizontal(lat, decl, hourAngle);
  return { elevation, azimuth, hourAngle, phase };
}

// ── Where the user is, when they didn't say ───────────────────────────────────

/** Approximate coordinates for common time zones (a city near their middle). */
const ZONES: Record<string, readonly [number, number]> = {
  "America/New_York": [40.71, -74.01],
  "America/Detroit": [42.33, -83.05],
  "America/Toronto": [43.65, -79.38],
  "America/Montreal": [45.5, -73.57],
  "America/Halifax": [44.65, -63.57],
  "America/Chicago": [41.88, -87.63],
  "America/Winnipeg": [49.9, -97.14],
  "America/Denver": [39.74, -104.99],
  "America/Edmonton": [53.55, -113.49],
  "America/Phoenix": [33.45, -112.07],
  "America/Los_Angeles": [34.05, -118.24],
  "America/Vancouver": [49.28, -123.12],
  "America/Anchorage": [61.22, -149.9],
  "Pacific/Honolulu": [21.31, -157.86],
  "America/Mexico_City": [19.43, -99.13],
  "America/Guatemala": [14.63, -90.51],
  "America/Havana": [23.11, -82.37],
  "America/Bogota": [4.71, -74.07],
  "America/Lima": [-12.05, -77.04],
  "America/Caracas": [10.48, -66.9],
  "America/Santiago": [-33.45, -70.67],
  "America/Sao_Paulo": [-23.55, -46.63],
  "America/Argentina/Buenos_Aires": [-34.6, -58.38],
  "America/Buenos_Aires": [-34.6, -58.38],
  "America/Montevideo": [-34.9, -56.16],
  "Europe/London": [51.51, -0.13],
  "Europe/Dublin": [53.35, -6.26],
  "Europe/Lisbon": [38.72, -9.14],
  "Europe/Madrid": [40.42, -3.7],
  "Europe/Paris": [48.86, 2.35],
  "Europe/Brussels": [50.85, 4.35],
  "Europe/Amsterdam": [52.37, 4.9],
  "Europe/Luxembourg": [49.61, 6.13],
  "Europe/Zurich": [47.37, 8.54],
  "Europe/Berlin": [52.52, 13.4],
  "Europe/Rome": [41.9, 12.5],
  "Europe/Vienna": [48.21, 16.37],
  "Europe/Prague": [50.08, 14.44],
  "Europe/Warsaw": [52.23, 21.01],
  "Europe/Budapest": [47.5, 19.04],
  "Europe/Copenhagen": [55.68, 12.57],
  "Europe/Oslo": [59.91, 10.75],
  "Europe/Stockholm": [59.33, 18.07],
  "Europe/Helsinki": [60.17, 24.94],
  "Europe/Athens": [37.98, 23.73],
  "Europe/Bucharest": [44.43, 26.1],
  "Europe/Istanbul": [41.01, 28.98],
  "Europe/Kiev": [50.45, 30.52],
  "Europe/Kyiv": [50.45, 30.52],
  "Europe/Moscow": [55.76, 37.62],
  "Africa/Casablanca": [33.57, -7.59],
  "Africa/Algiers": [36.75, 3.06],
  "Africa/Tunis": [36.8, 10.18],
  "Africa/Cairo": [30.04, 31.24],
  "Africa/Dakar": [14.72, -17.47],
  "Africa/Abidjan": [5.36, -4.01],
  "Africa/Lagos": [6.52, 3.38],
  "Africa/Kinshasa": [-4.44, 15.27],
  "Africa/Nairobi": [-1.29, 36.82],
  "Africa/Johannesburg": [-26.2, 28.05],
  "Asia/Jerusalem": [31.77, 35.21],
  "Asia/Beirut": [33.89, 35.5],
  "Asia/Baghdad": [33.31, 44.36],
  "Asia/Riyadh": [24.71, 46.68],
  "Asia/Tehran": [35.69, 51.39],
  "Asia/Dubai": [25.2, 55.27],
  "Asia/Karachi": [24.86, 67.0],
  "Asia/Tashkent": [41.3, 69.24],
  "Asia/Almaty": [43.24, 76.95],
  // India is one zone two thousand kilometres wide: somewhere near its middle.
  "Asia/Kolkata": [21.15, 79.09],
  "Asia/Calcutta": [21.15, 79.09],
  "Asia/Kathmandu": [27.72, 85.32],
  "Asia/Dhaka": [23.81, 90.41],
  "Asia/Yangon": [16.87, 96.2],
  "Asia/Bangkok": [13.76, 100.5],
  "Asia/Ho_Chi_Minh": [10.82, 106.63],
  "Asia/Saigon": [10.82, 106.63],
  "Asia/Jakarta": [-6.21, 106.85],
  "Asia/Singapore": [1.35, 103.82],
  "Asia/Kuala_Lumpur": [3.14, 101.69],
  "Asia/Manila": [14.6, 120.98],
  "Asia/Shanghai": [31.23, 121.47],
  "Asia/Hong_Kong": [22.32, 114.17],
  "Asia/Taipei": [25.03, 121.57],
  "Asia/Seoul": [37.57, 126.98],
  "Asia/Tokyo": [35.68, 139.65],
  "Australia/Perth": [-31.95, 115.86],
  "Australia/Adelaide": [-34.93, 138.6],
  "Australia/Brisbane": [-27.47, 153.03],
  "Australia/Sydney": [-33.87, 151.21],
  "Australia/Melbourne": [-37.81, 144.96],
  "Pacific/Auckland": [-36.85, 174.76],
};

/** A latitude that makes sense for a zone's region, when the zone isn't in the table. */
function regionLatitude(zone: string): number {
  const southAmerica = /^America\/(Argentina|Sao_Paulo|Santiago|Lima|La_Paz|Asuncion|Montevideo|Bahia|Recife|Fortaleza|Belem|Manaus|Cuiaba|Campo_Grande|Porto_Velho|Rio_Branco|Punta_Arenas|Guayaquil)/;
  if (southAmerica.test(zone)) return -20;
  const region = zone.split("/")[0];
  switch (region) {
    case "Europe": return 48;
    case "America": return 38;
    case "Asia": return 30;
    case "Africa": return 5;
    case "Australia": return -30;
    case "Pacific": return -15;
    case "Indian": return -15;
    case "Atlantic": return 35;
    case "Antarctica": return -75;
    case "Arctic": return 78;
    default: return 40;
  }
}

/** The standard (winter-time) offset of the computer's zone, minutes east of UTC. */
function standardOffset(year: number): number {
  const jan = -new Date(year, 0, 1).getTimezoneOffset();
  const jul = -new Date(year, 6, 1).getTimezoneOffset();
  return Math.min(jan, jul);
}

export interface Place {
  lat: number;
  lon: number;
  /** True when it comes from the time zone, not from the user. */
  guessed: boolean;
  zone: string;
}

/**
 * Coordinates from an IANA time zone: the table above, else the longitude of
 * the zone's standard offset (15° an hour) and a latitude for its region.
 */
export function guessLocation(zone: string, offsetMinutes: number): { lat: number; lon: number } {
  const known = ZONES[zone];
  if (known) return { lat: known[0], lon: known[1] };
  return { lat: regionLatitude(zone), lon: clamp(offsetMinutes / 4, -180, 180) };
}

/** The computer's time zone, as Intl knows it ("" if it doesn't). */
export function systemZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
  } catch {
    return "";
  }
}

/** The place to compute the sky for: the user's coordinates, or a guess. */
export function placeFor(lat: number | null, lon: number | null, date = new Date()): Place {
  const zone = systemZone();
  if (lat != null && lon != null) return { lat, lon, guessed: false, zone };
  const g = guessLocation(zone, standardOffset(date.getFullYear()));
  return { lat: lat ?? g.lat, lon: lon ?? g.lon, guessed: true, zone };
}
