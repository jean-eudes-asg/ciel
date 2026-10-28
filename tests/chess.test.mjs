// The Games page's chess (src/play/chess.ts): the rules against the standard
// perft counts, the end of a game, SAN, and Ciel's player.

import { test } from "node:test";
import assert from "node:assert/strict";
import { Chess, choose, sq } from "../src/play/chess.ts";

const MEDIUM = { depth: 3, ms: 900, slack: 35 };
const EASY = { depth: 1, ms: 300, slack: 220 };

function perft(c, depth) {
  if (depth === 0) return 1;
  let n = 0;
  for (const m of c.moves()) {
    c.make(m);
    n += perft(c, depth - 1);
    c.unmake();
  }
  return n;
}

test("every legal move, counted as the chess programming wiki does", () => {
  assert.equal(perft(new Chess(), 3), 8902);
  // "Kiwipete": castling, en passant, promotions and pins all at once.
  assert.equal(perft(new Chess("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"), 3), 97862);
  assert.equal(perft(new Chess("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"), 4), 43238);
  assert.equal(perft(new Chess("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1"), 3), 9467);
});

test("make and unmake leave the position as it was", () => {
  const fen = "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
  const c = new Chess(fen);
  perft(c, 2);
  assert.equal(c.fen(), fen);
});

test("mate, stalemate and repetition end the game", () => {
  const fools = new Chess();
  for (const s of ["f3", "e5", "g4", "Qh4#"]) fools.play(fools.moves().find((m) => fools.san(m) === s));
  assert.deepEqual(fools.result(), { winner: "b", why: "mate" });
  assert.deepEqual(fools.line(), ["f3", "e5", "g4", "Qh4#"]);

  assert.deepEqual(new Chess("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").result(), { winner: null, why: "stalemate" });
  assert.deepEqual(new Chess("8/8/4k3/8/8/3NK3/8/8 w - - 0 1").result(), { winner: null, why: "material" });

  const back = new Chess();
  for (let i = 0; i < 2; i++) {
    for (const s of ["Nf3", "Nf6", "Ng1", "Ng8"]) back.play(back.moves().find((m) => back.san(m) === s));
  }
  assert.equal(back.result()?.why, "repetition");
});

test("SAN tells two knights apart and shows castling", () => {
  const c = new Chess("4k3/8/8/8/8/8/8/R3K1NR w KQ - 0 1");
  const sans = c.moves().map((m) => c.san(m));
  assert.ok(sans.includes("O-O-O"));
  assert.ok(sans.includes("Rb1"));
  const two = new Chess("4k3/8/8/8/8/2N3N1/8/4K3 w - - 0 1");
  assert.ok(two.moves().map((m) => two.san(m)).includes("Nce4"));
});

test("Ciel takes a free queen, and even his dreamiest cloud finds a mate in one", () => {
  const free = new Chess("4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1");
  assert.equal(free.san(choose(free, MEDIUM).move), "exd5");
  const mate = new Chess("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1");
  assert.equal(mate.san(choose(mate, EASY).move), "Ra8#");
  assert.equal(new Chess("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").moves().length, 0);
  assert.equal(sq("e1"), 95);
});
