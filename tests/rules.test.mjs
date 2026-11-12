// A remembered answer, put into words (src/core/rules.ts). What a rule covers
// is decided in Rust (src-tauri/src/rules.rs) and tested there.

import { test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";

installFakeDom();

const { asRule, ruleLabel } = await import("../src/core/rules.ts");
const { setLanguage } = await import("../src/i18n/i18n.ts");

test("a rule reads as what it lets through", () => {
  assert.equal(ruleLabel({ kind: "exact", text: "cargo build", tool: "Bash" }), "cargo build");
  assert.equal(ruleLabel({ kind: "prefix", text: "npm test", tool: "Bash" }), "npm test …");
  assert.equal(ruleLabel({ kind: "domain", text: "docs.rs", tool: "WebFetch" }), "docs.rs");
  assert.equal(ruleLabel({ kind: "tool", tool: "mcp__github__list_issues" }), "mcp__github__list_issues");
  assert.equal(ruleLabel({ kind: "edits", tool: "Edit" }), "file edits");
  setLanguage("fr");
  try {
    assert.equal(ruleLabel({ kind: "edits", tool: "Edit" }), "modifications de fichiers");
  } finally {
    setLanguage("en");
  }
});

test("only what Rust sends as a rule is taken as one", () => {
  const rule = { id: "k1", agent: "codex", cwd: "/home/me/app", tool: "Bash", kind: "exact", text: "ls", created: 5 };
  assert.deepEqual(asRule(rule), rule);
  // A whole-tool rule has no text; a missing agent is Claude Code.
  assert.deepEqual(asRule({ id: "k2", tool: "WebSearch", kind: "tool" }),
    { id: "k2", agent: "", cwd: "", tool: "WebSearch", kind: "tool", created: 0 });
  for (const not of [null, undefined, "rule", 3, [], {}, { id: "k", tool: "Bash" }, { id: "k", tool: "Bash", kind: "all" },
    { id: 1, tool: "Bash", kind: "exact" }, { id: "k", kind: "exact" }]) {
    assert.equal(asRule(not), null);
  }
});
