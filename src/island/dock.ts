// The dock's agents and music, island side: the island knows the sessions
// (core/sessions.ts) and what plays, and tells the docks' windows (one a
// screen on GNOME) whenever that changes (src/dock). A click on the dock's
// agents opens the island here. On GNOME the panel over the dock
// (src/dock/panel.ts) shows the island's notifications and calendar: they go
// to it too once it is there, and what it opens or dismisses comes back here.
// What plays goes to the dock too, unless the user keeps it in the island
// (Settings → Dock): then the island neither shows up for it nor sings its
// words. Nothing is sent while the dock is off.

import { Bridge, broadcast, onEvent } from "../core/bridge";
import { DOCK_EVENTS, commandName, dockSnapshot, type DockMusic, type DockPanelSnapshot, type DockRequest } from "../core/dock";
import { Sessions } from "../core/sessions";
import { Spotify, currentArtwork } from "../core/spotify";
import { YtMusic, ytCover } from "../core/ytmusic";
import { Lyrics } from "../tools/lyrics";
import { State } from "../core/state";
import type { IslandViewName } from "../core/layout";
import { settle } from "../views/inbox";
import { t } from "../i18n/i18n";

const CALENDAR_ID = "integration_calendar";

export interface DockHost {
  alert(view: IslandViewName): void;
  /** The user's click on Allow or Deny in the dock, for the request that waits now (island.ts checks it). */
  decide(requestId: string, decision: "allow" | "deny"): void;
}

/** A dock has said it is there: it shows (Rust makes none it can't show). */
let docked = false;

/** What plays is the dock's: the island doesn't come up for it or sing its words. */
export function musicInDock(): boolean {
  const dock = State.settings.dock;
  return docked && !!dock?.on && dock.music !== false;
}

/** The player that plays, else the one that last did, with the song's words. */
function music(): DockMusic | null {
  if (!musicInDock()) return null;
  const players = [{ source: "spotify", state: Spotify.state }, { source: "ytmusic", state: YtMusic.state }] as const;
  const pick = players.find((p) => p.state.playing && p.state.track) ?? players.find((p) => p.state.track);
  if (!pick) return null;
  // The island's chip doesn't ask for them while the dock has the music.
  Lyrics.want(pick.state);
  return { ...pick, lyrics: Lyrics.timed(pick.state) };
}

/** A session's line as the dock shows it: of a command, only its name (the island keeps it whole). */
function dockLine(line: string): string {
  const runs = `${t("Runs")} · `;
  if (!line.startsWith(runs)) return line;
  const name = commandName(line.slice(runs.length));
  return name ? runs + name : t("Runs");
}

/** The request that waits, with whose it is, as the dock shows it. */
function request(): DockRequest | null {
  const req = State.pendingApproval;
  if (!req) return null;
  const row = Sessions.rows({ pillId: req.pillId, sessionId: req.sessionId }, Date.now()).find((r) => r.phase === "asking");
  const pill = State.tasks.find((x) => x.id === req.pillId);
  return {
    requestId: req.requestId,
    agent: row?.agent ?? pill?.name ?? "",
    project: row?.project ?? "",
    tool: req.tool,
    command: req.command,
    question: !!req.questions,
  };
}

export function registerDock(host: DockHost) {
  let pushed = "";
  let pending = 0;
  // The cover goes on its own, once a track: it is heavy, the rest changes often.
  let art: string | null | undefined;
  // The panel said it is there (GNOME only), and what it was last told.
  let panel = false;
  let panelPushed = "";

  const pushPanel = (force = false) => {
    const snapshot: DockPanelSnapshot = {
      notifications: State.notifications,
      calendar: State.integrations[CALENDAR_ID] ?? null,
      request: request(),
      codex: State.codexPlanUsage,
    };
    // Not the icons, which an id stands for: the ids, the calendar, the request and the plan.
    const key = JSON.stringify([snapshot.notifications.map((n) => `${n.id}:${n.title}:${n.body}`), snapshot.calendar, snapshot.request, snapshot.codex]);
    if (key === panelPushed && !force) return;
    panelPushed = key;
    void broadcast(DOCK_EVENTS.panel, snapshot);
  };

  const push = (force = false) => {
    pending = 0;
    if (!State.settings.dock?.on) {
      pushed = panelPushed = "";
      art = undefined;
      return;
    }
    if (panel) pushPanel(force);
    const waiting = State.pendingApproval;
    const rows = Sessions.rows(waiting ? { pillId: waiting.pillId, sessionId: waiting.sessionId } : null, Date.now())
      .map((row) => ({ ...row, line: dockLine(row.line) }));
    const playing = music();
    const cover = !playing ? null : playing.source === "spotify" ? currentArtwork(playing.state) : ytCover(playing.state);
    if (cover !== art || force) {
      art = cover;
      void broadcast(DOCK_EVENTS.art, cover);
    }
    const snapshot = dockSnapshot(rows, playing, State.planUsage);
    const key = JSON.stringify(snapshot);
    if (key === pushed && !force) return;
    pushed = key;
    void broadcast(DOCK_EVENTS.island, snapshot);
  };

  // The island tells its listeners of every change, often several a frame: once is enough.
  State.subscribe(() => {
    if (!pending && State.settings.dock?.on) pending = window.setTimeout(() => push(), 150);
  });
  const dockShown = () => {
    docked = true;
    push(true);
  };
  void onEvent<null>(DOCK_EVENTS.ready, dockShown);
  // A dock that started before the island said so to no one: Rust knows.
  void Bridge.dockState().then((s) => s?.shown && dockShown()).catch(() => {});
  void onEvent<null>(DOCK_EVENTS.panelReady, () => {
    panel = true;
    push(true);
  });
  // Only ever the user's own click there; the island makes sure it is for the request that waits.
  void onEvent<{ requestId: string; decision: string }>(DOCK_EVENTS.decide, ({ requestId, decision }) => {
    if (decision === "allow" || decision === "deny") host.decide(requestId, decision);
  });
  void onEvent<{ id: string | null; action: string }>(DOCK_EVENTS.note, ({ id, action }) => {
    const ids = id ? [id] : State.notifications.map((n) => n.id);
    for (const one of ids) settle(one, action);
  });
  void onEvent<string>(DOCK_EVENTS.open, (what) => {
    host.alert(what === "agents" ? "agents" : State.defaultView());
  });
}
