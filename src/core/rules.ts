// A remembered answer — "Don't ask again" on a permission card — as Rust
// describes it (src-tauri/src/rules.rs). What a rule covers is decided there
// and only there; here it is put into words for the card, the session's line
// and the list in Settings.

import { t } from "../i18n/i18n";

export type RuleKind = "exact" | "prefix" | "domain" | "tool" | "edits";

export interface Rule {
  id: string;
  /** The relay's tag: empty for Claude Code. */
  agent: string;
  /** The project folder the rule holds in. */
  cwd: string;
  tool: string;
  kind: RuleKind;
  /** The command, its first words, or the host — by kind. */
  text?: string;
  /** Seconds since 1970. */
  created: number;
}

const KINDS: ReadonlySet<string> = new Set(["exact", "prefix", "domain", "tool", "edits"]);

/** A rule as it comes in a hook payload or from the list, or null. */
export function asRule(value: unknown): Rule | null {
  if (!value || typeof value !== "object") return null;
  const v = value as Record<string, unknown>;
  if (typeof v.id !== "string" || typeof v.tool !== "string" || typeof v.kind !== "string") return null;
  if (!KINDS.has(v.kind)) return null;
  return {
    id: v.id,
    agent: typeof v.agent === "string" ? v.agent : "",
    cwd: typeof v.cwd === "string" ? v.cwd : "",
    tool: v.tool,
    kind: v.kind as RuleKind,
    ...(typeof v.text === "string" ? { text: v.text } : {}),
    created: typeof v.created === "number" ? v.created : 0,
  };
}

/**
 * What the rule lets through, in a few words: the command as it is, its first
 * words and "…" for what may follow, the host, the tool, or "file edits".
 */
export function ruleLabel(rule: Pick<Rule, "kind" | "text" | "tool">): string {
  switch (rule.kind) {
    case "exact":
    case "domain":
      return rule.text ?? "";
    case "prefix":
      return `${rule.text ?? ""} …`;
    case "edits":
      return t("file edits");
    default:
      return rule.tool;
  }
}
