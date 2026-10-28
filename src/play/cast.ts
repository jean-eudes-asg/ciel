// Who you play against: five clouds of Ciel's family, from the dreamiest to
// the storm. Each has his colour, his way of playing chess, his pace in the
// maze, his sky (the real one Ciel draws, src/sky) and how he talks. The
// board and the maze take their colours from that sky.

import { t } from "../i18n/i18n";
import { lightFor, mix, hex, luma, css, type RGB, type Light } from "../sky/palette";
import type { SceneSpec } from "../sky/scenes";
import type { Theme } from "../sky/prefs";
import type { Level } from "./chess";

export type RivalId = "cirrus" | "cumulus" | "ciel" | "stratus" | "nimbus";

export interface Rival {
  id: RivalId;
  /** His name, the same in every language. */
  name: string;
  color: string;
  tagline: () => string;
  chess: Level;
  draughts: Level;
  /** How long he takes over a move, in seconds, when the move needs thinking. */
  think: [number, number];
  /** His pace in the maze, in cells a second. */
  maze: number;
  /** What hangs around him when he is idle: z's, hearts, stars or sparks. */
  quirk: "z" | "heart" | "star" | "spark";
  /** His voice: Ciel's sounds a little higher or lower. */
  pitch: number;
  /** For the brain: who he is and how he talks. */
  persona: string;
  sky: { theme: Theme; sun: number; moon?: boolean };
}

export const RIVALS: Rival[] = [
  {
    id: "cirrus",
    name: "Cirrus",
    color: "#EEF3FB",
    tagline: () => t("Dreamy, often looking elsewhere."),
    chess: { depth: 1, ms: 300, slack: 220 },
    draughts: { depth: 2, ms: 300, slack: 150 },
    think: [2, 4.5],
    maze: 2,
    quirk: "z",
    pitch: 1.25,
    persona: "You are Cirrus, the dreamiest cloud of the family: soft-spoken, poetic, easily distracted by the sky, delighted by everything, and you never mind losing.",
    sky: { theme: "clear", sun: 34 },
  },
  {
    id: "cumulus",
    name: "Cumulus",
    color: "#FFFFFF",
    tagline: () => t("Cheerful, and loves to attack."),
    chess: { depth: 2, ms: 500, slack: 90 },
    draughts: { depth: 4, ms: 500, slack: 60 },
    think: [1, 2.6],
    maze: 2.7,
    quirk: "heart",
    pitch: 1.12,
    persona: "You are Cumulus, a round cheerful cloud: bubbly, enthusiastic, loves to attack, cheers for good moves even when they are against you.",
    sky: { theme: "golden", sun: 7 },
  },
  {
    id: "ciel",
    name: "Ciel",
    color: "#F5F6F8",
    tagline: () => t("Plays for fun, and plays well."),
    chess: { depth: 3, ms: 900, slack: 35 },
    draughts: { depth: 6, ms: 900, slack: 25 },
    think: [1.6, 3.6],
    maze: 3.3,
    quirk: "star",
    pitch: 1,
    persona: "You are Ciel himself, the little cloud who lives at the top of the user's screen: playful, warm, a little cheeky, and you know them well because you work beside them all day.",
    sky: { theme: "sunset", sun: 0.5 },
  },
  {
    id: "stratus",
    name: "Stratus",
    color: "#C3CBD6",
    tagline: () => t("Calm and patient, plays at night."),
    chess: { depth: 4, ms: 1600, slack: 12 },
    draughts: { depth: 8, ms: 1600, slack: 8 },
    think: [3, 6.5],
    maze: 4,
    quirk: "star",
    pitch: 0.86,
    persona: "You are Stratus, a calm grey cloud who plays at night: few words, patient, precise, quietly kind, a dry sense of humour.",
    sky: { theme: "night", sun: -24, moon: true },
  },
  {
    id: "nimbus",
    name: "Nimbus",
    color: "#8E98A8",
    tagline: () => t("The storm. Rarely forgives."),
    chess: { depth: 7, ms: 3500, slack: 0 },
    draughts: { depth: 14, ms: 3000, slack: 0 },
    think: [2.4, 6],
    maze: 5,
    quirk: "spark",
    pitch: 0.72,
    persona: "You are Nimbus, the storm cloud: dramatic, theatrical, proud of your thunder, yet a fair player who respects a good fight.",
    sky: { theme: "storm", sun: 20 },
  },
];

export const rival = (id: string): Rival => RIVALS.find((r) => r.id === id) ?? RIVALS[2];

/** The maze's rival for a level: two levels each, then the storm. */
export const mazeRival = (level: number): Rival => RIVALS[Math.min(RIVALS.length - 1, Math.floor((level - 1) / 2))];

/** His sky, as the sky renderer draws it. */
export function skySpec(r: Rival): SceneSpec {
  return {
    theme: r.sky.theme,
    phase: r.sky.theme === "night" ? "night" : r.sky.theme === "sunset" ? "sunset" : r.sky.theme === "golden" ? "golden" : "day",
    mood: "idle",
    sun: { elevation: r.sky.sun, azimuth: 205 },
    moon: r.sky.moon ? { elevation: 38, azimuth: 150, phase: 0.42 } : null,
    facing: "south",
    afterglow: false,
    seed: 7 + RIVALS.indexOf(r) * 101,
    ciel: null,
    lockScreen: false,
  };
}

export function lightOf(r: Rival): Light {
  return lightFor(r.sky.theme, r.sky.sun, false);
}

/** `c` mixed towards `to` until it is at least (or at most) `target` bright. */
function towards(c: RGB, to: RGB, target: number, above: boolean): RGB {
  let out = c;
  for (let k = 0.1; k <= 1 && (above ? luma(out) < target : luma(out) > target); k += 0.1) out = mix(c, to, k);
  return out;
}

/** The board in his sky's colours: its lit clouds for the light squares, its shade for the dark ones. */
export function boardColors(r: Rival): { light: string; dark: string; mark: string; edge: string } {
  const l = lightOf(r);
  // His sky, quietened towards paper and slate: a board to look at for an hour.
  const light = towards(mix(mix(l.lit, l.cmid, 0.35), hex("#eee8dc"), 0.45), hex("#f4f1ea"), 196, true);
  const dark = towards(towards(mix(mix(l.mid, l.shade, 0.45), hex("#7a808b"), 0.4), hex("#8a909a"), 112, true), hex("#3c4452"), 150, false);
  const mark = l.dark > 0.5 ? hex("#c9d6ff") : l.sunColor;
  return { light: css(light), dark: css(dark), mark: css(mark, 0.5), edge: css(l.zenith) };
}
