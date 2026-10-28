// The Games page's international draughts (src/play/draughts.ts): the move
// counts every draughts program agrees on, flying kings, the majority rule,
// crowning only at the end of a move.

import { test } from "node:test";
import assert from "node:assert/strict";
import { Draughts, choose, notation } from "../src/play/draughts.ts";

function perft(d, depth) {
  if (depth === 0) return 1;
  let n = 0;
  for (const m of d.moves()) {
    d.make(m);
    n += perft(d, depth - 1);
    d.unmake();
  }
  return n;
}

const empty = (turn = "w") => {
  const d = new Draughts();
  d.board.fill("");
  d.turn = turn;
  return d;
};
const sq = (r, c) => r * 10 + c;

test("the moves from the start, as every draughts program counts them", () => {
  const d = new Draughts();
  assert.deepEqual([1, 2, 3, 4, 5].map((n) => perft(d, n)), [9, 81, 658, 4265, 27117]);
  assert.equal(d.key(), new Draughts().key());
});

test("a king flies, and takes from afar landing anywhere beyond", () => {
  const d = empty();
  d.board[sq(9, 0)] = "W";
  d.board[sq(5, 4)] = "b";
  d.board[sq(0, 1)] = "b";
  const moves = d.moves();
  // Over the man on (5,4): onto (4,5), (3,6), (2,7), (1,8) or (0,9).
  assert.equal(moves.length, 5);
  assert.ok(moves.every((m) => m.taken[0] === sq(5, 4)));
  const quiet = empty();
  quiet.board[sq(9, 0)] = "W";
  quiet.board[sq(0, 9)] = "b";
  assert.equal(quiet.moves().length, 8);
});

test("the most pieces must be taken, men take backwards, and are crowned only where they stop", () => {
  const d = empty();
  d.board[sq(5, 4)] = "w";
  d.board[sq(4, 5)] = "b"; // one, forwards
  d.board[sq(6, 3)] = "b"; // backwards, then on to a second
  d.board[sq(6, 1)] = "b";
  const moves = d.moves();
  assert.equal(moves.length, 1);
  assert.deepEqual(moves[0].path, [sq(5, 4), sq(7, 2), sq(5, 0)]);
  assert.equal(notation({ path: [sq(6, 3), sq(8, 1)], taken: [sq(7, 2)], crowns: false }), "32x41");

  const through = empty();
  through.board[sq(2, 1)] = "w";
  through.board[sq(1, 2)] = "b";
  through.board[sq(1, 4)] = "b";
  const [m] = through.moves();
  // Over the far row and down again: still a man.
  assert.deepEqual(m.path, [sq(2, 1), sq(0, 3), sq(2, 5)]);
  assert.equal(m.crowns, false);
});

test("the rival takes the piece left hanging", () => {
  const d = empty("b");
  d.board[sq(3, 2)] = "b";
  d.board[sq(4, 3)] = "w";
  d.board[sq(9, 0)] = "w";
  d.board[sq(0, 9)] = "b";
  assert.equal(choose(d, { depth: 2, ms: 500, slack: 0 }).move.taken.length, 1);
});
