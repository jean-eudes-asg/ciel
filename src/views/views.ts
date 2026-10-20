// Island views — DOM ports of IslandViewContent.swift. Paddings, font sizes,
// colours and wording are copied from the Swift views so both platforms read
// identically.

import { h, svg, clear, dot } from "./dom";
import { ICONS } from "./icons";
import { Ticker } from "./ticker";
import { State, type AgentTask } from "../core/state";
import { washRGBA, type IslandViewName, type Wash } from "../core/layout";
import { createMiniBot, pruneMiniBots } from "../cloud/minibots";
import { buildPrompt } from "./chat";
import { buildChoose, buildUpload, buildUploading } from "./upload";
import { renderIntegrationCard, type IntegrationCardHooks } from "./integrations";
import { highlightRow, listRows, openRow } from "./github";
import { pillDefinition, sessionSubtitle } from "../core/pills";
import {
  PlanCard, buildPlanPill, claudePillVisible, codexPillVisible, planCardOpen, refreshCodexPlanUsage,
} from "./usage";
import { buildDiffCard } from "./diff";
import { lastTextStep } from "../core/diff";
import { Bridge } from "../core/bridge";
import { buildRecap } from "./recap";
import { buildPlayerCard, buildPlayerPill, type SpotifyCardHost, type SpotifyPillHost } from "./spotify";
import { playerFor } from "../core/player";
import { pillPlan, planOverview, recentEdits, type OverviewLayout } from "../core/overview";
import { buildFinanceCard } from "./finance";
import { FINANCE_ID } from "../core/finance";
import { language, t, tl, type Msg } from "../i18n/i18n";
import type { ViewCommand } from "../island/shortcuts";
import { buildInbox } from "./inbox";

export interface ViewActions {
  setView(v: IslandViewName): void;
  /** "Cancel" on a dropped file: forgets it and goes back home. */
  cancelDrop(): void;
  collapse(): void;
  /** Folds a waiting card to the compact island without answering it. */
  foldApproval(): void;
  setFocus(id: string): void;
  openTerminal(): void;
  /** The ↗ button: opens whatever the focused pill points at. */
  openTarget(): void;
  openUrl(url: string): void;
  decide(d: "allow" | "deny"): void;
  /** Answers the question Claude Code asked: question text → chosen label. */
  answer(answers: Record<string, string | string[]>): void;
  /** Hands the pending request back to the terminal. */
  answerInTerminal(): void;
  toggleSound(): void;
  setVolume(v: number): void;
  setAutoClose(seconds: number): void;
  openSettingsWindow(): void;
  blip(): void;
}

export interface ViewHost {
  el: HTMLElement;
  sync(): void;
  /** Called when the view becomes active, for views with a text field. */
  focus?(): void;
  /** Called every frame while the view is on screen. True = needs another frame. */
  tick?(nowMs: number): boolean | void;
  /** Ctrl+O / Ctrl+E while the view is on screen (island/shortcuts.ts). */
  command?(command: ViewCommand): void;
}

// ── Shared pieces ─────────────────────────────────────────────────────────────

function card(wash: Wash, ...children: (Node | string)[]): HTMLElement {
  const el = h("div", { class: wash ? "card wash" : "card" }, ...children);
  if (wash) el.style.setProperty("--wash", washRGBA(wash));
  return el;
}

function btn(
  label: string | Msg,
  kind: "primary" | "secondary",
  onClick: () => void,
  kbd?: string,
): HTMLElement {
  return h(
    "button",
    { class: `btn ${kind}`, onclick: onClick },
    h("span", { text: label }),
    kbd ? h("span", { class: "kbd", text: kbd }) : null,
  );
}

/** AgentWho — coloured dot + task name + grey label. */
function agentWho(task: AgentTask | null, label: string): HTMLElement {
  const row = h("div", { class: "who-row" });
  if (task) {
    row.append(dot(task.color, 8), h("span", { class: "n", text: task.name }));
  }
  row.append(h("span", { text: label }));
  return row;
}

function stack(padLeft: number, padRight: number, ...children: Node[]): HTMLElement {
  const el = h("div", { class: "stack" }, ...children);
  el.style.padding = `4px ${padRight}px 4px ${padLeft}px`;
  return el;
}

// ── Header ────────────────────────────────────────────────────────────────────

export function buildHeader(actions: ViewActions): ViewHost {
  const tabHome = h("button", { class: "tab", title: tl("Overview"), onclick: () => go("overview") }, svg(ICONS.house, 13));
  const tabChat = h("button", { class: "tab", title: tl("Ask"), onclick: () => go("prompt") }, svg(ICONS.bubble, 13));
  const tabDrop = h("button", { class: "tab", title: tl("Drop"), onclick: () => go("upload") }, svg(ICONS.plus, 13));
  // The notifications Ciel keeps: there while some wait, or while on their list.
  const tabBadge = h("span", { class: "tab-badge" });
  const tabNotes = h("button", { class: "tab tab-notes", title: tl("Notifications"), onclick: () => go("notifications") }, svg(ICONS.bell, 13), tabBadge);

  const gearBtn = h("button", { title: tl("Settings"), onclick: () => go("settings") }, svg(ICONS.gear, 14));
  const soundBtn = h("button", { title: tl("Mute"), onclick: () => actions.toggleSound() }, svg(ICONS.speakerOn, 14));
  // Plan usage pills (off by default): before the gear, Claude first, as on the Mac.
  const claudePill = buildPlanPill(false);
  const codexPill = buildPlanPill(true);
  const planPills = h("div", { class: "plan-pills" }, claudePill.el, codexPill.el);
  let codexShown = false;

  function go(v: IslandViewName) {
    actions.blip();
    actions.setView(v);
  }

  const el = h(
    "div",
    { id: "header" },
    h("div", { class: "tabs" }, tabHome, tabChat, tabDrop, tabNotes),
    h("div", { class: "header-actions" }, planPills, gearBtn, soundBtn),
  );
  const headerActions = el.lastElementChild as HTMLElement;

  return {
    el,
    sync() {
      const v = State.view;
      tabHome.classList.toggle("on", v === "overview" || v === "empty");
      tabChat.classList.toggle("on", v === "prompt");
      tabDrop.classList.toggle("on", v === "upload");
      const waiting = State.notifications.length;
      tabNotes.classList.toggle("on", v === "notifications");
      tabNotes.style.display = waiting || v === "notifications" ? "" : "none";
      tabBadge.textContent = waiting > 99 ? "99+" : String(waiting);
      tabBadge.style.display = waiting ? "" : "none";
      gearBtn.classList.toggle("on", v === "settings");
      clear(gearBtn);
      gearBtn.append(svg(v === "settings" ? ICONS.gearFill : ICONS.gear, 14));
      clear(soundBtn);
      soundBtn.append(svg(State.settings.soundEnabled ? ICONS.speakerOn : ICONS.speakerOff, 14));
      syncPlanPills();
      el.style.opacity = v === "confused" ? "0" : "1";
    },
  };

  function syncPlanPills() {
    const claudeOn = claudePillVisible();
    const codexOn = codexPillVisible();
    claudePill.el.style.display = claudeOn ? "" : "none";
    codexPill.el.style.display = codexOn ? "" : "none";
    planPills.classList.toggle("on", claudeOn || codexOn);
    // Both pills: the right side tightens so it still clears the screen edge.
    headerActions.classList.toggle("both-plans", claudeOn && codexOn);
    if (claudeOn) claudePill.sync();
    if (codexOn) codexPill.sync();
    // Codex is asked when its pill comes into view (stale answers only).
    const shown = codexOn && State.mode === "expanded";
    if (shown && !codexShown) refreshCodexPlanUsage();
    codexShown = shown;
  }
}

// ── Overview ──────────────────────────────────────────────────────────────────

/**
 * One pill in detail, on either side of the overview: its live feed, a diff
 * from it, or its own card. `wide`: it has the island to itself, and shows
 * more — the files the session edited and its plan's usage, a card's detail
 * beside it, longer lists.
 */
interface DetailPane {
  el: HTMLElement;
  readonly taskId: string | null;
  /** Its ↗ makes sense: no diff and no detail open over the card. */
  readonly jumpable: boolean;
  /** It shows a plan's usage, whose countdowns move every 30 s. */
  readonly showsPlan: boolean;
  sync(task: AgentTask | null, wide: boolean): void;
  tick(nowMs: number): boolean;
  /** Ctrl+O on the highlighted row, Ctrl+E on the latest diff. */
  command(command: ViewCommand): void;
  /** Closes the diff it shows; `quiet` skips the redraw. True when there was one. */
  closeDiff(quiet?: boolean): boolean;
  /** Ctrl+↓ Ctrl+↑ walk the focused pane's list (cardItemCount / cardSelection). */
  syncSelection(focused: boolean): void;
}

function buildDetailPane(actions: ViewActions): DetailPane {
  /** The diff open here (a FileDiff id), as activeDiffId on macOS. */
  let activeDiffId: number | null = null;
  let taskId: string | null = null;
  let detailOpen = false;
  let mode: "ticker" | "card" | "diff" | "player" | "finance" | null = null;
  let cardKey = "";
  let editsKey = "";
  let showsPlan = false;
  /** The list row highlighted at the last sync, to scroll only when it moves. */
  let shownSelection: number | null = null;

  const openDiff = (diffId: number) => {
    actions.blip();
    activeDiffId = diffId;
    State.notify();
  };
  const ticker = new Ticker(openDiff);
  const who = h("div", { class: "who" });
  // With the room for them, beside the feed: its plan's usage, and the files
  // the session edited.
  const edits = h("div", { class: "edits" });
  const plan = new PlanCard("side");
  const side = h("div", { class: "side" }, plan.el, edits);
  const tickerBody = h("div", { class: "card-body" }, h("div", { class: "feed" }, who, ticker.el), side);
  const el = h("div", { class: "left-body" });
  // The players' cards and Finance's are kept and updated in place: the progress
  // bar runs on, and a slider or a form must not be rebuilt under the pointer.
  const playerCards = new Map<string, SpotifyCardHost>();
  let financeCard: ReturnType<typeof buildFinanceCard> | null = null;

  const hooks: IntegrationCardHooks = {
    get detailOpen() {
      return detailOpen;
    },
    wide: false,
    openDetail() {
      detailOpen = true;
      cardKey = "";
      State.notify();
    },
    closeDetail() {
      detailOpen = false;
      cardKey = "";
      State.notify();
    },
    openSettings: () => actions.openSettingsWindow(),
  };

  /** Puts `child` in the pane, unless it is there already. */
  const showOnly = (next: typeof mode, child: HTMLElement) => {
    if (mode === next && child.parentElement === el) return;
    clear(el);
    el.append(child);
    mode = next;
    cardKey = "";
  };

  /** The files the session edited, newest first: a press opens the diff. How many shown. */
  function syncEdits(task: AgentTask, wide: boolean, max: number): number {
    const list = wide ? recentEdits(State.sessionDiffs.get(task.id) ?? [], max) : [];
    const key = list.map((d) => `${d.id}:${d.added}:${d.removed}`).join("|");
    if (key === editsKey) return list.length;
    editsKey = key;
    clear(edits);
    for (const d of list) {
      edits.append(h(
        "button",
        { class: "edit-row", title: d.path, onclick: () => openDiff(d.id) },
        svg(ICONS.doc, 9),
        h("span", { class: "edit-name", text: d.path.split(/[\\/]/).pop() || d.path }),
        h("span", { class: "plus", text: `+${d.added}` }),
        h("span", { class: "minus", text: `−${d.removed}` }),
      ));
    }
    return list.length;
  }

  const closeDiff = (quiet = false) => {
    if (activeDiffId == null) return false;
    activeDiffId = null;
    if (!quiet) State.notify();
    return true;
  };

  return {
    el,
    get taskId() {
      return taskId;
    },
    get jumpable() {
      return taskId != null && !detailOpen && mode !== "diff";
    },
    get showsPlan() {
      return showsPlan;
    },
    tick(nowMs) {
      if (mode !== "ticker") return false;
      ticker.tick(nowMs);
      return ticker.animating;
    },
    command(command) {
      if (command === "openSelection") {
        // Ctrl+O: what a click on the highlighted row does.
        if (mode !== "card" || State.cardSelection == null) return;
        openRow(listRows(el)[State.cardSelection]);
        return;
      }
      // Ctrl+E (⌘E, islandToggleDiff): closes the diff that is open, else
      // opens the pill's latest edit.
      if (closeDiff()) return;
      const diffs = taskId ? State.sessionDiffs.get(taskId) : undefined;
      const last = diffs?.[diffs.length - 1];
      if (last?.id == null) return;
      activeDiffId = last.id;
      State.notify();
    },
    closeDiff,
    syncSelection(focused) {
      const rows = mode === "card" ? listRows(el) : [];
      if (!focused) {
        highlightRow(rows, null, false);
        return;
      }
      State.cardItemCount = rows.length;
      if (State.cardSelection != null && State.cardSelection >= rows.length) State.cardSelection = null;
      highlightRow(rows, State.cardSelection, State.cardSelection !== shownSelection);
      shownSelection = State.cardSelection;
    },
    sync(task, wide) {
      if ((task?.id ?? null) !== taskId) {
        taskId = task?.id ?? null;
        detailOpen = false;
        activeDiffId = null;
        cardKey = "";
        editsKey = "";
        mode = null;
      }
      el.classList.toggle("wide", wide);
      hooks.wide = wide;
      showsPlan = false;
      if (!task) {
        clear(el);
        mode = null;
        return;
      }

      // A diff that has since been dropped (cap, expiry, session end) just closes.
      const diff = activeDiffId != null ? State.findDiff(task.id, activeDiffId) : null;
      if (!diff) activeDiffId = null;

      if (diff) {
        const key = `diff~${task.id}~${diff.id}`;
        if (key !== cardKey) {
          clear(el);
          el.append(buildDiffCard(diff, {
            dismiss: () => {
              actions.blip();
              closeDiff();
            },
            open: (path) => void Bridge.openFileInVSCode(path),
          }));
          mode = "diff";
          cardKey = key;
        }
      } else if (hasSessionTicker(task)) {
        // A workspace or agent pill with a live session keeps the ticker; every
        // other pill shows its own card, exactly like IntegrationCardView.
        showOnly("ticker", tickerBody);
        clear(who);
        // The agent's name is already the pill's: the label says what kind of
        // pill it is, as on the Mac (PillDefinition.sessionSubtitle).
        who.append(
          dot(task.color, 7),
          h("span", { class: "name", text: task.name }),
          h("span", { class: "tool", text: t(sessionSubtitle(task.id)) }),
        );
        if (task.steps.length > 1) {
          who.append(h("span", {
            class: "count",
            text: `${Math.min(task.stepIndex + 1, task.steps.length)}/${task.steps.length}`,
          }));
        }
        ticker.sync(task);
        const which = wide ? pillPlan(task.id) : null;
        const usage = which === "codex" ? State.codexPlanUsage : which === "claude" ? State.planUsage : null;
        showsPlan = usage != null;
        plan.el.style.display = showsPlan ? "" : "none";
        if (showsPlan) plan.sync(Date.now(), which === "codex");
        // Under the plan's two gauges there is room for three files; alone, four.
        const shown = syncEdits(task, wide, showsPlan ? 3 : 4);
        tickerBody.classList.toggle("with-side", showsPlan || shown > 0);
      } else if (playerFor(task.id)) {
        // A music pill (Spotify, YouTube Music): its own card for every
        // state — playing, idle, not there.
        const source = playerFor(task.id)!;
        let player = playerCards.get(source.id);
        if (!player) {
          player = buildPlayerCard(source);
          playerCards.set(source.id, player);
        }
        showOnly("player", player.el);
        player.sync();
      } else if (task.id === FINANCE_ID) {
        financeCard ??= buildFinanceCard();
        if (mode !== "finance") {
          showOnly("finance", financeCard.el);
          financeCard.reset();
        }
        financeCard.sync();
      } else {
        const info = State.integrations[task.id];
        const key = [
          language(), task.id, task.color, detailOpen, wide, task.state, task.steps.join("|"),
          info?.loaded, info?.error, info?.configured,
          JSON.stringify(info?.data ?? {}),
        ].join("~");
        if (key !== cardKey) {
          clear(el);
          el.append(renderIntegrationCard(task, hooks));
          mode = "card";
          cardKey = key;
        }
      }
    },
  };
}

/**
 * The open island's home (core/overview.ts decides the layout): one pill has
 * it all, in detail; two are both in detail, each in its own place (the main
 * pill beside Ciel); from three on, the focused pill is in detail and the
 * others are listed on the right, a press on one bringing it over.
 */
function buildOverview(actions: ViewActions): ViewHost {
  const main = buildDetailPane(actions);
  const second = buildDetailPane(actions);
  // Opened from a plan pill in the header: stands in for the left card.
  const plan = new PlanCard();
  let planTimer: number | null = null;
  let planShown = false;
  let layout: OverviewLayout = "list";

  /** A pane's ↗: whatever its pill points at (the focused pill's, so it gets the focus). */
  const openPane = (pane: DetailPane) => {
    if (pane.taskId && State.focusId !== pane.taskId) State.setFocus(pane.taskId);
    actions.openTarget();
  };
  const jumpButton = (pane: DetailPane) => h(
    "button",
    { class: "icon-btn jump", title: tl("Open"), onclick: () => openPane(pane) },
    svg(ICONS.arrowUpRight, 8),
  );
  const leftHolder = h("div", { class: "pane-holder" }, main.el);
  const jump = jumpButton(main);
  const left = card(null, leftHolder, jump);
  const pills = h("div", { class: "pills" });
  const rightHolder = h("div", { class: "pane-holder" }, pills);
  const jumpRight = jumpButton(second);
  const right = card(null, rightHolder, jumpRight);
  // The music pills in the list are kept and updated in place, like their cards.
  let playerPills: SpotifyPillHost[] = [];

  const el = h("div", { class: "view overview" },
    h("div", { class: "left" }, left),
    h("div", { class: "right" }, right),
  );

  let pillIds = "";

  /** The pane the keys go to: the one showing the focused pill. */
  const focusedPane = () => (layout === "duo" && second.taskId != null && second.taskId === State.focusTask?.id ? second : main);

  // Two in detail: a press in one gives its pill the focus, silently — the
  // keys, the ↗ and Ciel's mood follow it.
  for (const [cardEl, pane] of [[left, main], [right, second]] as const) {
    cardEl.addEventListener("mousedown", () => {
      if (layout !== "duo" || !pane.taskId || State.focusId === pane.taskId) return;
      State.setFocus(pane.taskId);
      State.notify();
    }, true);
  }

  // Leaving the overview or folding the island closes the diffs, as on macOS.
  State.subscribe(() => {
    if (State.view !== "overview" || State.mode !== "expanded") {
      main.closeDiff(true);
      second.closeDiff(true);
    }
  });
  // Escape steps back out of a diff before it closes the island.
  window.addEventListener(
    "keydown",
    (e) => {
      if (e.key !== "Escape" || State.view !== "overview") return;
      const pane = focusedPane();
      if (pane.closeDiff() || (pane === main ? second : main).closeDiff()) e.stopImmediatePropagation();
    },
    true,
  );

  /** The countdowns move every 30 s while a plan shows, and only then. */
  function syncPlanTimer(open: boolean) {
    const stop = () => {
      if (planTimer != null) window.clearInterval(planTimer);
      planTimer = null;
    };
    if (!open) return stop();
    if (planTimer != null) return;
    planTimer = window.setInterval(() => {
      if ((planCardOpen() || main.showsPlan) && State.mode === "expanded") State.notify();
      else stop();
    }, 30_000);
  }

  /** A duo's pane wears a hairline of its pill's colour while it has the focus. */
  const ring = (cardEl: HTMLElement, task: AgentTask | null, on: boolean) => {
    cardEl.style.borderColor = on && task ? `${task.color}59` : "";
  };

  return {
    el,
    tick(nowMs: number) {
      const a = main.tick(nowMs);
      const b = layout === "duo" ? second.tick(nowMs) : false;
      return a || b;
    },
    command(command) {
      focusedPane().command(command);
    },
    sync() {
      const p = planOverview(State.tasks, State.focusId);
      layout = p.layout;
      el.classList.toggle("solo", layout === "solo");
      el.classList.toggle("duo", layout === "duo");

      const planOpen = planCardOpen();
      if (planOpen !== planShown) {
        planShown = planOpen;
        clear(leftHolder);
        leftHolder.append(planOpen ? plan.el : main.el);
      }
      if (planOpen) plan.sync();
      main.sync(p.left, layout === "solo");
      second.sync(p.right, false);
      syncPlanTimer(planOpen || main.showsPlan);

      // The right card: the second pill of a duo in detail, else the list.
      const rightChild = layout === "duo" ? second.el : pills;
      if (rightChild.parentElement !== rightHolder) {
        clear(rightHolder);
        rightHolder.append(rightChild);
      }
      jump.style.display = planShown || !main.jumpable ? "none" : "";
      jumpRight.style.display = layout === "duo" && second.jumpable ? "" : "none";

      const focusId = State.focusTask?.id ?? null;
      ring(left, p.left, layout === "duo" && main.taskId === focusId);
      ring(right, p.right, layout === "duo" && second.taskId === focusId);
      const keys = focusedPane();
      main.syncSelection(keys === main && !planShown);
      second.syncSelection(keys === second);

      if (layout !== "list") return;
      // Finance's pill says how the month goes: its label is part of the key.
      const pillKey = p.list
        .map((t) => `${t.id}:${t.color}:${t.pillBadge ?? ""}${t.id === FINANCE_ID ? `:${t.name}` : ""}`)
        .join("|");
      if (pillKey !== pillIds) {
        pillIds = pillKey;
        clear(pills);
        playerPills = [];
        for (const t of p.list) {
          const source = playerFor(t.id);
          if (source) {
            const pill = buildPlayerPill(t, source, () => actions.setFocus(t.id));
            playerPills.push(pill);
            pills.append(pill.el);
          } else {
            pills.append(buildPill(t, actions));
          }
        }
        pruneMiniBots();
      }
      for (const pill of playerPills) pill.sync();
    },
  };
}

/**
 * IntegrationCardView.agentSessionActive: a workspace tool or an agent — or
 * any other tagged agent — with something going on.
 */
export function hasSessionTicker(task: AgentTask): boolean {
  // A pill made for an agent's session (Gemini CLI, Codex… not declared) only
  // exists while that session does: it keeps the ticker from the first event.
  if (task.source === "agent" && !task.isIntegration) return true;
  const category = pillDefinition(task.id)?.category;
  const isSession = category === "workspace" || category === "agent" ||
    (category == null && task.id.startsWith("agent_"));
  return isSession && (task.state !== "idle" || task.steps.length > 0);
}

function buildPill(task: AgentTask, actions: ViewActions): HTMLElement {
  const label = task.id === "integration_claude" ? pillDefinition(task.id)!.name : task.name;
  const canvas = createMiniBot(task, 24);
  const pill = h(
    "div",
    { class: "pill", onclick: () => actions.setFocus(task.id) },
    canvas,
    h("span", { class: "lbl", text: label }),
  );
  pill.style.borderColor = `${task.color}24`;
  pill.addEventListener("mouseenter", () => {
    pill.style.background = `${task.color}2e`;
    pill.style.borderColor = `${task.color}8c`;
    pill.style.boxShadow = `0 2px 10px ${task.color}59`;
    (pill.querySelector(".lbl") as HTMLElement).style.color = lighten(task.color, 0.3);
  });
  pill.addEventListener("mouseleave", () => {
    pill.style.background = "";
    pill.style.borderColor = `${task.color}24`;
    pill.style.boxShadow = "";
    (pill.querySelector(".lbl") as HTMLElement).style.color = "";
  });

  if (task.pillBadge) {
    const colors = { approval: "#F5A524", finished: "#22C55E", error: "#F4505E" } as const;
    const icons = { approval: ICONS.bang, finished: ICONS.check, error: ICONS.xmark } as const;
    const inner = h("i", { style: `background:${colors[task.pillBadge]}` }, svg(icons[task.pillBadge], 6, { stroke: task.pillBadge === "finished" ? 3 : 0 }));
    const badge = h("div", { class: "pill-badge" }, inner);
    badge.style.boxShadow = `0 0 4px ${colors[task.pillBadge]}99`;
    pill.append(badge);
  }
  return pill;
}

function lighten(hex: string, amount: number): string {
  const v = parseInt(hex.replace("#", ""), 16);
  const c = [(v >> 16) & 255, (v >> 8) & 255, v & 255].map((x) =>
    Math.min(255, Math.round(x + amount * 255)),
  );
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

// ── Empty ─────────────────────────────────────────────────────────────────────

function buildEmpty(actions: ViewActions): ViewHost {
  const body = h(
    "div",
    { class: "stack", style: "padding:0 18px 0 118px;flex-direction:row;align-items:center;gap:16px" },
    h(
      "div",
      { style: "display:flex;flex-direction:column;gap:5px" },
      h("div", { class: "title", text: tl("Nothing running right now.") }),
      h("div", { class: "sub", text: tl("Drop a file or window, or ask me anything.") }),
    ),
    h("div", { class: "grow" }),
    btn(tl("Ask Claude"), "primary", () => actions.setView("prompt")),
  );
  return { el: h("div", { class: "view" }, card(null, body)), sync() {} };
}

// ── Approval ──────────────────────────────────────────────────────────────────

/** How long a fresh permission card ignores clicks on its buttons. */
const CLICK_GUARD_MS = 600;

/**
 * The ⌃ in the corner of a waiting card: folds the island to its compact size
 * and leaves the request waiting — nothing is answered (Mac #290). Opening the
 * island again brings the card back.
 */
function foldButton(actions: ViewActions): HTMLElement {
  return h(
    "button",
    { class: "icon-btn fold", title: tl("Later — keep it waiting"), onclick: () => actions.foldApproval() },
    svg(ICONS.chevronUp, 8, { stroke: 2.4 }),
  );
}

function buildApproval(actions: ViewActions): ViewHost {
  const who = h("div");
  const code = h("div", { class: "code" });
  const row = h("div", { class: "actions" });
  const el = h("div", { class: "view" },
    card("amber", stack(116, 16, who, code, row), foldButton(actions)));
  let rowKey = "";
  // The card pops up under a cursor that was busy with something else: a click
  // meant for the window underneath must not land on Allow. Clicks in the first
  // moments after a new request appears are ignored.
  let shownFor: string | null = null;
  let shownAt = 0;
  const guarded = (d: "allow" | "deny") => () => {
    if (performance.now() - shownAt < CLICK_GUARD_MS) return;
    actions.decide(d);
  };
  return {
    el,
    sync() {
      const req = State.pendingApproval?.requestId ?? null;
      if (req !== shownFor) {
        shownFor = req;
        shownAt = performance.now();
      }
      clear(who);
      who.append(agentWho(State.focusTask, t("needs permission")));
      // The whole point of approving here rather than in the terminal: this line
      // is the command, the file path or the URL being authorised, not just the
      // name of the tool asking.
      code.textContent = State.pendingApproval?.command || State.pendingApproval?.tool || "…";
      // Two buttons, built once. Rebuilding them between a mouse-down and a
      // mouse-up would swallow the click, and there is nothing left to vary:
      // "Always" is gone until the remembered-rules list exists to back it.
      if (rowKey === "built") return;
      rowKey = "built";
      clear(row);
      row.append(
        btn(tl("Deny"), "secondary", guarded("deny"), "N"),
        btn(tl("Allow"), "primary", guarded("allow"), "Y"),
      );
    },
  };
}

// ── Question ──────────────────────────────────────────────────────────────────

function buildQuestion(actions: ViewActions): ViewHost {
  const who = h("div");
  const title = h("div", { class: "title question-text" });
  const row = h("div", { class: "actions options" });
  const fold = foldButton(actions);
  const el = h("div", { class: "view" }, card("cyan", stack(116, 16, who, title, row), fold));

  // Where we are in the request on screen: which question, what is answered so
  // far, and what is ticked in a pick-several question.
  let requestId = "";
  let index = 0;
  let answers: Record<string, string | string[]> = {};
  let picked = new Set<string>();
  // The buttons are only rebuilt when what they show changes: rebuilding them
  // between a mouse-down and a mouse-up would swallow the click.
  let rowKey = "";

  const next = (question: string, answer: string | string[], total: number) => {
    answers[question] = answer;
    picked = new Set();
    index += 1;
    if (index >= total) actions.answer(answers);
    else State.notify();
  };

  return {
    el,
    sync() {
      const questions = State.pendingApproval?.questions;
      clear(who);
      // Only a request that is waiting can be folded away and come back.
      fold.style.display = State.pendingApproval ? "" : "none";

      // A question that arrived as a notification has nothing to pick from.
      if (!questions) {
        who.append(agentWho(State.focusTask, t("is asking a question")));
        const task = State.focusTask;
        title.textContent = (task && lastTextStep(task.steps)) ?? t("Claude needs an answer.");
        if (rowKey !== "terminal") {
          rowKey = "terminal";
          clear(row);
          row.append(h("div", { class: "sub", text: tl("Answer it in your terminal.") }));
        }
        return;
      }

      if (State.pendingApproval!.requestId !== requestId) {
        requestId = State.pendingApproval!.requestId;
        index = 0;
        answers = {};
        picked = new Set();
      }
      const q = questions[Math.min(index, questions.length - 1)];
      const asking = questions.length > 1
        ? t("is asking ({index} of {total})", { index: index + 1, total: questions.length })
        : t("is asking");
      who.append(agentWho(State.focusTask, asking));
      title.textContent = q.question;
      title.title = q.question;

      const key = `${requestId}:${index}:${[...picked].join("|")}`;
      if (rowKey === key) return;
      rowKey = key;
      clear(row);
      for (const option of q.options) {
        const on = picked.has(option.label);
        const button = btn(option.label, on ? "primary" : "secondary", () => {
          if (!q.multiSelect) {
            next(q.question, option.label, questions.length);
            return;
          }
          if (on) picked.delete(option.label);
          else picked.add(option.label);
          State.notify();
        });
        if (option.description) button.title = option.description;
        row.append(button);
      }
      if (q.multiSelect) {
        const done = btn(tl("Done"), "primary", () => {
          if (picked.size > 0) next(q.question, [...picked], questions.length);
        });
        if (picked.size === 0) done.classList.add("off");
        row.append(done);
      }
      row.append(
        h("button", {
          class: "link-btn",
          style: "color:#8e939c",
          text: tl("Answer in terminal"),
          onclick: () => actions.answerInTerminal(),
        }),
      );
    },
  };
}

// ── Error ─────────────────────────────────────────────────────────────────────

function buildError(actions: ViewActions): ViewHost {
  const who = h("div");
  const title = h("div", { class: "title" });
  const detail = h("div", { class: "detail" });
  const row = h("div", { class: "actions" },
    btn(tl("Retry"), "primary", () => actions.setView(State.defaultView())),
    btn(tl("Open in n8n"), "secondary", () => actions.openUrl("")),
  );
  const el = h("div", { class: "view" }, card("red", stack(116, 16, who, title, detail, row)));
  return {
    el,
    sync() {
      const task = State.focusTask;
      clear(who);
      // agentWho already shows an agent's name: its label is just the kind.
      const whoLabel = task?.source === "n8n" ? "n8n" : task?.source === "agent" ? t("Agent") : "Claude Code";
      who.append(agentWho(task, whoLabel));
      title.textContent = task?.source === "n8n" ? t("Workflow stopped.") : t("Session stopped on an error.");
      detail.textContent = (task && lastTextStep(task.steps)) ?? t("No detail available.");
    },
  };
}

// ── Finished ──────────────────────────────────────────────────────────────────

function buildFinished(actions: ViewActions): ViewHost {
  const who = h("div");
  const title = h("div", { class: "title one-line" });
  const open = btn(tl("Open terminal"), "primary", () => actions.openTerminal());
  const row = h("div", { class: "actions" },
    open,
    btn(tl("OK"), "secondary", () => actions.collapse()),
  );
  const el = h("div", { class: "view" }, card("green", stack(116, 16, who, title, row)));
  return {
    el,
    sync() {
      clear(who);
      const agent = State.focusTask?.source === "agent";
      who.append(agentWho(State.focusTask, agent ? t("finished") : t("Claude Code finished")));
      // The final message, else the last step that is not a diff (FinishedView).
      const task = State.focusTask;
      title.textContent = task?.finalLine || (task && lastTextStep(task.steps)) || t("Session finished");
      // Sessions from the Claude desktop app live there, not in a terminal.
      const label = task?.id === "agent_claude-desktop" ? t("Open Claude") : t("Open terminal");
      const span = open.firstElementChild as HTMLElement;
      if (span.textContent !== label) span.textContent = label;
    },
  };
}

// ── Confused ──────────────────────────────────────────────────────────────────

function buildConfused(): ViewHost {
  const body = h(
    "div",
    { class: "stack", style: "padding:0 18px 0 128px" },
    h("div", { class: "title", text: tl("Too many hits at once.") }),
    h("div", { class: "sub", text: tl("Give me a sec — back to work in three seconds.") }),
  );
  return { el: h("div", { class: "view" }, card("pink", body)), sync() {} };
}

// ── Note ──────────────────────────────────────────────────────────────────────

function buildNote(): ViewHost {
  const title = h("div", { class: "title" });
  const el = h("div", { class: "view" }, card(null, h("div", { class: "stack", style: "padding:0 18px 0 98px" }, title)));
  return {
    el,
    sync() {
      title.textContent = State.noteMessage ?? "";
    },
  };
}

// ── In-island settings ────────────────────────────────────────────────────────

function buildSettings(actions: ViewActions): ViewHost {
  const soundSwitch = h("button", { class: "switch", onclick: () => actions.toggleSound() });
  const volume = h("input", {
    type: "range", min: "0", max: "0.2", step: "0.005",
    oninput: (e: Event) => actions.setVolume(Number((e.target as HTMLInputElement).value)),
  }) as HTMLInputElement;
  const autoLabel = h("span", {});
  // 0: never closes by itself.
  const segChoices = [10, 15, 30, 0];
  const segButtons = segChoices.map((s) =>
    h("button", { onclick: () => actions.setAutoClose(s), title: s === 0 ? tl("Never") : "" }, s === 0 ? "∞" : `${s}s`),
  );
  const claudeBadge = h("span", { class: "status-badge" });
  const apiBadge = h("span", { class: "status-badge" });

  const rows = h(
    "div",
    { class: "settings-rows" },
    h("div", { class: "settings-row" }, soundSwitch, h("span", { text: tl("Sound") }), volume),
    h(
      "div",
      { class: "settings-row" },
      svg(ICONS.timer, 12),
      autoLabel,
      h("div", { class: "seg" }, ...segButtons),
    ),
    h(
      "div",
      { class: "settings-row", style: "gap:14px" },
      claudeBadge,
      apiBadge,
      h("div", { class: "grow" }),
      h("button", {
        class: "link-btn",
        style: "color:#8e939c;font-size:11.5px",
        text: tl("Settings…"),
        onclick: () => actions.openSettingsWindow(),
      }),
    ),
  );

  const el = h("div", { class: "view" },
    card(null, h("div", { class: "stack", style: "padding:14px 16px 14px 84px" }, rows)));

  return {
    el,
    sync() {
      const s = State.settings;
      soundSwitch.classList.toggle("on", s.soundEnabled);
      volume.value = String(s.soundVolume);
      volume.style.opacity = s.soundEnabled ? "1" : "0.4";
      autoLabel.textContent = s.autoCloseInterval > 0
        ? t("Auto-close · {seconds}s", { seconds: Math.round(s.autoCloseInterval) })
        : t("Auto-close · never");
      segButtons.forEach((b, i) => b.classList.toggle("on", s.autoCloseInterval === segChoices[i]));
      clear(claudeBadge);
      claudeBadge.append(
        dot(s.hooksInstalled ? "#22C55E" : "#F4505E", 6),
        h("span", { text: "Claude Code" }),
      );
      clear(apiBadge);
      apiBadge.append(dot("#F4505E", 6), h("span", { text: "API" }));
    },
  };
}

// ── Placeholders filled in later stages ───────────────────────────────────────

function buildPlaceholder(title: Msg, sub: string): ViewHost {
  const body = h(
    "div",
    { class: "stack", style: "padding:0 18px 0 118px" },
    h("div", { class: "title", text: title }),
    h("div", { class: "sub", text: sub }),
  );
  return { el: h("div", { class: "view" }, card(null, body)), sync() {} };
}

// ── Registry ──────────────────────────────────────────────────────────────────

export function buildViews(
  actions: ViewActions,
  onChatHeightChange: () => void,
): Map<IslandViewName, ViewHost> {
  const map = new Map<IslandViewName, ViewHost>();
  map.set("overview", buildOverview(actions));
  map.set("empty", buildEmpty(actions));
  map.set("approval", buildApproval(actions));
  map.set("question", buildQuestion(actions));
  map.set("error", buildError(actions));
  map.set("finished", buildFinished(actions));
  map.set("confused", buildConfused());
  map.set("note", buildNote());
  map.set("settings", buildSettings(actions));
  map.set("prompt", buildPrompt(onChatHeightChange));
  map.set("upload", buildUpload());
  map.set("uploading", buildUploading());
  map.set("choose", buildChoose(actions));
  map.set("recap", buildRecap(actions));
  map.set("notifications", buildInbox(actions));
  // Not in the Windows v1: sending a file by email, window attach + web result.
  map.set("mail", buildPlaceholder(tl("Sending by email isn't in this version."), ""));
  map.set("searching", buildPlaceholder(tl("Claude is searching…"), ""));
  map.set("result", buildPlaceholder(tl("Result"), ""));
  return map;
}
