// The open island's home: one pill in detail across it, two side by side in
// detail, more as detail and list (src/core/overview.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import { pillPlan, planOverview, recentEdits } from "../src/core/overview.ts";

const pill = (id) => ({ id });
const ids = (list) => list.map((t) => t.id);

test("one pill has the island to itself", () => {
  const p = planOverview([pill("integration_claude")], "integration_claude");
  assert.equal(p.layout, "solo");
  assert.equal(p.left.id, "integration_claude");
  assert.equal(p.right, null);
  assert.deepEqual(p.list, []);
  assert.equal(planOverview([], null).left, null, "nothing at all: nothing in detail");
});

test("two pills are both in detail, in their places whichever has the focus", () => {
  const tasks = [pill("integration_claude"), pill("integration_spotify")];
  for (const focus of ["integration_claude", "integration_spotify", null]) {
    const p = planOverview(tasks, focus);
    assert.equal(p.layout, "duo");
    assert.equal(p.left.id, "integration_claude", "the main pill stays beside Ciel");
    assert.equal(p.right.id, "integration_spotify");
    assert.deepEqual(p.list, []);
  }
});

test("from three on: the focused one in detail, the others listed, four at most", () => {
  const tasks = ["a", "b", "c", "d", "e", "f"].map(pill);
  const p = planOverview(tasks, "c");
  assert.equal(p.layout, "list");
  assert.equal(p.left.id, "c");
  assert.equal(p.right, null);
  assert.deepEqual(ids(p.list), ["a", "b", "d", "e"]);
  assert.equal(planOverview(tasks, "gone").left.id, "a", "a focus no longer there: the first pill");
});

test("the files a session edited: newest first, each once, three at most", () => {
  const d = (id, path) => ({ id, path });
  const diffs = [d(1, "a.ts"), d(2, "b.ts"), d(3, "a.ts"), d(4, "c.ts"), d(5, "d.ts")];
  assert.deepEqual(recentEdits(diffs).map((x) => x.id), [5, 4, 3]);
  assert.deepEqual(recentEdits(diffs, 10).map((x) => x.id), [5, 4, 3, 2], "a.ts once, its latest edit");
  assert.deepEqual(recentEdits([]), []);
});

test("the plans an agent's pill can show beside its feed", () => {
  assert.equal(pillPlan("integration_claude"), "claude");
  assert.equal(pillPlan("agent_codex"), "codex");
  assert.equal(pillPlan("agent_cursor"), null);
  assert.equal(pillPlan("integration_spotify"), null);
});
