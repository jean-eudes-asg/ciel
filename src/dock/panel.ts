// The panel over the dock on GNOME (dock-panel.html): the corner opens
// Tools → Controls, the clock the month (Tools → Calendar) with the
// notifications, as the island has them. The same page in a second window
// (dock-hover) opens while the pointer rests on Ciel's tile: the request
// that waits, with Allow and Deny, the agents at work and the plans' usage.
// That one never takes the keyboard, and goes once the pointer has left. Rust shows it over the dock clicked
// once it is drawn, at its size (src-tauri/src/dock/linux.rs), and takes it
// away when another window takes the keyboard, or on Escape. The island
// keeps the notifications and the calendar's events (src/island/dock.ts): it
// tells this page, and what is opened or dismissed here goes back to it.

import "../style.css";
import "./panel.css";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Bridge, broadcast, emitToWindow, onEvent } from "../core/bridge";
import { CREW_MAX, DOCK_EVENTS, EMPTY_SNAPSHOT, crewNow, type DockPanelSnapshot, type DockRequest, type DockSnapshot } from "../core/dock";
import { ageText } from "../core/sessions";
import { PLAN_TEXT, effectivePct, resetLabel, type PlanUsage, type PlanWindow } from "../core/plan";
import { State, type Settings } from "../core/state";
import { applyTheme } from "../core/theme";
import { onLanguageChange, resolveLanguage, setLanguage, systemLanguages, t, tl } from "../i18n/i18n";
import { Tools } from "../tools/store";
import { buildControls } from "../tools/controls";
import { buildCalendar } from "../tools/calendar";
import type { ToolPanel } from "../tools/view";
import type { ViewActions } from "../views/views";
import { ago, noteIcon } from "../views/inbox";
import { h } from "../views/dom";

const PANEL_EVENT = "dock-panel";
/** Rust: the window goes in a moment; the card fades first (panel.css). */
const PANEL_OUT = "dock-panel-out";
const CALENDAR_ID = "integration_calendar";

// The tools ask only for their click sound, which GNOME's own panels don't make.
const actions = { blip: () => {} } as unknown as ViewActions;

/** Opened or dismissed (id), or all cleared (null): the island does it. */
const settle = (id: string | null, action: string) => void broadcast(DOCK_EVENTS.note, { id, action });

/** The notifications, as the island's list draws them, under the month. */
function buildNotes(): ToolPanel {
  const count = h("span", { class: "inbox-count" });
  const clear = h("button", { class: "inbox-clear", text: tl("Clear"), onclick: () => settle(null, "dismiss") });
  const list = h("div", { class: "inbox-list" });
  const empty = h("div", { class: "inbox-empty", text: tl("You're all caught up.") });
  const el = h("div", { class: "panel-notes" },
    h("div", { class: "inbox-head" }, h("span", { class: "inbox-title", text: tl("Notifications") }), count, h("div", { class: "grow" }), clear),
    list, empty);
  let key = "";
  return {
    el,
    sync() {
      const all = State.notifications;
      count.textContent = all.length ? String(all.length) : "";
      clear.style.visibility = all.length ? "visible" : "hidden";
      empty.style.display = all.length ? "none" : "";
      const now = Date.now();
      const next = JSON.stringify([all.map((n) => n.id), Math.floor(now / 60_000)]);
      if (next === key) return;
      key = next;
      list.replaceChildren(...all.map((n) => {
        const dismiss = h("button", { class: "inbox-dismiss", title: tl("Dismiss"), text: "✕" });
        dismiss.addEventListener("click", (e) => {
          e.stopPropagation();
          settle(n.id, "dismiss");
        });
        const row = h("div", { class: "inbox-row", role: "button", tabindex: "0", title: t("Open") },
          noteIcon(n, 30),
          h("div", { class: "inbox-text" },
            h("div", { class: "inbox-meta" }, h("span", { class: "inbox-app", text: n.app }), h("span", { class: "inbox-when", text: ago(n.at, now) })),
            h("div", { class: "inbox-row-title", text: n.title }),
            ...(n.body ? [h("div", { class: "inbox-row-body", text: n.body })] : [])),
          dismiss);
        row.addEventListener("click", () => {
          settle(n.id, "open");
          void Bridge.dockPanelClose();
        });
        return row;
      }));
    },
  };
}

// ── Over Ciel's tile: the agents ──────────────────────────────────────────────

const HOVER = getCurrentWebview().label === "dock-hover";
/** The island's agents and Claude plan (DOCK_EVENTS.island), and what waits (DOCK_EVENTS.panel). */
let island: DockSnapshot = EMPTY_SNAPSHOT;
let request: DockRequest | null = null;
let codex: PlanUsage | null = null;
/** Allow and Deny wait this long after they appear or move: a click meant for something else never answers. */
const ARM_MS = 400;

function buildAgents(): ToolPanel {
  const asks = h("div", { class: "ag-request" });
  const list = h("div", { class: "ag-list" });
  const plans = h("div", { class: "ag-plans" });
  const open = h("button", { class: "tool-link", text: tl("Open the island"), onclick: () => openIsland(false) });
  const el = h("div", { class: "panel-agents" },
    asks,
    h("div", { class: "ag-title", text: tl("Agents") }), list,
    plans,
    open);
  let armedAt = 0;
  // Never a request's key: the first paint hides the card when nothing waits.
  let requestKey = "-";

  function openIsland(waiting: boolean) {
    void emitToWindow("island", DOCK_EVENTS.open, waiting ? "waiting" : "agents");
    void Bridge.dockHover("close", false);
  }

  function paintRequest() {
    const key = request ? `${request.requestId}:${request.command}` : "";
    if (key === requestKey) return;
    requestKey = key;
    armedAt = Date.now() + ARM_MS;
    asks.hidden = !request;
    if (!request) return asks.replaceChildren();
    const req = request;
    const who = [req.agent, req.project].filter(Boolean).join(" · ");
    // The request whole, as the island's card shows it ("Bash · npm test"): what Allow allows.
    const what = req.command || req.tool;
    const decide = (decision: "allow" | "deny") => () => {
      if (Date.now() < armedAt) return;
      void broadcast(DOCK_EVENTS.decide, { requestId: req.requestId, decision });
    };
    asks.replaceChildren(
      h("div", { class: "ag-who", text: who || t("Agents") }),
      h("div", { class: "ag-what", text: req.question ? t("Asks you a question") : what }),
      h("div", { class: "ag-buttons" }, ...(req.question
        ? [h("button", { class: "ag-btn primary", text: tl("Answer"), onclick: () => openIsland(true) })]
        : [h("button", { class: "ag-btn", text: tl("Deny"), onclick: decide("deny") }),
          h("button", { class: "ag-btn primary", text: tl("Allow"), onclick: decide("allow") })])),
    );
  }

  function paintList(now: number) {
    const all = crewNow(island.crew, now);
    list.replaceChildren(...(all.length ? all.slice(0, CREW_MAX * 2).map((m) => {
      const doing = m.phase === "asking" ? t("Asks you a question") : m.line;
      return h("div", { class: `ag-row ${m.phase}` },
        h("i", { class: "ag-pip" }),
        h("div", { class: "ag-text" },
          h("div", { class: "ag-name" }, h("b", { text: m.project || m.agent }), h("span", { text: m.project ? m.agent : "" })),
          ...(doing ? [h("div", { class: "ag-doing", text: doing })] : [])),
        h("span", { class: "ag-age", text: ageText(now - m.since) }));
    }) : [h("div", { class: "ag-none", text: t("No agent is running.") })]));
  }

  function planRows(title: string, u: PlanUsage | null, now: number): HTMLElement[] {
    const line = (name: string, w: PlanWindow | undefined, weekly: boolean): HTMLElement | null => {
      if (!w) return null;
      const pct = Math.round(effectivePct(w, now));
      const bar = h("span", { class: "ag-bar" }, h("i"));
      bar.style.setProperty("--used", `${pct}%`);
      bar.classList.toggle("high", pct >= 80);
      return h("div", { class: "ag-window" },
        h("span", { class: "ag-window-name", text: name }), bar,
        h("span", { class: "ag-pct", text: `${pct}%` }),
        h("span", { class: "ag-reset", text: `${PLAN_TEXT.resets} ${resetLabel(w, weekly, now)}` }));
    };
    const rows = [line(PLAN_TEXT.fiveHours, u?.fiveHour, false), line(PLAN_TEXT.week, u?.sevenDay, true)].filter((r): r is HTMLElement => !!r);
    return rows.length ? [h("div", { class: "ag-plan-name", text: title }), ...rows] : [];
  }

  return {
    el,
    sync() {
      const now = Date.now();
      paintRequest();
      paintList(now);
      const rows = [...planRows(PLAN_TEXT.claudeTitle, island.plan, now), ...planRows(PLAN_TEXT.codexTitle, codex, now)];
      plans.hidden = rows.length === 0;
      plans.replaceChildren(h("div", { class: "ag-title", text: tl("Plan usage") }), ...rows);
    },
  };
}

function buildClock(): ToolPanel {
  const calendar = buildCalendar(actions);
  const notes = buildNotes();
  return {
    el: h("div", { class: "panel-clock" }, calendar.el, notes.el),
    shown: () => calendar.shown?.(),
    sync() {
      calendar.sync();
      notes.sync();
    },
  };
}

// The hover window shows the agents alone; the clicked one the corner's and the clock's.
const panels: Record<string, ToolPanel> = HOVER
  ? { agents: buildAgents() }
  : { controls: buildControls(actions), clock: buildClock() };
const root = h("div", { class: "card dock-panel" });
document.body.append(root);
let current: ToolPanel | null = null;

function fit() {
  if (current) void Bridge.dockPanelFit(root.offsetWidth, root.offsetHeight);
}

function show(kind: string) {
  current = panels[kind] ?? null;
  if (!current) return;
  root.dataset.kind = kind;
  root.replaceChildren(current.el);
  // It rises in, as the island opens: the animation again for each opening.
  root.classList.remove("enter", "leave");
  void root.offsetWidth;
  root.classList.add("enter");
  current.shown?.();
  current.sync();
  // Not a frame later: a window not shown yet draws no frames.
  fit();
}

function apply(s: Partial<Settings> | null | undefined) {
  if (!s) return;
  State.settings = { ...State.settings, ...s };
  setLanguage(resolveLanguage(s.language, systemLanguages()));
  applyTheme(s.islandTheme ?? "black");
}

State.subscribe(() => current?.sync());
onLanguageChange(() => current?.sync());
// A list opened in place of the tiles: the window takes its new height.
new ResizeObserver(fit).observe(root);
if (HOVER) {
  // The pointer on this window keeps it, as on Ciel's tile (Rust waits a moment for it to cross).
  document.documentElement.addEventListener("pointerenter", () => void Bridge.dockHover("panel", true));
  document.documentElement.addEventListener("pointerleave", () => void Bridge.dockHover("panel", false));
}
window.addEventListener("keydown", (e) => {
  // Escape in the calendar's city search only leaves the search.
  if (e.key === "Escape" && !(e.target instanceof HTMLInputElement)) void Bridge.dockPanelClose();
});
window.addEventListener("contextmenu", (e) => e.preventDefault());

Tools.start();
void onEvent<string>(PANEL_EVENT, show);
void onEvent<null>(PANEL_OUT, () => root.classList.add("leave"));
void onEvent<Settings>("settings-changed", apply);
void onEvent<DockSnapshot>(DOCK_EVENTS.island, (s) => {
  island = s;
  State.notify();
});
void onEvent<DockPanelSnapshot>(DOCK_EVENTS.panel, ({ notifications, calendar, request: waiting, codex: plan }) => {
  request = waiting;
  codex = plan;
  State.notifications = notifications;
  if (calendar) State.integrations[CALENDAR_ID] = calendar;
  else delete State.integrations[CALENDAR_ID];
  State.notify();
});
void Bridge.boot()
  .then((boot) => apply(boot?.settings))
  .catch(() => {})
  .finally(() => {
    void broadcast(DOCK_EVENTS.panelReady);
    void Bridge.dockPanelKind().then((kind) => kind && show(kind));
  });
