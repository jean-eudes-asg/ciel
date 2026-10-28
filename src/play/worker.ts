// The rivals think here, and the cloud puzzles are made here, so the page
// and their faces keep moving meanwhile.

import { Chess, choose as chessMove, rank as chessRank, type Level } from "./chess";
import { Draughts, choose as draughtsMove } from "./draughts";
import { puzzle } from "./clouds";

/** What a chess hint looks with: the best move, never one of the near ones a rival plays for variety. */
const HINT: Level = { depth: 5, ms: 1500, slack: 0 };

export type Ask =
  | { game: "chess"; id: number; fen: string; level: Level; hint?: boolean }
  | { game: "draughts"; id: number; moves: number[][]; level: Level }
  | { game: "clouds"; id: number; level: number };

self.onmessage = (e: MessageEvent<Ask>) => {
  const ask = e.data;
  if (ask.game === "chess") {
    const c = new Chess(ask.fen);
    const choice = ask.hint ? chessRank(c, HINT)[0] : chessMove(c, ask.level);
    self.postMessage({ id: ask.id, from: choice?.move.from, to: choice?.move.to, promo: choice?.move.promo ?? "", score: choice?.score ?? 0, hint: ask.hint });
  } else if (ask.game === "draughts") {
    const d = new Draughts();
    for (const path of ask.moves) {
      const m = d.moves().find((m) => m.path.join() === path.join());
      if (m) d.play(m);
    }
    const choice = draughtsMove(d, ask.level);
    self.postMessage({ id: ask.id, path: choice?.move.path ?? null, score: choice?.score ?? 0 });
  } else {
    self.postMessage({ id: ask.id, puzzle: puzzle(ask.level) });
  }
};
