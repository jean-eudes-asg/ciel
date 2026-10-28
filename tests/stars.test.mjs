// The Games page's Constellations (src/play/stars.ts): the made sky has a
// way that joins every pair and fills every point.

import { test } from "node:test";
import assert from "node:assert/strict";
import { adjacent, progress, sizeFor, sky, wander } from "../src/play/stars.ts";

test("the wandering line goes once through every point, one step at a time", () => {
  for (const size of [5, 7, 9]) {
    const path = wander(size);
    assert.equal(new Set(path).size, size * size);
    for (let k = 1; k < path.length; k++) assert.ok(adjacent(size, path[k - 1], path[k]));
  }
});

test("a sky's own lines join every pair and fill it all", () => {
  for (const level of [1, 5, 12]) {
    const s = sky(level);
    assert.equal(s.size, sizeFor(level));
    assert.ok(s.lines.every((l) => l.length >= 3));
    assert.deepEqual(progress(s, s.lines), { joined: s.pairs.length, filled: 1 });
    assert.equal(progress(s, []).joined, 0);
  }
});
