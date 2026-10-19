// Notch → Wallpaper: Ciel's sky on the desktop and the lock screen. Off by
// default; everything here only changes settings.json, and the island does
// the rest (island/wallpaper.ts). Photos are downloaded only from the button,
// with their credits listed under their theme.

import { Bridge, emitToWindow, onEvent, type WallpaperPhoto, type WallpaperStatus } from "../core/bridge";
import type { BotStateName } from "../core/layout";
import { h, clear } from "../views/dom";
import { t, tn } from "../i18n/i18n";
import { toggle } from "../hub/ui";
import { MOODS, THEMES, wallpaperPrefs, type Theme, type WallpaperPrefs } from "../sky/prefs";
import { sceneSpec } from "../sky/scenes";
import { placeFor, sunTimes } from "../sky/sun";
import { SkyRenderer } from "../sky/render";
import { WALLPAPER_EVENTS, type LiveScene } from "../sky/live";

/** How the page reads and saves the preferences (settings.json `wallpaper`). */
export interface WallpaperStore {
  get(): unknown;
  set(next: WallpaperPrefs): void;
}

/** The names shown for the themes and the moods. */
export function themeName(theme: Theme): string {
  switch (theme) {
    case "clear": return t("Clear sky");
    case "cloudy": return t("Cloudy");
    case "golden": return t("Golden hour");
    case "sunset": return t("Sunset");
    case "night": return t("Night");
    case "space": return t("Space");
    case "storm": return t("Storm");
  }
}

export function moodName(mood: BotStateName): string {
  switch (mood) {
    case "idle": return t("Idle");
    case "working": return t("Working");
    case "thinking": return t("Thinking");
    case "searching": return t("Searching");
    case "approval": return t("Waiting for approval");
    case "question": return t("Asking a question");
    case "error": return t("Error");
    case "finished": return t("Finished");
    case "ratelimit": return t("Rate limited");
    case "sleeping": return t("Sleeping");
    case "dizzy": return t("Dizzy");
  }
}

const ALL_SUPPORTED: WallpaperStatus = {
  supported: { desktop: true, lockScreen: true, live: true }, live: false, paused: false, saved: false,
};

/** The page's sections. */
export async function wallpaperSections(store: WallpaperStore): Promise<HTMLElement[]> {
  const status = (await Bridge.wallpaperStatus()) ?? ALL_SUPPORTED;
  const monitors = (await Bridge.wallpaperMonitors()) ?? [];
  const primary = monitors.find((m) => m.primary) ?? monitors[0];
  const aspect = primary ? primary.width / primary.height : 16 / 9;
  const prefs = () => wallpaperPrefs(store.get());
  // A slider being dragged shows in the preview before it is saved.
  const draft: Partial<WallpaperPrefs> = {};
  const change = (edit: (p: WallpaperPrefs) => void) => {
    const next = prefs();
    edit(next);
    for (const key of Object.keys(draft)) delete draft[key as keyof WallpaperPrefs];
    store.set(next);
    preview.redraw();
  };
  const preview = previewCanvas(() => ({ ...prefs(), ...draft }), aspect);
  return [
    mainSection(status, prefs, change, preview.el),
    locationSection(prefs, change),
    moodsSection(prefs, change),
    themesSection(prefs, change),
    cielSection(prefs, change, (key, value) => {
      draft[key] = value;
      preview.redraw();
    }),
  ];
}

// ── The preview ───────────────────────────────────────────────────────────────

/** A small live picture of the scene for Ciel's current mood, drawn while on screen. */
function previewCanvas(prefs: () => WallpaperPrefs, aspect: number) {
  const W = 360;
  const H = Math.round(W / aspect);
  const el = h("canvas", { class: "wallpaper-preview", width: W * 2, height: H * 2 }) as HTMLCanvasElement;
  el.style.cssText = `width:${W}px;height:${H}px;border-radius:10px;display:block;max-width:100%`;
  const renderer = new SkyRenderer();
  let mood: BotStateName = "idle";
  let frame: number | null = null;
  let last = 0;
  const draw = (now: number) => {
    frame = null;
    if (!el.isConnected && last > 0) return;
    if (document.visibilityState === "visible" && now - last > 1000 / 12) {
      last = now;
      const ctx = el.getContext("2d");
      if (ctx) renderer.render(ctx, sceneSpec(new Date(), mood, prefs()), performance.now() / 1000);
    }
    frame = requestAnimationFrame(draw);
  };
  const redraw = () => {
    if (frame == null) frame = requestAnimationFrame(draw);
  };
  // The island says which mood the wallpapers show.
  void onEvent<LiveScene>(WALLPAPER_EVENTS.scene, (s) => {
    if (s?.mood) mood = s.mood;
  });
  document.addEventListener("visibilitychange", redraw);
  redraw();
  return { el, redraw };
}

// ── Desktop, lock screen, live or still ───────────────────────────────────────

function mainSection(
  status: WallpaperStatus, prefs: () => WallpaperPrefs,
  change: (edit: (p: WallpaperPrefs) => void) => void, preview: HTMLElement,
): HTMLElement {
  const p = prefs();
  const sup = status.supported;
  const unsupported = () => h("span", { class: "tag", text: t("Not on this desktop") });

  const desktop = toggle(p.desktop && sup.desktop, (v) => change((x) => (x.desktop = v)));
  (desktop as HTMLButtonElement).disabled = !sup.desktop;
  const lock = toggle(p.lockScreen && sup.lockScreen, (v) => change((x) => (x.lockScreen = v)));
  (lock as HTMLButtonElement).disabled = !sup.lockScreen;

  const mode = h("select", {}) as HTMLSelectElement;
  mode.append(
    h("option", { value: "live", text: t("Animated") }),
    h("option", { value: "still", text: t("Still") }),
  );
  mode.value = p.live && sup.live ? "live" : "still";
  mode.disabled = !sup.live;
  mode.addEventListener("change", () => change((x) => (x.live = mode.value === "live")));

  const battery = toggle(p.pauseOnBattery, (v) => change((x) => (x.pauseOnBattery = v)));

  const feedback = h("span", { class: "hint" });
  const say = (text: string) => {
    feedback.textContent = text;
    window.setTimeout(() => {
      if (feedback.textContent === text) feedback.textContent = "";
    }, 3200);
  };
  const apply = h("button", {
    class: "primary",
    text: t("Apply now"),
    onclick: () => {
      void emitToWindow("island", WALLPAPER_EVENTS.apply);
      say(t("Updating the wallpaper…"));
    },
  });
  const restore = h("button", {
    text: t("Restore my previous wallpaper"),
    onclick: async () => {
      // Off first, so the island doesn't put Ciel's sky straight back.
      change((x) => {
        x.desktop = false;
        x.lockScreen = false;
      });
      (desktop as HTMLButtonElement).classList.remove("on");
      (lock as HTMLButtonElement).classList.remove("on");
      await Bridge.wallpaperRestore();
      say(t("Your previous wallpaper is back."));
    },
  });

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("Wallpaper") })),
    preview,
    h("div", { class: "hint", text: t("Nothing changes on your PC until you turn one of these on.") }),
    h("div", { class: "row" }, h("label", { text: t("Desktop") }), desktop, sup.desktop ? null : unsupported()),
    h("div", { class: "row" }, h("label", { text: t("Lock screen") }), lock, sup.lockScreen ? null : unsupported()),
    h("div", { class: "hint", text: t("The lock screen can't animate: Ciel appears there as a picture, refreshed with his mood.") }),
    h("div", { class: "row" }, h("label", { text: t("Mode") }), mode),
    h("div", {
      class: "hint",
      text: sup.live
        ? t("Animated pauses while a fullscreen app is in front and on battery.")
        : t("Animated isn't available on this desktop: still pictures are used."),
    }),
    h("div", { class: "row" }, h("label", { text: t("Pause on battery") }), battery),
    h("div", { class: "row" }, apply, restore, feedback),
  );
}

// ── Location ──────────────────────────────────────────────────────────────────

function locationSection(prefs: () => WallpaperPrefs, change: (edit: (p: WallpaperPrefs) => void) => void): HTMLElement {
  const p = prefs();
  const manual = p.lat != null && p.lon != null;
  const mode = h("select", {}) as HTMLSelectElement;
  mode.append(
    h("option", { value: "auto", text: t("Automatic (from your time zone)") }),
    h("option", { value: "manual", text: t("Coordinates") }),
  );
  mode.value = manual ? "manual" : "auto";
  const guess = placeFor(null, null);
  const lat = h("input", { type: "number", min: "-90", max: "90", step: "0.01", style: "width:96px" }) as HTMLInputElement;
  const lon = h("input", { type: "number", min: "-180", max: "180", step: "0.01", style: "width:96px" }) as HTMLInputElement;
  lat.value = String(p.lat ?? guess.lat);
  lon.value = String(p.lon ?? guess.lon);
  const coords = h("div", { class: "row" },
    h("label", { text: t("Latitude") }), lat,
    h("label", { text: t("Longitude"), style: "min-width:0" }), lon,
  );
  const today = h("div", { class: "hint" });
  const guessed = h("div", { class: "hint" });

  const showToday = () => {
    const now = prefs();
    const place = placeFor(now.lat, now.lon);
    const times = sunTimes(new Date(), place.lat, place.lon);
    const time = (d: Date) => d.toLocaleTimeString(document.documentElement.lang || undefined, { hour: "2-digit", minute: "2-digit" });
    let text: string;
    if (times.sunrise && times.sunset) text = t("Today: sunrise {rise} · sunset {set}", { rise: time(times.sunrise), set: time(times.sunset) });
    else text = times.polar === "day" ? t("Today the sun doesn't set.") : t("Today the sun doesn't rise.");
    today.textContent = text;
    guessed.textContent = place.guessed && place.zone ? t("Guessed from your time zone ({zone}).", { zone: place.zone }) : "";
    guessed.style.display = guessed.textContent ? "" : "none";
  };
  const save = () => {
    const a = Number(lat.value);
    const b = Number(lon.value);
    if (!Number.isFinite(a) || !Number.isFinite(b) || Math.abs(a) > 90 || Math.abs(b) > 180) return;
    change((x) => {
      x.lat = a;
      x.lon = b;
    });
    showToday();
  };
  lat.addEventListener("change", save);
  lon.addEventListener("change", save);
  mode.addEventListener("change", () => {
    coords.style.display = mode.value === "manual" ? "" : "none";
    if (mode.value === "manual") save();
    else change((x) => {
      x.lat = null;
      x.lon = null;
    });
    showToday();
  });
  coords.style.display = manual ? "" : "none";
  showToday();
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("Location") })),
    h("div", { class: "row" }, h("label", { text: t("Where you are") }), mode),
    coords,
    today,
    guessed,
  );
}

// ── Moods → themes ────────────────────────────────────────────────────────────

function moodsSection(prefs: () => WallpaperPrefs, change: (edit: (p: WallpaperPrefs) => void) => void): HTMLElement {
  const rows = MOODS.map((mood) => {
    const select = h("select", {}) as HTMLSelectElement;
    select.append(h("option", { value: "auto", text: t("Auto") }));
    for (const theme of THEMES) select.append(h("option", { value: theme, text: themeName(theme) }));
    select.value = prefs().moods[mood] ?? "auto";
    select.addEventListener("change", () => change((x) => {
      if (select.value === "auto") delete x.moods[mood];
      else x.moods[mood] = select.value as Theme;
    }));
    return h("div", { class: "row" }, h("label", { text: moodName(mood) }), select);
  });
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("Moods") })),
    h("div", { class: "hint", text: t("Each mood picks the weather. Auto follows the time of day.") }),
    ...rows,
  );
}

// ── Themes: drawn or photos ───────────────────────────────────────────────────

function themesSection(prefs: () => WallpaperPrefs, change: (edit: (p: WallpaperPrefs) => void) => void): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column;gap:12px" });
  for (const theme of THEMES) body.append(themeRow(theme, prefs, change));
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: t("Themes") })),
    h("div", {
      class: "hint",
      text: t("Drawn skies are made by Ciel. Photos are free pictures from NASA (space) and Wikimedia Commons, downloaded only when you ask."),
    }),
    body,
  );
}

function themeRow(theme: Theme, prefs: () => WallpaperPrefs, change: (edit: (p: WallpaperPrefs) => void) => void): HTMLElement {
  const source = h("select", {}) as HTMLSelectElement;
  source.append(
    h("option", { value: "drawn", text: t("Drawn") }),
    h("option", { value: "photos", text: t("Photos") }),
  );
  source.value = prefs().sources[theme] ?? "drawn";
  const count = h("span", { class: "hint" });
  const credits = h("div", { style: "display:flex;flex-direction:column;gap:4px" });
  const fetchBtn = h("button", { text: t("Get 10 photos") }) as HTMLButtonElement;
  const note = h("span", { class: "hint" });

  const show = async () => {
    const list = (await Bridge.wallpaperPhotos(theme)) ?? [];
    count.textContent = list.length ? tn("{count} photo", "{count} photos", list.length) : "";
    clear(credits);
    const usingPhotos = source.value === "photos";
    fetchBtn.style.display = usingPhotos ? "" : "none";
    if (!usingPhotos) return;
    if (list.length === 0) {
      credits.append(h("div", { class: "hint", text: t("No photos yet: the drawn sky is used.") }));
      return;
    }
    for (const photo of list) credits.append(creditLine(photo));
  };
  source.addEventListener("change", () => {
    change((x) => {
      if (source.value === "photos") x.sources[theme] = "photos";
      else delete x.sources[theme];
    });
    void show();
  });
  fetchBtn.addEventListener("click", async () => {
    fetchBtn.disabled = true;
    note.textContent = t("Downloading…");
    try {
      const added = await Bridge.wallpaperPhotosFetch(theme, 10);
      note.textContent = tn("{count} new photo", "{count} new photos", added);
    } catch (err) {
      note.textContent = t("Could not download: {error}", { error: String(err).replace(/^Error:\s*/, "") });
    } finally {
      fetchBtn.disabled = false;
    }
    await show();
  });
  void show();
  return h(
    "div",
    { style: "display:flex;flex-direction:column;gap:6px" },
    h("div", { class: "row" }, h("label", { text: themeName(theme) }), source, count, fetchBtn, note),
    credits,
  );
}

/** "Title by Author (License)" and a button to its page. */
function creditLine(photo: WallpaperPhoto): HTMLElement {
  const line = h("div", { class: "row", style: "gap:8px;flex-wrap:nowrap" },
    h("span", {
      class: "hint",
      style: "flex:1 1 auto;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap",
      text: t("{title} by {author} ({license})", { title: photo.title, author: photo.author, license: photo.license }),
    }),
  );
  if (/^https?:\/\//i.test(photo.sourceUrl)) {
    line.append(h("button", {
      text: "↗",
      title: t("Open the source page"),
      "aria-label": t("Open the source page"),
      style: "padding:3px 10px",
      onclick: () => void Bridge.openUrl(photo.sourceUrl),
    }));
  }
  return line;
}

// ── Ciel ──────────────────────────────────────────────────────────────────────

type CielKey = "cielSize" | "cielX" | "cielY";

function cielSection(
  prefs: () => WallpaperPrefs, change: (edit: (p: WallpaperPrefs) => void) => void,
  drag: (key: CielKey, value: number) => void,
): HTMLElement {
  const slider = (key: CielKey, min: number, max: number) => {
    const el = h("input", { type: "range", min: String(min), max: String(max), step: "0.01", value: String(prefs()[key]) }) as HTMLInputElement;
    el.addEventListener("input", () => drag(key, Number(el.value)));
    el.addEventListener("change", () => change((x) => (x[key] = Number(el.value))));
    return el;
  };
  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: "Ciel" })),
    h("div", { class: "row" }, h("label", { text: t("Size") }), slider("cielSize", 0.08, 0.4)),
    h("div", { class: "row" }, h("label", { text: t("Horizontal position") }), slider("cielX", 0.05, 0.95)),
    h("div", { class: "row" }, h("label", { text: t("Vertical position") }), slider("cielY", 0.15, 0.9)),
    h("div", { class: "hint", text: t("On the lock screen he stays below the clock.") }),
  );
}
