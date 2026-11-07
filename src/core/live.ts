// What the folded island shows in place of its clock while something is going
// on (island/live.ts): the one that matters most. Pure.
//
// The microphones muted comes first (people talk into a muted call), then a
// timer that rang, the soonest timer or Focus round, rain about to start,
// the computer kept awake, and last the line of the song being sung.

import type { Controls, TimerItem, Weather } from "./tools";
import { msLeft } from "./timers";

export type Live =
  | { kind: "mic" }
  | { kind: "rang"; timer: TimerItem }
  | { kind: "timer"; timer: TimerItem; left: number }
  | { kind: "rain"; minutes: number }
  | { kind: "awake" }
  | { kind: "lyric"; text: string };

export interface LiveInputs {
  timers: readonly TimerItem[];
  controls: Controls | null;
  weather: Weather | null;
  now: number;
  /** The line of the song being sung, when lyrics are on (tools/lyrics.ts). */
  lyric?: string | null;
}

/** A timer shows in the folded island from this close to ringing (and always while paused). */
export const TIMER_WINDOW_MS = 60 * 60_000;

export function pickLive({ timers, controls, weather, now, lyric }: LiveInputs): Live | null {
  if (controls?.micMuted) return { kind: "mic" };
  const rang = timers.find((t) => t.ringing && t.kind !== "focus");
  if (rang) return { kind: "rang", timer: rang };
  const running = timers
    .filter((t) => !t.ringing && t.kind !== "reminder")
    .map((t) => ({ t, left: msLeft(t, now) }))
    .filter(({ t, left }) => t.left != null || left <= TIMER_WINDOW_MS)
    .sort((a, b) => a.left - b.left)[0];
  if (running) return { kind: "timer", timer: running.t, left: running.left };
  if (weather && !weather.raining && weather.rainAt != null) {
    const minutes = Math.round((weather.rainAt - now) / 60_000);
    if (minutes >= 0 && minutes <= 45) return { kind: "rain", minutes };
  }
  if (controls?.awake) return { kind: "awake" };
  if (lyric) return { kind: "lyric", text: lyric };
  return null;
}

/**
 * It keeps the folded island on screen (it would otherwise slide away). The
 * microphones off don't: they may stay off for hours; the key that turns them
 * off shows the island for a moment instead.
 */
export function holdsIsland(live: Live | null): boolean {
  return live != null && (live.kind === "rang" || live.kind === "timer");
}

/** It changes every second (a countdown): the island redraws it that often, and only then. */
export function ticks(live: Live | null): boolean {
  return (live?.kind === "timer" && live.timer.left == null) || live?.kind === "lyric";
}
