// What the open island's overview shows (views/views.ts buildOverview): one
// pill fills it in detail, two share it in detail, and from three on the
// focused one is in detail with the others listed beside it.

export type OverviewLayout = "solo" | "duo" | "list";

export interface OverviewPlan<T> {
  layout: OverviewLayout;
  /** In detail on the left, beside Ciel (across the island when solo). */
  left: T | null;
  /** In detail on the right: the second pill of a duo. */
  right: T | null;
  /** The other pills, listed on the right (at most four). */
  list: T[];
}

/**
 * Two pills keep their places — the first (the main pill, State.tasks is
 * ordered so) on the left, the other on the right — whichever has the focus,
 * so they don't swap each time an agent speaks. From three on, the focused
 * one has the detail.
 */
export function planOverview<T extends { id: string }>(tasks: readonly T[], focusId: string | null): OverviewPlan<T> {
  if (tasks.length <= 1) return { layout: "solo", left: tasks[0] ?? null, right: null, list: [] };
  if (tasks.length === 2) return { layout: "duo", left: tasks[0], right: tasks[1], list: [] };
  const focus = tasks.find((t) => t.id === focusId) ?? tasks[0];
  return { layout: "list", left: focus, right: null, list: tasks.filter((t) => t !== focus).slice(0, 4) };
}

/** The files a session edited, newest first, each once (its latest diff). */
export function recentEdits<T extends { path: string }>(diffs: readonly T[], max = 3): T[] {
  const seen = new Set<string>();
  const out: T[] = [];
  for (let i = diffs.length - 1; i >= 0 && out.length < max; i--) {
    if (seen.has(diffs[i].path)) continue;
    seen.add(diffs[i].path);
    out.push(diffs[i]);
  }
  return out;
}

/** The plan whose usage an agent's pill shows beside its feed when it has the room. */
export function pillPlan(id: string): "claude" | "codex" | null {
  if (id === "integration_claude") return "claude";
  if (id === "agent_codex") return "codex";
  return null;
}
