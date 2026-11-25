// The dock's agents and music, island side: the island knows the sessions
// (core/sessions.ts) and what plays, and tells the dock's window whenever that
// changes (src/dock). A click on the dock's agents opens the island here.
// Nothing is sent while the dock is off.

import { emitToWindow, onEvent } from "../core/bridge";
import { DOCK_EVENTS, dockSnapshot } from "../core/dock";
import { Sessions } from "../core/sessions";
import { Spotify } from "../core/spotify";
import { State } from "../core/state";
import type { IslandViewName } from "../core/layout";

/** Label of the dock's window (src-tauri/src/dock/mod.rs LABEL). */
const WINDOW = "dock";

export interface DockHost {
  alert(view: IslandViewName): void;
}

export function registerDock(host: DockHost) {
  let pushed = "";
  let pending = 0;

  const push = (force = false) => {
    pending = 0;
    if (!State.settings.dock?.on) {
      pushed = "";
      return;
    }
    const waiting = State.pendingApproval;
    const rows = Sessions.rows(waiting ? { pillId: waiting.pillId, sessionId: waiting.sessionId } : null, Date.now());
    const snapshot = dockSnapshot(rows, Spotify.state);
    const key = JSON.stringify(snapshot);
    if (key === pushed && !force) return;
    pushed = key;
    void emitToWindow(WINDOW, DOCK_EVENTS.island, snapshot);
  };

  // The island tells its listeners of every change, often several a frame: once is enough.
  State.subscribe(() => {
    if (!pending && State.settings.dock?.on) pending = window.setTimeout(() => push(), 150);
  });
  void onEvent<null>(DOCK_EVENTS.ready, () => push(true));
  void onEvent<string>(DOCK_EVENTS.open, (what) => {
    host.alert(what === "agents" ? "agents" : State.defaultView());
  });
}
