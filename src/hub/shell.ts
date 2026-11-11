// The Ciel window: a menu of five spaces — Today, Island, Voice, Sky and
// Preferences — the space you are in open on its pages, a small Ciel beside
// the page you are on; the page on the right under a plain title. The setup
// takes the whole window while it runs (settings/setup.ts) and waits at the
// foot of the menu until it is done. Pitch black, or white.

import "@fontsource-variable/nunito/wght.css";
import "./hub.css";
import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { onEvent } from "../core/bridge";
import { FlowBridge, type FlowPrefs, type FlowStatus, type ModelsStatus } from "../core/flow";
import { NO_BRAIN } from "../core/brain";
import { cielMark, constellation } from "./constellation";
import { icon, keyLabel } from "./ui";

export type PageId =
  | "home" | "games"
  | "dictation" | "notes" | "insights" | "dictionary" | "snippets" | "styles" | "flowsettings"
  | "general" | "pills" | "agents" | "chat" | "voice" | "tools" | "integrations" | "shortcuts" | "wallpaper"
  | "screensaver" | "updates" | "setup";

export type SpaceId = "today" | "island" | "listen" | "sky" | "prefs";

interface PageDef {
  id: PageId;
  title: () => string;
  /** The line under the title of the pages drawn from the island's settings. */
  subtitle?: () => string;
  /** Pages with columns and lists get the window's full width. */
  wide?: boolean;
}

interface SpaceDef {
  id: SpaceId;
  icon: string;
  title: () => string;
  pages: PageDef[];
}

export const SPACES: SpaceDef[] = [
  {
    id: "today", icon: "sun", title: () => t("Today"), pages: [
      { id: "home", title: () => t("Your day"), wide: true },
      { id: "games", title: () => t("Games"), wide: true },
    ],
  },
  {
    id: "island", icon: "island", title: () => t("Island"), pages: [
      { id: "pills", title: () => t("Active pills"), subtitle: () => t("The pills shown in the island, their order and their colours.") },
      { id: "agents", title: () => t("Agents"), subtitle: () => t("Claude Code, Codex and the other agents Ciel follows.") },
      { id: "tools", title: () => t("Tools"), subtitle: () => t("The island's clipboard, timers, controls, weather and shelf.") },
      { id: "integrations", title: () => t("Integrations"), subtitle: () => t("Stripe, GitHub, Vercel, n8n, Notion and the rest.") },
      { id: "chat", title: () => t("Chat"), subtitle: () => t("The AI of the island's chat, with your own keys.") },
    ],
  },
  {
    id: "listen", icon: "mic", title: () => t("Voice"), pages: [
      { id: "dictation", title: () => t("Dictation"), wide: true },
      { id: "notes", title: () => t("Notetaker"), wide: true },
      { id: "voice", title: () => "OK Ciel" },
      { id: "insights", title: () => t("Statistics"), wide: true },
      { id: "dictionary", title: () => t("Dictionary") },
      { id: "snippets", title: () => t("Snippets") },
      { id: "styles", title: () => t("Styles") },
      { id: "flowsettings", title: () => t("Settings") },
    ],
  },
  {
    id: "sky", icon: "cloud", title: () => t("Sky"), pages: [
      { id: "wallpaper", title: () => t("Wallpaper"), subtitle: () => t("Ciel's sky on your desktop and lock screen, in his mood.") },
      { id: "screensaver", title: () => t("Screensaver"), subtitle: () => t("A flip clock when you step away, and the screens off at night.") },
    ],
  },
  {
    id: "prefs", icon: "sliders", title: () => t("Preferences"), pages: [
      { id: "general", title: () => t("General"), subtitle: () => t("Ciel, his sounds and how the island behaves.") },
      { id: "shortcuts", title: () => t("Shortcuts"), subtitle: () => t("The keys that open the island and its actions.") },
      { id: "updates", title: () => t("Updates and privacy"), subtitle: () => t("New versions of Ciel, and what reaches the network.") },
    ],
  },
];

const ALL: PageDef[] = SPACES.flatMap((s) => s.pages);
const spaceOf = (page: PageId) => SPACES.find((s) => s.pages.some((p) => p.id === page)) ?? SPACES[0];

/** The window's colours: pitch black, white, or the system's. */
export { applyTheme, type Theme as WindowTheme } from "../core/theme";

/** What a page gets to draw itself and reach the rest of the window. */
export interface HubContext {
  status: FlowStatus;
  navigate(page: PageId): void;
  /** Reads the status again (prefs, models) and redraws the menu's line. */
  refresh(): Promise<FlowStatus>;
  /** Saves new preferences and keeps them as the window's. */
  setPrefs(change: (p: FlowPrefs) => void): Promise<void>;
  /** The title of a page: its space, its name, a line, and its actions. */
  hero(page: PageId, title: string, subtitle: string | null, actions?: HTMLElement[], compact?: boolean): HTMLElement;
  /** Draws the menu again (the setup's progress changed). */
  redrawMenu(): void;
  /** Set by a page: called when the page is left. */
  onLeave?: () => void;
}

export type FlowPageRenderer = (container: HTMLElement, ctx: HubContext) => void | Promise<void>;

export interface HubOptions {
  version: string;
  flowPages: Partial<Record<PageId, FlowPageRenderer>>;
  /** Draws the island's settings for one of their pages into `container`. */
  renderNotch(page: PageId, container: HTMLElement): Promise<void>;
  /** Draws the setup, over the whole window. */
  renderSetup(container: HTMLElement, ctx: HubContext): Promise<void>;
  /** The steps of the setup done so far, or null once it is finished. */
  setupProgress(): boolean[] | null;
  /** The page the window opens on. */
  firstPage: PageId;
}

const DEFAULT_STATUS: FlowStatus = {
  prefs: {
    enabled: false, key: "right_ctrl", edge: "island", mic: "", keepDays: 30, cleanup: false,
    defaultStyle: "natural", summaryEngine: "claude", openAtLaunch: true, voice: false, voiceOutput: "system", voiceGender: "female",
  },
  models: { parakeet: { state: "missing" }, downloadMb: 700, kitten: { state: "missing" }, kittenMb: 63, kokoro: { state: "missing" }, kokoroMb: 349 },
  keys: "ok",
  hasClaudeKey: false,
  ai: NO_BRAIN,
  mics: [],
  platform: navigator.userAgent.includes("Windows") ? "windows" : "linux",
  autostarted: false,
};

/** A page's title block: where it is, what it is, and its actions on the right. */
function pageHead(page: PageId, title: string, subtitle: string | null, actions: HTMLElement[] = []): HTMLElement {
  const copy = h("div", { class: "copy" },
    h("div", { class: "crumb", text: spaceOf(page).title() }),
    h("h1", { text: title }),
  );
  if (subtitle) copy.append(h("p", { text: subtitle }));
  const head = h("header", { class: "hub-head" }, copy);
  if (actions.length) head.append(h("div", { class: "actions" }, ...actions));
  return head;
}

export async function startHub(root: HTMLElement, opts: HubOptions) {
  document.body.classList.add("hub-window");
  let current: PageId = opts.firstPage;
  let status: FlowStatus = (await FlowBridge.status()) ?? DEFAULT_STATUS;
  const statusLine = h("div", { class: "hub-status" });
  const scroll = h("div", { class: "hub-scroll" });
  const nav = h("div", { class: "hub-nav" });
  const setupSlot = h("div", { class: "hub-setup-slot" });
  const setupLayer = h("div", { class: "hub-setup-layer" });
  let rendering = 0;

  const ctx: HubContext = {
    get status() {
      return status;
    },
    navigate: (page) => void show(page),
    refresh: async () => {
      status = (await FlowBridge.status()) ?? status;
      drawStatus();
      return status;
    },
    setPrefs: async (change) => {
      const next = { ...status.prefs };
      change(next);
      status = { ...status, prefs: next };
      drawStatus();
      await FlowBridge.setPrefs(next);
    },
    hero: (page, title, subtitle, actions = []) => pageHead(page, title, subtitle, actions),
    redrawMenu: () => drawMenu(),
  };

  function drawStatus() {
    clear(statusLine);
    const models = status.models.parakeet.state;
    let tone = "off";
    let text: string;
    if (!status.prefs.enabled) {
      text = t("Dictation is off");
    } else if (models !== "ready") {
      tone = "warn";
      text = models === "downloading" ? t("Downloading the voice model…") : t("Download the voice model to dictate");
    } else if (status.keys !== "ok" && status.keys !== "wayland") {
      tone = "warn";
      text = t("Use the pill's microphone to dictate");
    } else {
      tone = "ok";
      text = t("Ready · hold {0}", { 0: keyLabel(status.keys === "wayland" ? "wayland" : status.prefs.key, status.platform) });
    }
    statusLine.append(h("i", { class: `dot ${tone}` }), h("span", { text }));
  }

  function drawMenu() {
    clear(nav);
    const open = spaceOf(current);
    for (const space of SPACES) {
      const head = h("button", {
        class: space.id === open.id && current !== "setup" ? "hub-space open" : "hub-space",
        onclick: () => void show(space.pages[0].id),
      });
      head.append(icon(space.icon), h("span", { text: space.title() }));
      nav.append(head);
      if (space.id !== open.id || current === "setup") continue;
      const subs = h("div", { class: "hub-subs" });
      for (const p of space.pages) {
        const b = h("button", {
          class: p.id === current ? "hub-item on" : "hub-item",
          "aria-current": p.id === current ? "page" : "false",
          onclick: () => void show(p.id),
        });
        b.append(cielMark(16, "hub-here"), h("span", { text: p.title() }));
        subs.append(b);
      }
      nav.append(subs);
    }

    clear(setupSlot);
    const progress = opts.setupProgress();
    if (progress) {
      const done = progress.filter(Boolean).length;
      const card = h("button", { class: "hub-setup-card", onclick: () => void show("setup") });
      card.append(
        h("span", { class: "night" }, constellation(48, 38, { done: progress, current: progress.indexOf(false), big: false })),
        h("span", { class: "words" },
          h("b", { text: t("Set up Ciel") }),
          h("span", { text: t("{0} of {1} done", { 0: done, 1: progress.length }) }),
        ),
      );
      setupSlot.append(card);
    }
  }

  async function show(page: PageId) {
    const def = page === "setup" ? null : ALL.find((p) => p.id === page) ?? ALL[0];
    ctx.onLeave?.();
    ctx.onLeave = undefined;
    current = def?.id ?? "setup";
    const token = ++rendering;

    if (!def) {
      const layer = h("div", { class: "setup" });
      await opts.renderSetup(layer, ctx);
      if (token !== rendering) return;
      clear(setupLayer);
      setupLayer.append(layer);
      root.classList.add("in-setup");
      drawMenu();
      return;
    }

    drawMenu();
    const page_ = h("div", { class: def.wide ? "hub-page wide" : "hub-page" });
    const renderer = opts.flowPages[def.id];
    if (renderer) {
      await renderer(page_, ctx);
    } else {
      page_.append(pageHead(def.id, def.title(), def.subtitle?.() ?? null));
      const container = h("div", { id: "settings-root" });
      page_.append(container);
      await opts.renderNotch(def.id, container);
    }
    if (token !== rendering) return;
    root.classList.remove("in-setup");
    clear(setupLayer);
    clear(scroll);
    scroll.append(page_);
    scroll.scrollTop = 0;
  }

  // ── The menu ──
  const side = h("nav", { class: "hub-side" });
  side.append(
    h("div", { class: "hub-brand" }, cielMark(26, "hub-logo"), h("span", { text: "Ciel" })),
    nav,
    h("div", { class: "spacer" }),
    setupSlot,
    statusLine,
    h("div", { class: "hub-version", text: opts.version ? `Ciel ${opts.version}` : "Ciel" }),
  );

  clear(root);
  root.append(side, h("main", { class: "hub-panel" }, scroll), setupLayer);
  drawStatus();
  await show(current);

  void onEvent<string>("hub-page", (page) => {
    const id = (page === "settings" ? "flowsettings" : page) as PageId;
    if (id === "setup" || ALL.some((p) => p.id === id)) void show(id);
  });
  void onEvent<FlowPrefs>("flow-prefs", (prefs) => {
    status = { ...status, prefs };
    drawStatus();
  });
  void onEvent<ModelsStatus>("speech-models", (models) => {
    status = { ...status, models };
    drawStatus();
  });
  return {
    redraw: () => void show(current),
    current: () => current,
    show: (page: PageId) => void show(page),
  };
}
