// The agents' list: every session Ciel hears from, one row each (core/sessions.ts).
// On top, how many need you, work, or are done. Each row has that agent's own
// little cloud in its mood, what the session was asked, what it is doing now,
// since when, and where. The row whose card waits says what it asks, with Deny
// and Allow on the row itself; a press on it opens the card, where "Don't ask
// again" is. A press on any other row brings its pill to the overview, and the
// ↗ brings that session's own terminal window to the front.

import { Bridge } from "../core/bridge";
import { agentsHeight } from "../core/layout";
import { State, type AgentTask } from "../core/state";
import { Sessions, ageText, countSessions, type SessionPhase, type SessionRow } from "../core/sessions";
import type { BotStateName } from "../core/layout";
import { createMiniBot, pruneMiniBots } from "../cloud/minibots";
import { language, t, tl, tn } from "../i18n/i18n";
import { h, svg } from "./dom";
import { ICONS } from "./icons";
import type { ViewActions, ViewHost } from "./views";

const CLAUDE_DESKTOP_ID = "agent_claude-desktop";

/** How long a row that has just begun to ask ignores clicks on its buttons. */
const CLICK_GUARD_MS = 600;

/** The mood of a row's little cloud. */
const MOOD: Record<SessionPhase, BotStateName> = {
  asking: "approval",
  working: "working",
  done: "finished",
  error: "error",
  idle: "idle",
};

/** `onRows`: the number of rows changed, the island takes its height again. */
export function buildSessions(actions: ViewActions, onRows: () => void = () => {}): ViewHost {
  const chips = h("div", { class: "sessions-chips" });
  const clearBtn = h("button", {
    class: "inbox-clear",
    text: tl("Clear"),
    onclick: () => {
      actions.blip();
      Sessions.clearFinished();
      State.notify();
    },
  });
  const list = h("div", { class: "sessions-list" });
  const empty = h("div", { class: "sessions-empty" },
    h("b", { text: tl("No agent is running.") }),
    h("span", { text: tl("Start one in your terminal and it shows here.") }),
  );
  const el = h(
    "div",
    { class: "view sessions" },
    h("div", { class: "card" },
      h("div", { class: "inbox-body" },
        h("div", { class: "inbox-head" }, h("span", { class: "inbox-title", text: tl("Agents") }), chips, h("div", { class: "grow" }), clearBtn),
        list,
        empty,
      ),
    ),
  );

  const ages = new Map<string, HTMLElement>();
  let rows: SessionRow[] = [];
  let key = "";
  let count = -1;
  let askingKey = "";
  let askingSince = 0;
  let timer: number | null = null;

  /** The ages move while the list is on screen, and only then. */
  function keepTime(on: boolean) {
    if (!on) {
      if (timer != null) window.clearInterval(timer);
      timer = null;
      return;
    }
    if (timer != null) return;
    timer = window.setInterval(() => {
      if (State.view === "agents" && State.mode === "expanded") syncAges();
      else keepTime(false);
    }, 5000);
  }

  function syncAges() {
    const now = Date.now();
    for (const row of rows) {
      const el = ages.get(row.key);
      if (el) el.textContent = ageText(now - row.since);
    }
  }

  function chip(phase: SessionPhase, text: string): HTMLElement {
    return h("span", { class: `sessions-chip ${phase}` }, h("i"), h("span", { text }));
  }

  /** What the row says under its title, by what the session is at. */
  function lineOf(row: SessionRow): string {
    const waiting = State.pendingApproval;
    switch (row.phase) {
      case "asking":
        if (waiting?.questions) return waiting.questions[0]?.question ?? t("Asks you a question");
        return t("Allow {what}?", { what: waiting?.command || waiting?.tool || "…" });
      case "working":
        return row.line || t("Thinking…");
      case "done":
        return row.line || t("Finished");
      case "error":
        return t("Stopped on an error");
      default:
        return t("Waiting for a prompt");
    }
  }

  function open(row: SessionRow) {
    actions.blip();
    actions.setFocus(row.pillId);
    // A waiting card came up with the focus; anything else opens on its pill.
    const waiting = State.pendingApproval;
    if (!(waiting && waiting.pillId === row.pillId)) actions.setView("overview");
  }

  function build(row: SessionRow): HTMLElement {
    const mood: Pick<AgentTask, "id" | "name" | "color" | "state" | "stepIndex" | "steps" | "source" | "isIntegration"> = {
      id: `session:${row.key}`, name: row.agent, color: row.color, state: MOOD[row.phase],
      stepIndex: 0, steps: [], source: "agent", isIntegration: false,
    };
    const age = h("span", { class: "session-age", text: ageText(Date.now() - row.since) });
    ages.set(row.key, age);
    const where = [row.agent, row.project].filter(Boolean).join(" · ");
    const jump = h("button", { class: "icon-btn session-jump", title: tl("Open terminal") }, svg(ICONS.arrowUpRight, 8));
    jump.addEventListener("click", (e) => {
      e.stopPropagation();
      actions.blip();
      if (row.pillId === CLAUDE_DESKTOP_ID) void Bridge.openClaudeDesktop();
      else void Bridge.openSession(row.sessionId || null, row.cwd || null);
    });

    const side = h("div", { class: "session-side" });
    const waiting = State.pendingApproval;
    if (row.phase === "asking" && waiting && !waiting.questions) {
      // Deny and Allow on the row. A click meant for something else must not
      // land on Allow as the row turns: its first moments are ignored.
      const answer = (d: "allow" | "deny") => (e: Event) => {
        e.stopPropagation();
        if (performance.now() - askingSince < CLICK_GUARD_MS) return;
        actions.decide(d);
      };
      side.append(
        h("button", { class: "btn secondary small", onclick: answer("deny") }, h("span", { text: tl("Deny") })),
        h("button", { class: "btn primary small", onclick: answer("allow") }, h("span", { text: tl("Allow") })),
      );
    } else {
      side.append(h("div", { class: "session-meta" }, age, h("span", { class: "session-where", text: where })), jump);
    }

    const r = h(
      "div",
      { class: `session-row ${row.phase}`, role: "button", tabindex: "0", title: where },
      createMiniBot(mood as AgentTask, 22),
      h("div", { class: "session-text" },
        h("div", { class: "session-title", text: row.title || row.agent }),
        h("div", { class: "session-line", text: lineOf(row) }),
      ),
      side,
    );
    r.addEventListener("click", () => open(row));
    r.addEventListener("keydown", (e) => {
      if ((e as KeyboardEvent).key === "Enter") open(row);
    });
    return r;
  }

  return {
    el,
    sync() {
      const on = State.view === "agents" && State.mode === "expanded";
      keepTime(on);
      const waiting = State.pendingApproval;
      rows = Sessions.rows(waiting ? { pillId: waiting.pillId, sessionId: waiting.sessionId } : null, Date.now());

      const asking = rows.find((r) => r.phase === "asking")?.key ?? "";
      if (asking !== askingKey) {
        askingKey = asking;
        askingSince = performance.now();
      }

      const next = [
        language(),
        waiting ? `${waiting.requestId}:${waiting.command}` : "",
        ...rows.map((r) => `${r.key}|${r.phase}|${r.title}|${r.line}|${r.color}|${r.agent}|${r.project}`),
      ].join("\n");
      if (next !== key) {
        key = next;
        ages.clear();
        list.replaceChildren(...rows.map(build));
        pruneMiniBots();

        const c = countSessions(rows);
        const shown: HTMLElement[] = [];
        if (c.asking) shown.push(chip("asking", tn("{count} needs you", "{count} need you", c.asking)));
        if (c.working) shown.push(chip("working", t("{count} working", { count: c.working })));
        if (c.done) shown.push(chip("done", t("{count} done", { count: c.done })));
        if (c.error) shown.push(chip("error", t("{count} stopped", { count: c.error })));
        chips.replaceChildren(...shown);
        clearBtn.style.display = c.done + c.error > 0 ? "" : "none";
        empty.style.display = rows.length ? "none" : "";
        list.style.display = rows.length ? "" : "none";
      } else {
        syncAges();
      }
      if (rows.length !== count) {
        count = rows.length;
        if (on) onRows();
      }
    },
  };
}

/** The island's height on the agents' list, for the rows there are now. */
export function sessionsHeight(): number {
  return agentsHeight(Sessions.list.length);
}
