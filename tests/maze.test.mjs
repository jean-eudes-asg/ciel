// The Games page's maze (src/play/maze.ts): every cell can be reached, walls
// agree on both sides, and Ciel's way out is a real one.

import { test } from "node:test";
import assert from "node:assert/strict";
import { generate, path } from "../src/play/maze.ts";

test("a maze joins every cell, and its shortest way goes through open walls", () => {
  for (const [cols, rows] of [[12, 7], [36, 20], [2, 2]]) {
    const m = generate(cols, rows);
    for (let i = 0; i < cols * rows; i++) {
      const x = i % cols;
      if (x < cols - 1) assert.equal(!!(m.walls[i] & 2), !!(m.walls[i + 1] & 8));
      if (i + cols < cols * rows) assert.equal(!!(m.walls[i] & 4), !!(m.walls[i + cols] & 1));
      assert.deepEqual(path(m, 0, i).at(-1), i);
    }
    const way = path(m, 0, cols * rows - 1);
    assert.equal(way[0], 0);
    for (let k = 1; k < way.length; k++) {
      const [a, b] = [way[k - 1], way[k]];
      const d = b - a === 1 ? 2 : b - a === -1 ? 8 : b - a === cols ? 4 : 1;
      assert.equal(m.walls[a] & d, 0);
    }
  }
});
