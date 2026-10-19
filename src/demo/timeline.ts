// The demo's script: about 60 to 90 seconds that loop. Every step is something
// the real island receives or the person does — a hook event, opening the
// island, clicking a pill, Ctrl+E on a diff, answering a card, typing in the
// chat — so the demo runs through the same code as real use.
//
// `after` is the pause before the step, in ms. A permission or a question
// waits for a click (Allow, Deny, an option), and answers itself after
// `autoMs` when nobody clicks, so the loop always carries on.

import type { HookPayload } from "../island/hooks";
import {
  CHAT_QUESTION, CLAUDE_ID, CLAUDE_SESSION, CODEX_CWD, CODEX_FINAL_LINE, CODEX_PROMPT, CODEX_SESSION, DEMO_CWD,
  DEMO_REQUEST_PREFIX, EDIT_BACKOFF, EDIT_CLIENT, EDIT_DOCS, FINAL_LINE, FINANCE_ID, GITHUB_ID, PROMPT,
  QUESTION, READ_CLIENT, READ_DOCS, SEARCH_UPLOADS, TEST_COMMAND, WRITE_TEST,
} from "./data";

/** The moments `?scene=` can freeze on, in the order they come. */
export const SCENES = ["diff", "session", "approval", "question", "finished", "finance", "chat"] as const;
export type SceneName = (typeof SCENES)[number];

export function isScene(name: unknown): name is SceneName {
  return typeof name === "string" && (SCENES as readonly string[]).includes(name);
}

/** Ciel waits this long for a click on a demo card before answering it himself. */
export const AUTO_ANSWER_MS = 8000;

export type DemoAction =
  /** A hook event, as the relay would send it. */
  | { kind: "hook"; payload: HookPayload }
  /** Opens the island on a view, as a click on it would. */
  | { kind: "open"; view: "overview" | "prompt" }
  /** Clicks a pill. */
  | { kind: "focus"; pill: string }
  /** Ctrl+E: opens the newest diff, or closes the one that is open. */
  | { kind: "diff" }
  /** A permission request; waits for Allow or Deny. */
  | { kind: "permission"; payload: HookPayload; autoMs: number }
  /** An AskUserQuestion; waits for an option. `auto` is what Ciel picks if nobody does. */
  | { kind: "question"; payload: HookPayload; autoMs: number; auto: Record<string, string> }
  /** GitHub's sample data: the watched CI run is still going, or it passed. */
  | { kind: "ci"; passed: boolean }
  /** Types a question in the chat and sends it; the answer streams in. */
  | { kind: "chat"; question: string }
  /** Folds the island to its compact size. */
  | { kind: "fold" }
  /** A mood of Ciel's own (sleeping), or none. */
  | { kind: "mood"; state: "sleeping" | null }
  /** A moment `?scene=` can freeze on: everything before it has run. */
  | { kind: "scene"; name: SceneName }
  /** The loop is over: it starts again (or, with `?loop=0`, stays here). */
  | { kind: "end" };

export interface DemoStep {
  /** Pause before this step, ms. */
  after: number;
  action: DemoAction;
  /** Only when the last permission was allowed. */
  ifAllowed?: true;
}

type Extra = Partial<HookPayload>;

const claude = (hook_event_name: string, extra: Extra = {}): HookPayload => ({
  hook_event_name, session_id: CLAUDE_SESSION, cwd: DEMO_CWD, ...extra,
});

const codex = (hook_event_name: string, extra: Extra = {}): HookPayload => ({
  hook_event_name, session_id: CODEX_SESSION, cwd: CODEX_CWD, ciel_agent: "codex", ...extra,
});

const tool = (tool_name: string, tool_input: Record<string, unknown>): Extra => ({ tool_name, tool_input });

const hook = (payload: HookPayload): DemoAction => ({ kind: "hook", payload });
const scene = (name: SceneName): DemoStep => ({ after: 0, action: { kind: "scene", name } });

export const DEMO_TIMELINE: readonly DemoStep[] = [
  // 1. A Claude Code session in ciel-demo: reads, searches, edits with live diffs.
  { after: 900, action: hook(claude("SessionStart")) },
  { after: 500, action: { kind: "open", view: "overview" } },
  { after: 600, action: hook(claude("UserPromptSubmit", { prompt: PROMPT })) },
  { after: 2000, action: hook(claude("PreToolUse", tool("Read", READ_CLIENT))) },
  { after: 1200, action: hook(claude("PostToolUse", tool("Read", READ_CLIENT))) },
  { after: 300, action: hook(claude("PreToolUse", tool("Grep", SEARCH_UPLOADS))) },
  { after: 1200, action: hook(claude("PostToolUse", tool("Grep", SEARCH_UPLOADS))) },
  { after: 300, action: hook(claude("PreToolUse", tool("Edit", EDIT_CLIENT))) },
  { after: 900, action: hook(claude("PostToolUse", tool("Edit", EDIT_CLIENT))) },
  { after: 700, action: { kind: "diff" } },
  scene("diff"),
  { after: 4200, action: { kind: "diff" } },
  { after: 400, action: hook(claude("PreToolUse", tool("Write", WRITE_TEST))) },
  { after: 900, action: hook(claude("PostToolUse", tool("Write", WRITE_TEST))) },
  scene("session"),

  // 2. It asks to run the tests: Allow or Deny.
  {
    after: 1400,
    action: {
      kind: "permission",
      payload: claude("PermissionRequest", { request_id: `${DEMO_REQUEST_PREFIX}npm-test`, ...tool("Bash", TEST_COMMAND) }),
      autoMs: AUTO_ANSWER_MS,
    },
  },
  scene("approval"),
  { after: 300, ifAllowed: true, action: hook(claude("PreToolUse", tool("Bash", TEST_COMMAND))) },
  { after: 2600, ifAllowed: true, action: hook(claude("PostToolUse", tool("Bash", TEST_COMMAND))) },

  // 3. A question with three options.
  {
    after: 900,
    action: {
      kind: "question",
      payload: claude("PermissionRequest", {
        request_id: `${DEMO_REQUEST_PREFIX}retry-question`,
        ...tool("AskUserQuestion", { questions: [QUESTION] }),
      }),
      autoMs: AUTO_ANSWER_MS,
      auto: { [QUESTION.question]: QUESTION.options[0].label },
    },
  },
  scene("question"),
  { after: 500, action: hook(claude("PostToolUse", tool("AskUserQuestion", { questions: [QUESTION] }))) },

  // 4. Codex starts on the API docs, alongside.
  { after: 400, action: hook(codex("SessionStart")) },
  { after: 500, action: hook(codex("UserPromptSubmit", { prompt: CODEX_PROMPT })) },
  { after: 500, action: hook(claude("PreToolUse", tool("Edit", EDIT_BACKOFF))) },
  { after: 900, action: hook(claude("PostToolUse", tool("Edit", EDIT_BACKOFF))) },
  { after: 300, action: hook(codex("PreToolUse", tool("Read", READ_DOCS))) },
  { after: 1200, action: hook(codex("PreToolUse", tool("Edit", EDIT_DOCS))) },
  { after: 800, action: hook(codex("PostToolUse", tool("Edit", EDIT_DOCS))) },
  { after: 600, action: hook(claude("PreToolUse", tool("Bash", TEST_COMMAND))) },
  { after: 2000, action: hook(claude("PostToolUse", tool("Bash", TEST_COMMAND))) },

  // 5. Claude Code finishes: the card, and Ciel's happy jump. Codex a little later.
  { after: 500, action: hook(claude("Stop", { last_assistant_message: FINAL_LINE })) },
  scene("finished"),
  { after: 3800, action: hook(codex("Stop", { last_assistant_message: CODEX_FINAL_LINE })) },

  // 6. The integration pills: GitHub's CI run turns green, then the month's spending.
  { after: 1800, action: { kind: "focus", pill: GITHUB_ID } },
  { after: 2600, action: { kind: "ci", passed: true } },
  { after: 2600, action: { kind: "focus", pill: FINANCE_ID } },
  scene("finance"),

  // 7. Back on Claude Code (Ciel in white again), a question in the chat, its answer streamed.
  { after: 5200, action: { kind: "focus", pill: CLAUDE_ID } },
  { after: 500, action: { kind: "open", view: "prompt" } },
  { after: 500, action: { kind: "chat", question: CHAT_QUESTION } },
  scene("chat"),

  // 8. Folded away, Ciel falls asleep; then it all starts again.
  { after: 9000, action: { kind: "fold" } },
  { after: 900, action: { kind: "mood", state: "sleeping" } },
  { after: 6500, action: { kind: "end" } },
];

/** How long one loop takes when nobody clicks: every pause, every card answered by Ciel. */
export function loopDuration(steps: readonly DemoStep[] = DEMO_TIMELINE): number {
  let ms = 0;
  for (const step of steps) {
    ms += step.after;
    if (step.action.kind === "permission" || step.action.kind === "question") ms += step.action.autoMs;
  }
  return ms;
}
