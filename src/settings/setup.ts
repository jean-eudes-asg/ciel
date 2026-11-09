// The setup of the Ciel window: seven short steps over the whole window — the
// language, what Ciel follows, how the island opens, the everyday tools, the
// voice, the pills and the sky — then the keys worth knowing. On the left the
// night, where each step done lights a star on Ciel's outline; at the end he
// is drawn whole and opens his eyes. It opens by itself the first time the
// window shows; after that it waits at the foot of the menu until done.
//
// Every choice is the same setting as in the window's pages, saved as it is
// made. What writes to an agent's config (the hooks) is the pages' own
// section, embedded: the change is shown and confirmed there, as always.

import { Bridge } from "../core/bridge";
import type { Settings } from "../core/state";
import type { FlowKey } from "../core/flow";
import type { Place } from "../core/tools";
import { DEFAULT_TOOL_PREFS } from "../core/tools";
import { unitsFor, unitsOnPick } from "../core/weather";
import { NO_MAIN_PILL, chooseMainPill, mainPillChoices } from "../core/pills";
import { SHORTCUTS, SHORTCUT_TEXT, displayKeys, effective } from "../core/shortcuts";
import { h, clear } from "../views/dom";
import { LANGUAGES, t } from "../i18n/i18n";
import { applyTheme, type HubContext, type WindowTheme } from "../hub/shell";
import { cielMark, constellation, starField } from "../hub/constellation";
import { icon, keyLabel, number, segmented, select, toggle } from "../hub/ui";
import { modelsControl } from "../hub/pages/flowsettings";

export const SETUP_STEPS = ["hello", "work", "island", "tools", "voice", "pills", "sky"] as const;
export type SetupStep = (typeof SETUP_STEPS)[number];

const STEP_TITLES: Record<SetupStep, () => string> = {
  hello: () => t("Hello"),
  work: () => t("Your work"),
  island: () => t("The island"),
  tools: () => t("Everyday tools"),
  voice: () => t("Your voice"),
  pills: () => t("Pills"),
  sky: () => t("Your sky"),
};

/** The steps done, in order, or null once all are. */
export function setupProgress(s: Settings): boolean[] | null {
  const done = SETUP_STEPS.map((step) => (s.setupSteps ?? []).includes(step));
  return done.every(Boolean) ? null : done;
}

/** The setup has never been opened nor put off: the window opens on it. */
export function setupFirstTime(s: Settings): boolean {
  return (s.setupSteps ?? []).length === 0;
}

/** What the setup reaches in the settings window (main.ts). */
export interface SetupHost {
  settings(): Settings;
  /** Changes the settings and saves them. */
  change(apply: (s: Settings) => void): void;
  /** The language was changed: the window speaks it now. */
  languageChanged(): void;
  /** The sections that install the main tool's hooks (Island → Agents), ready to show. */
  agentSections(mainPill: string): Promise<HTMLElement[]>;
  /** The pills' section (Island → Active pills). */
  pillsSection(): Promise<HTMLElement>;
}

/** Ciel's place while the setup is open: the step on screen, kept across redraws. */
let shown: number | "done" = 0;

export async function renderSetup(container: HTMLElement, ctx: HubContext, host: SetupHost) {
  const progress = () => SETUP_STEPS.map((step) => (host.settings().setupSteps ?? []).includes(step));
  // Opened again: on the first step not done yet.
  const first = progress().indexOf(false);
  shown = first === -1 ? "done" : first;

  const markDone = (step: SetupStep) =>
    host.change((s) => {
      const steps = s.setupSteps ?? [];
      if (!steps.includes(step)) s.setupSteps = [...steps, step];
    });

  const go = (to: number | "done") => {
    shown = to;
    void draw();
  };

  const next = () => {
    if (shown === "done") return;
    markDone(SETUP_STEPS[shown]);
    const after = SETUP_STEPS.findIndex((_, i) => i > (shown as number) && !progress()[i]);
    const any = progress().indexOf(false);
    go(after !== -1 ? after : any !== -1 ? any : "done");
  };

  const later = () => {
    host.change((s) => {
      const steps = s.setupSteps ?? [];
      if (!steps.includes("later")) s.setupSteps = [...steps, "later"];
    });
    ctx.navigate("home");
  };

  async function draw() {
    const done = progress();
    const current = shown === "done" ? -1 : shown;

    // ── The night ──
    const stars = h("canvas", {}) as HTMLCanvasElement;
    const steps = h("div", { class: "setup-steps" });
    SETUP_STEPS.forEach((step, i) => {
      const mark = h("i", {});
      if (done[i]) mark.append(icon("check"));
      else mark.textContent = String(i + 1);
      steps.append(h("button", {
        class: ["setup-step", done[i] ? "done" : "", i === current ? "current" : ""].filter(Boolean).join(" "),
        onclick: () => go(i),
      }, mark, h("span", { text: STEP_TITLES[step]() })));
    });
    const night = h("aside", { class: "setup-night" },
      stars,
      h("div", { class: "hub-brand" }, cielMark(26, "hub-logo"), h("span", { text: "Ciel" })),
      h("div", { class: "setup-sky" }, constellation(360, 300, { done, current, big: true })),
      steps,
    );
    starField(stars);

    // ── The question ──
    const inner = h("div", { class: "inner" });
    const ask = h("main", { class: "setup-ask" }, inner);
    if (shown === "done") {
      await doneScreen(inner);
    } else {
      await STEP_DRAW[SETUP_STEPS[shown]](inner, {
        index: shown,
        host,
        ctx,
        redraw: () => void draw(),
        foot: (opts = {}) => foot(opts),
      });
    }
    clear(container);
    container.append(night, ask);
  }

  function foot(opts: { primary?: string; back?: boolean }): HTMLElement {
    const el = h("div", { class: "setup-foot" },
      h("button", { class: "later", text: t("Set up later"), onclick: later }),
      h("div", { class: "grow" }),
    );
    if (opts.back !== false && typeof shown === "number" && shown > 0) {
      const back = shown - 1;
      el.append(h("button", { class: "hub-btn", text: t("Back"), onclick: () => go(back) }));
    }
    el.append(h("button", { class: "hub-btn primary", text: opts.primary ?? t("Continue"), onclick: next }));
    return el;
  }

  async function doneScreen(inner: HTMLElement) {
    const s = host.settings();
    const rows = h("div", { class: "setup-rows" });
    for (const id of ["toggleIsland", "openChat", "goToAlert", "clipboardOpen", "micToggle"] as const) {
      const def = SHORTCUTS.find((d) => d.id === id);
      if (!def) continue;
      const b = effective(def, s.shortcuts);
      if (!b.enabled || !b.keys) continue;
      rows.append(keyRow(t(SHORTCUT_TEXT[id]), displayKeys(b.keys).split("+")));
    }
    const status = ctx.status;
    if (status.prefs.enabled) {
      rows.append(keyRow(t("Hold to dictate"), [keyLabel(status.keys === "wayland" ? "wayland" : status.prefs.key, status.platform)]));
    }
    inner.append(
      h("div", { class: "setup-kicker", text: t("All set") }),
      h("h1", { text: t("Ciel is up there now") }),
      h("p", { class: "lede", text: t("He waits at the top of your screen. The keys worth knowing:") }),
      h("div", { class: "setup-body" }, rows),
      h("div", { class: "setup-foot" },
        h("button", { class: "later", text: t("Change anything in Preferences"), onclick: () => ctx.navigate("general") }),
        h("div", { class: "grow" }),
        h("button", { class: "hub-btn primary", text: t("Open Today"), onclick: () => ctx.navigate("home") }),
      ),
    );
  }

  await draw();
}

// ── The steps ───────────────────────────────────────────────────────────────

interface StepEnv {
  index: number;
  host: SetupHost;
  ctx: HubContext;
  redraw(): void;
  foot(opts?: { primary?: string; back?: boolean }): HTMLElement;
}

type StepDraw = (inner: HTMLElement, env: StepEnv) => void | Promise<void>;

function heading(env: StepEnv, title: string, lede: string): HTMLElement[] {
  return [
    h("div", { class: "setup-kicker", text: t("Step {0} of {1}", { 0: env.index + 1, 1: SETUP_STEPS.length }) }),
    h("h1", { text: title }),
    h("p", { class: "lede", text: lede }),
  ];
}

function settingRow(title: string, detail: string | null, ...controls: (HTMLElement | null)[]): HTMLElement {
  const what = h("div", { class: "what" }, h("b", { text: title }));
  if (detail) what.append(h("span", { text: detail }));
  const el = h("div", { class: "setup-row" }, what);
  for (const c of controls) if (c) el.append(c);
  return el;
}

function keyRow(title: string, keys: string[]): HTMLElement {
  return settingRow(title, null, h("div", { class: "keys" }, ...keys.map((k) => h("span", { class: "hub-keycap", text: k }))));
}

function choice(title: string, detail: string | null, on: boolean, onclick: () => void, dot?: string, wide = false): HTMLElement {
  const b = h("b", {});
  if (dot) b.append(h("i", { style: `background:${dot}` }));
  b.append(document.createTextNode(title));
  const el = h("button", {
    class: ["setup-choice", on ? "on" : "", wide ? "wide" : ""].filter(Boolean).join(" "),
    "aria-pressed": on,
    onclick,
  }, cielMark(20, "hub-here"), b);
  if (detail) el.append(h("span", { text: detail }));
  return el;
}

function note(text: string): HTMLElement {
  return h("div", { class: "setup-note" }, icon("lock"), h("div", { text }));
}

const STEP_DRAW: Record<SetupStep, StepDraw> = {
  hello(inner, env) {
    const s = env.host.settings();
    const language = h("select", { class: "hub-select" }) as HTMLSelectElement;
    language.append(h("option", { value: "", text: t("System") }));
    for (const { code, name } of LANGUAGES) language.append(h("option", { value: code, text: name, lang: code }));
    language.value = LANGUAGES.some((l) => l.code === s.language) ? s.language : "";
    language.addEventListener("change", () => {
      env.host.change((x) => (x.language = language.value));
      env.host.languageChanged();
      env.redraw();
    });
    const theme = segmented<WindowTheme>(
      [["black", t("Black")], ["white", t("White")], ["system", t("System")]],
      s.windowTheme ?? "black",
      (v) => {
        env.host.change((x) => (x.windowTheme = v));
        applyTheme(v);
      },
    );
    inner.append(
      h("div", { class: "setup-kicker", text: t("Set up Ciel") }),
      h("h1", { text: t("Ciel lives at the top of your screen") }),
      h("p", { class: "lede", text: t("A few questions and he is ready: what he follows, how he opens, the tools and the voice you want. All of it can be changed later.") }),
      h("div", { class: "setup-body" },
        h("div", { class: "setup-rows" },
          settingRow(t("Language"), t("Ciel and his window speak it."), language),
          settingRow(t("Window"), t("Pitch black or white."), theme),
        ),
      ),
      env.foot({ primary: t("Start"), back: false }),
    );
  },

  async work(inner, env) {
    const s = env.host.settings();
    const pick = (id: string) => {
      const next = chooseMainPill(env.host.settings(), id);
      if (!next) return;
      env.host.change((x) => {
        x.mainPill = next.mainPill;
        x.activeIntegrations = next.activeIntegrations;
      });
      env.redraw();
    };
    const grid = h("div", { class: "setup-choices" });
    for (const def of mainPillChoices()) {
      grid.append(choice(def.name, null, s.mainPill === def.id, () => pick(def.id), def.color));
    }
    grid.append(choice(
      t("I don't code"),
      t("Ciel keeps your day, your voice and your tools."),
      s.mainPill === NO_MAIN_PILL,
      () => pick(NO_MAIN_PILL),
      undefined,
      true,
    ));
    const body = h("div", { class: "setup-body" }, grid);
    if (s.mainPill !== NO_MAIN_PILL) {
      const sections = await env.host.agentSections(s.mainPill);
      if (sections.length) {
        body.append(
          note(t("Ciel adds his hooks to the agent's own config. You see the exact change and where the backup goes before anything is written.")),
          h("div", { class: "setup-embed" }, ...sections),
        );
      }
    }
    inner.append(
      ...heading(env, t("What do you code with?"), t("Ciel follows its sessions in the island, and you answer it from there. Other agents can be added later in Island → Agents.")),
      body,
      env.foot(),
    );
  },

  async island(inner, env) {
    const s = env.host.settings();
    const set = (apply: (x: Settings) => void) => env.host.change(apply);
    const opens = h("div", { class: "setup-choices" },
      choice(t("When I click it"), t("He stays folded until you click him."), !s.openOnHover, () => {
        set((x) => (x.openOnHover = false));
        env.redraw();
      }),
      choice(t("When I point at it"), t("He opens under the pointer and folds once it leaves."), s.openOnHover, () => {
        set((x) => (x.openOnHover = true));
        env.redraw();
      }),
    );
    const screen = h("select", { class: "hub-select" }) as HTMLSelectElement;
    screen.append(
      h("option", { value: "primary", text: t("Main display") }),
      h("option", { value: "cursor", text: t("Display under the cursor") }),
    );
    for (const m of (await Bridge.listMonitors()) ?? []) screen.append(h("option", { value: m.key, text: m.label }));
    screen.value = s.screen;
    if (!screen.value) screen.value = "primary";
    screen.addEventListener("change", () => set((x) => (x.screen = screen.value)));
    inner.append(
      ...heading(env, t("How should the island open?"), t("He waits folded at the top of the screen. Pick how he opens; the rest starts well as it is.")),
      h("div", { class: "setup-body" },
        opens,
        h("div", { class: "setup-rows" },
          settingRow(t("Close when I click outside"), t("A click anywhere else folds the open island. A card waiting for your answer stays open."),
            toggle(s.closeOnClickOutside, (v) => set((x) => (x.closeOnClickOutside = v)))),
          settingRow(t("Island lives on"), null, screen),
          settingRow(t("Launch at startup"), null, toggle(s.autostart, (v) => set((x) => (x.autostart = v)))),
        ),
      ),
      env.foot(),
    );
  },

  tools(inner, env) {
    const tools = () => ({ ...DEFAULT_TOOL_PREFS, ...(env.host.settings().tools ?? {}) });
    const set = (change: Partial<Settings["tools"]>) =>
      env.host.change((x) => (x.tools = { ...tools(), ...change }));

    // The weather's town: searched on Open-Meteo only once a name is typed.
    const placeLine = h("span", { text: tools().weatherPlace || t("Pick your town for the forecast and the rain alerts.") });
    const field = h("input", { class: "hub-input", type: "search", placeholder: t("Your town") }) as HTMLInputElement;
    const found = h("div", { class: "found" });
    const makeUnits = () => segmented<"c" | "f">([["c", "°C"], ["f", "°F"]], unitsFor(tools().weatherUnits, navigator.languages),
      (v) => set({ weatherUnits: v }));
    let units = makeUnits();
    let timer = 0;
    field.addEventListener("input", () => {
      window.clearTimeout(timer);
      const query = field.value.trim();
      if (query.length < 2) {
        clear(found);
        return;
      }
      timer = window.setTimeout(async () => {
        let places: Place[] = [];
        try {
          places = await Bridge.weatherSearch(query, document.documentElement.lang || "en");
        } catch {
          places = [];
        }
        clear(found);
        for (const p of places.slice(0, 5)) {
          found.append(h("button", {
            text: p.detail ? `${p.name}, ${p.detail}` : p.name,
            onclick: () => {
              const name = p.detail ? `${p.name}, ${p.detail}` : p.name;
              set({ weatherPlace: name, weatherLat: p.lat, weatherLon: p.lon, ...unitsOnPick(tools().weatherUnits, p.countryCode) });
              placeLine.textContent = name;
              // The place's country may have picked the units.
              const next = makeUnits();
              units.replaceWith(next);
              units = next;
              field.value = "";
              clear(found);
            },
          }));
        }
        if (!places.length) found.append(h("div", { class: "hint", style: "padding:4px 10px", text: t("No town by that name.") }));
      }, 350);
    });
    const weather = h("div", { class: "setup-row-group" },
      settingRow(t("Weather"), null),
      h("div", { class: "setup-place" }, field, found),
    );
    (weather.firstChild as HTMLElement).querySelector(".what")!.append(placeLine);

    inner.append(
      ...heading(env, t("Everyday tools"), t("Each stays off until you turn it on here or in the island's Tools. What you copy never leaves this computer.")),
      h("div", { class: "setup-body" },
        h("div", { class: "setup-rows" },
          settingRow(t("Keep what I copy"), t("A history of what you copy, in the island's Tools; what you pin stays."),
            toggle(tools().clipboard, (v) => set({ clipboard: v }))),
          settingRow(t("Lyrics"), t("The words of the song playing, from lrclib.net."),
            toggle(tools().lyrics, (v) => set({ lyrics: v }))),
          weather,
          settingRow(t("Units"), null, units),
          settingRow(t("Rain alerts"), t("A card when rain is about to start where you are."),
            toggle(tools().rainWarnings, (v) => set({ rainWarnings: v }))),
        ),
        note(t("From Open-Meteo, free and without an account. Only the place you pick is ever sent.")),
      ),
      env.foot(),
    );
  },

  voice(inner, env) {
    const status = env.ctx.status;
    const keys: [FlowKey, string][] = [
      ["right_ctrl", t("Right {0}", { 0: "Ctrl" })],
      ["right_alt", t("Right {0}", { 0: "Alt" })],
      ["right_shift", t("Right {0}", { 0: "Shift" })],
      ["left_ctrl_super", status.platform === "windows" ? "Ctrl + Win" : "Ctrl + Super"],
    ];
    inner.append(
      ...heading(env, t("Talk instead of typing"), t("Hold a key in any app, speak, let go: your words are typed where you are. It all runs on this computer, offline.")),
      h("div", { class: "setup-body" },
        h("div", { class: "setup-rows" },
          settingRow(t("Turn on dictation"), null, toggle(status.prefs.enabled, (on) => {
            void env.ctx.setPrefs((p) => (p.enabled = on));
          })),
          settingRow(
            t("Parakeet, on your computer"),
            t("The most accurate, offline, 25 languages. Downloaded once (about {0} MB).", { 0: number(status.models.downloadMb) }),
            modelsControl(env.ctx, () => env.redraw()),
          ),
          settingRow(t("Hold to dictate"), null, select(keys, status.prefs.key, (k) => {
            void env.ctx.setPrefs((p) => (p.key = k));
          })),
        ),
      ),
      env.foot(),
    );
  },

  async pills(inner, env) {
    inner.append(
      ...heading(env, t("Pills beside Ciel"), t("Up to four small clouds next to him: your music, GitHub, your calendar and more. Their keys go in Island → Integrations.")),
      h("div", { class: "setup-body" }, h("div", { class: "setup-embed" }, await env.host.pillsSection())),
      env.foot(),
    );
  },

  sky(inner, env) {
    const s = env.host.settings();
    inner.append(
      ...heading(env, t("Your sky"), t("Ciel can paint the sky of the hour on your desktop, with him in it, and he makes small sounds when something happens.")),
      h("div", { class: "setup-body" },
        h("div", { class: "setup-rows" },
          settingRow(t("Sky on the desktop"), t("The real sky of the hour as your wallpaper. Turning it off puts your own picture back."),
            toggle(s.wallpaper.desktop, (v) => env.host.change((x) => (x.wallpaper = { ...x.wallpaper, desktop: v })))),
          settingRow(t("Sound"), null, toggle(s.soundEnabled, (v) => env.host.change((x) => (x.soundEnabled = v)))),
        ),
      ),
      env.foot({ primary: t("Finish") }),
    );
  },
};
