// The Finance folder → the page (src-tauri/src/finance.rs). Nothing is pushed
// and nothing watches the disk: while the pill is on screen — the island open
// on the overview with the Finance pill loaded — the page asks every few
// seconds whether either file changed (two file stats on the Rust side) and
// gets the summary only when one did. Folded, hidden, paused or on another
// view, no timer runs at all.
//
// The pill wears the short text ("62% of budget") the way Spotify's wears the
// track's title.

import { Bridge } from "../core/bridge";
import { FINANCE_ID, POLL_MS, pillText, readSummary, type FinancePoll, type NewExpense } from "../core/finance";
import { pillDefinition } from "../core/pills";
import { Sound } from "../core/sound";
import { State } from "../core/state";

/** What the last answer was computed from; the next poll sends it back. */
let stamp: string | null = null;
let timer: number | null = null;
let polling = false;

export function registerFinanceHandlers() {
  State.subscribe(() => {
    syncFinancePill();
    syncPolling();
  });
}

/** Somewhere the pill or its card can be seen: the open island's overview. */
export function financeOnScreen(): boolean {
  return (
    !State.paused &&
    State.mode === "expanded" &&
    State.view === "overview" &&
    State.tasks.some((t) => t.id === FINANCE_ID)
  );
}

function syncPolling() {
  const want = financeOnScreen();
  if (want && timer == null) {
    void refreshFinance();
    timer = window.setInterval(() => void refreshFinance(), POLL_MS);
  } else if (!want && timer != null) {
    window.clearInterval(timer);
    timer = null;
  }
}

/** Asks Rust whether the folder changed; `force` reads it whatever the stamp. */
export async function refreshFinance(force = false) {
  if (polling) return;
  polling = true;
  try {
    const poll = await Bridge.financePoll(force ? null : stamp);
    if (poll) applyFinance(poll);
  } finally {
    polling = false;
  }
}

/** A fresh reading of the folder. Its error, if any, keeps what was shown. */
export function applyFinance(poll: FinancePoll) {
  // A failed reading (a file another program holds) is tried again next time.
  stamp = poll.error ? null : poll.stamp;
  const previous = State.integrations[FINANCE_ID];
  const summary = poll.summary ? readSummary(poll.summary) : null;
  State.integrations[FINANCE_ID] = {
    data: summary ? { ...summary, folder: poll.folder } : (previous?.data ?? {}),
    error: poll.error,
    loaded: summary != null || (previous?.loaded ?? false),
    configured: true,
  };
  syncFinancePill();
  State.notify();
}

/** The pill's label: the short text once the folder was read, else its name. */
export function syncFinancePill() {
  const task = State.tasks.find((x) => x.id === FINANCE_ID);
  if (!task) return;
  const text = pillText(readSummary(State.integrations[FINANCE_ID]?.data));
  const name = text ?? pillDefinition(FINANCE_ID)?.name ?? "Finance";
  if (task.name !== name) task.name = name;
}

/**
 * The card's Add button. Null once it is in the file (with a small "done"
 * sound), else the reason it was refused, in the interface language.
 */
export async function addExpense(expense: NewExpense): Promise<string | null> {
  try {
    const poll = await Bridge.financeAddExpense(expense);
    if (poll) applyFinance(poll);
    Sound.play("approve");
    return null;
  } catch (err) {
    const text = err instanceof Error ? err.message : String(err ?? "");
    return text || "…";
  }
}
