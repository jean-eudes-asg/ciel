// The Games page's Clouds puzzle (src/play/clouds.ts): a made puzzle is
// solvable in exactly the moves it says, and they grow with the level.

import { test } from "node:test";
import assert from "node:assert/strict";
import { EXIT_ROW, SIZE, puzzle, room, solve, target } from "../src/play/clouds.ts";

test("a puzzle takes the fewest moves it claims, and nothing overlaps", () => {
  for (const level of [1, 4, 9]) {
    const p = puzzle(level);
    const cells = new Set();
    for (const cl of p.clouds) {
      for (let k = 0; k < cl.len; k++) {
        const cell = cl.v ? (cl.r + k) * SIZE + cl.c : cl.r * SIZE + cl.c + k;
        assert.ok(!cells.has(cell));
        cells.add(cell);
      }
    }
    assert.equal(p.clouds[0].r, EXIT_ROW);
    assert.equal(solve(p.clouds).length - 1, p.moves);
    assert.ok(p.moves >= 2);
  }
  assert.ok(target(10) > target(1));
});

test("a cloud slides only as far as the sky is free", () => {
  const clouds = [{ r: 2, c: 0, len: 2, v: false }, { r: 1, c: 3, len: 3, v: true }];
  assert.deepEqual(room(clouds, [0, 1], 0), [0, 1]);
  assert.deepEqual(room(clouds, [0, 1], 1), [0, 3]);
  assert.equal(solve(clouds).length - 1, 2);
});
