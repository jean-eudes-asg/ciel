// The island as it was before the demo, put aside whole and given back exactly
// when the demo stops: pills, steps, diffs, cards, chat, integration data,
// settings, and where the island was (hidden, compact or open, on which view).

import { State, type SessionDiffStore } from "../core/state";
import type { FsmState } from "../island/fsm";
import type { DemoIsland } from "./host";

/** The parts of AppState the demo changes; everything else it leaves alone. */
const FIELDS = [
  "view", "tasks", "focusId", "stateOverride", "isPinned", "paused", "promptContext", "droppedFile",
  "noteMessage", "searchResult", "chatHistory", "pendingApproval", "focusBeforeApproval", "integrations",
  "planUsage", "codexPlanUsage", "showingPlanDetail", "planDetailIsCodex", "cardSelection",
  "cardItemCount", "settings", "lastActivity",
] as const;

type Field = (typeof FIELDS)[number];
type Fields = { [K in Field]: (typeof State)[K] };

export interface IslandSnapshot {
  fields: Fields;
  diffs: SessionDiffStore;
  fsm: FsmState;
  fsmPinned: boolean;
  petitToHiddenDelay: number;
}

/** A deep copy, so nothing the demo does to the live objects reaches it. */
function copy<T>(value: T): T {
  return value == null ? value : structuredClone(value);
}

/** Takes the island's state aside. The diffs (and their timers) leave State. */
export function takeSnapshot(island: DemoIsland): IslandSnapshot {
  const fields = {} as Record<Field, unknown>;
  for (const key of FIELDS) fields[key] = copy(State[key]);
  return {
    fields: fields as Fields,
    diffs: State.swapSessionDiffs(),
    fsm: island.fsm.state,
    fsmPinned: island.fsm.pinned,
    petitToHiddenDelay: island.fsm.petitToHiddenDelay,
  };
}

/** Puts it all back, the island where it was. */
export function restoreSnapshot(island: DemoIsland, snap: IslandSnapshot) {
  // The demo's diffs go, with their timers; the real ones come back with theirs.
  for (const id of [...State.sessionDiffs.keys()]) State.clearSessionDiffs(id);
  State.swapSessionDiffs(snap.diffs);
  const target = State as unknown as Record<Field, unknown>;
  for (const key of FIELDS) target[key] = copy(snap.fields[key]);
  island.applySettings();
  island.fsm.petitToHiddenDelay = snap.petitToHiddenDelay;

  const view = snap.fields.view;
  switch (snap.fsm) {
    case "hidden":
      island.fsm.forceHidden();
      break;
    case "home":
      island.alert(view);
      break;
    default:
      island.fsm.forcePetit();
      break;
  }
  State.view = view;
  // The usual countdowns start again from here (no-ops while pinned).
  island.dropPin();
  island.fsm.pinned = snap.fsmPinned;
  State.isPinned = snap.fields.isPinned;
  State.notify();
}
