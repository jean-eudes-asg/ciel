// Demo mode (src/demo): the timeline is well-formed and loops; the real island
// state goes aside and comes back exactly; nothing the demo does reaches a
// command that writes; and `?scene=` freezes on its moment. Run on the real
// hooks pipeline, State, chat view and bridge, with a stand-in for the Island
// class (it needs a real DOM and canvases).

import { afterEach, beforeEach, mock, test } from "node:test";
import assert from "node:assert/strict";
import { installFakeDom } from "./fakedom.mjs";
import { calls, emit, sent } from "./tauri.mjs";

installFakeDom();
// The demo types in the chat field and presses Enter, as a person would.
globalThis.KeyboardEvent = class {
  constructor(type, init = {}) {
    this.type = type;
    Object.assign(this, init);
  }
  preventDefault() {}
  stopPropagation() {}
};

const { State, DEFAULT_SETTINGS } = await import("../src/core/state.ts");
const { Bridge, onEvent, setBridgeGate } = await import("../src/core/bridge.ts");
const { IslandStateMachine } = await import("../src/island/fsm.ts");
const { registerHookHandlers, settleHookTimers } = await import("../src/island/hooks.ts");
const { buildPrompt } = await import("../src/views/chat.ts");
const { FINANCE_ID } = await import("../src/core/finance.ts");
const { startDemo } = await import("../src/demo/session.ts");
const { DemoPlayer, PLAY_FROM_MS } = await import("../src/demo/player.ts");
const { AUTO_ANSWER_MS, DEMO_TIMELINE, SCENES, loopDuration } = await import("../src/demo/timeline.ts");
const { WINDOW_COMMANDS } = await import("../src/demo/gate.ts");
const { readParams } = await import("../src/demo/params.ts");
const data = await import("../src/demo/data.ts");

const CLAUDE = "integration_claude";

// ── A stand-in for the Island class, doing what island.ts does ────────────────

const MODES = { hidden: "hidden", petit: "compact", home: "expanded", ciel: "expanded" };
/** Ctrl+E presses and the like, in order. */
let viewCommands = [];

function newFsm() {
  const fsm = new IslandStateMachine();
  fsm.onTransition = (_from, to) => {
    State.mode = MODES[to];
    if (to === "home") State.view = State.defaultView();
  };
  return fsm;
}

const island = {
  fsm: newFsm(),
  alert(view) {
    this.fsm.pinned = State.isPinned;
    this.fsm.forceHome();
    State.view = view;
  },
  setView(view) {
    if (State.mode !== "expanded") this.fsm.forceHome();
    State.view = view;
  },
  reveal() {
    this.fsm.reveal();
  },
  dropPin() {
    this.fsm.pinned = false;
    this.fsm.mouseLeft();
  },
  collapse() {
    if (State.pendingApproval) {
      if (State.mode !== "expanded") return;
      State.isPinned = true;
      this.fsm.pinned = true;
      this.fsm.forcePetit();
      return;
    }
    State.isPinned = false;
    this.fsm.pinned = false;
    this.fsm.forcePetit();
  },
  viewCommand(command) {
    viewCommands.push(command);
  },
  applySettings() {
    this.fsm.homeToPetitDelay = State.settings.autoCloseInterval;
    this.fsm.openOnHover = State.settings.openOnHover;
  },
  setPinned(on) {
    State.isPinned = on;
    this.fsm.pinned = on;
  },
  takeKeyboard() {},
  viewActions: {
    setFocus(id) {
      State.setFocus(id);
      const req = State.pendingApproval;
      if (req?.pillId === id) island.setView(req.questions ? "question" : "approval");
    },
    decide(d) {
      const req = State.pendingApproval;
      if (!req) return;
      void Bridge.approvalDecision(req.requestId, d);
      closeApproval();
    },
    answer(answers) {
      const req = State.pendingApproval;
      if (!req) return;
      void Bridge.approvalAnswer(req.requestId, answers);
      closeApproval();
    },
    answerInTerminal() {
      const req = State.pendingApproval;
      if (!req) return;
      void Bridge.approvalDecline(req.requestId);
      closeApproval();
    },
  },
};

function closeApproval() {
  State.endApproval();
  island.fsm.pinned = false;
  island.setView(State.defaultView());
}

registerHookHandlers(island);

// The chat view, whose field the demo types in.
const chat = buildPrompt(() => {});
const field = chat.el.querySelector(".chat-input");
field.dispatchEvent = (event) => {
  for (const fn of field.listeners.get(event.type) ?? []) fn(event);
  return true;
};
document.querySelector = (selector) => (selector === "#views .chat-input" ? field : null);

// ── Helpers ───────────────────────────────────────────────────────────────────

const hook = (payload) => emit("hook", payload);
const task = (id = CLAUDE) => State.tasks.find((t) => t.id === id);
const microtasks = async () => {
  for (let i = 0; i < 20; i++) await Promise.resolve();
};

/** Lets `ms` of demo time go by, microtasks running along (the chat's await). */
async function play(ms, step = 100) {
  for (let t = 0; t < ms; t += step) {
    mock.timers.tick(step);
    await microtasks();
  }
}

/** Lets demo time go by until `done()` holds; fails after `max` ms. */
async function playUntil(done, max = 30_000, what = "") {
  for (let t = 0; t <= max; t += 100) {
    if (done()) return t;
    mock.timers.tick(100);
    await microtasks();
  }
  assert.fail(`not reached within ${max} ms: ${what || done}`);
}

/**
 * Stops a demo and waits until the island is back. As in a browser, whatever
 * the stop set off in microtasks runs before its timeout fires.
 */
async function stop(session) {
  const done = session.stop();
  await microtasks();
  mock.timers.tick(1);
  await done;
  await microtasks();
}

/** Everything the demo must give back, as plain data. */
function picture() {
  return structuredClone({
    mode: State.mode,
    view: State.view,
    tasks: State.tasks,
    focusId: State.focusId,
    stateOverride: State.stateOverride,
    isPinned: State.isPinned,
    paused: State.paused,
    chatHistory: State.chatHistory,
    pendingApproval: State.pendingApproval,
    focusBeforeApproval: State.focusBeforeApproval,
    integrations: State.integrations,
    planUsage: State.planUsage,
    settings: State.settings,
    noteMessage: State.noteMessage,
    droppedFile: State.droppedFile,
    diffs: [...State.sessionDiffs],
    fsm: island.fsm.state,
    fsmPinned: island.fsm.pinned,
    petitToHiddenDelay: island.fsm.petitToHiddenDelay,
    homeToPetitDelay: island.fsm.homeToPetitDelay,
  });
}

/** A busy real island: a session with a diff, a chat, the person's own settings. */
function realIsland() {
  State.settings = {
    ...DEFAULT_SETTINGS,
    activeIntegrations: ["integration_vercel", "integration_stripe"],
    pillColors: { integration_vercel: "#123456" },
    soundVolume: 0.07,
    autoCloseInterval: 12,
    language: "fr",
  };
  island.applySettings();
  State.loadIntegrationTasks();
  State.integrations = {
    integration_vercel: { data: { deployments: [{ projectName: "my-site", state: "READY" }] }, error: null, loaded: true, configured: true },
  };
  State.chatHistory = [
    { id: 9001, role: "user", content: "real question" },
    { id: 9002, role: "assistant", content: "real answer" },
  ];
  State.planUsage = { fiveHour: { percent: 12, resetsAt: 1 }, weekly: null };
  hook({ hook_event_name: "SessionStart", cwd: "/work/real-project", session_id: "real" });
  hook({ hook_event_name: "PreToolUse", cwd: "/work/real-project", session_id: "real", tool_name: "Edit", tool_input: { file_path: "/work/real-project/a.ts" } });
  hook({
    hook_event_name: "PostToolUse", cwd: "/work/real-project", session_id: "real", tool_name: "Edit",
    tool_input: { file_path: "/work/real-project/a.ts", old_string: "a", new_string: "b" },
  });
}

beforeEach(() => {
  mock.timers.enable({ apis: ["setTimeout", "setInterval"] });
  calls.length = 0;
  viewCommands = [];
  island.fsm = newFsm();
  for (const id of [...State.sessionDiffs.keys()]) State.clearSessionDiffs(id);
  State.tasks = [];
  State.focusId = null;
  State.mode = "hidden";
  State.view = "overview";
  State.paused = false;
  State.isPinned = false;
  State.pendingApproval = null;
  State.focusBeforeApproval = null;
  State.stateOverride = null;
  State.chatHistory = [];
  State.integrations = {};
  State.planUsage = null;
  State.noteMessage = null;
  State.settings = { ...DEFAULT_SETTINGS };
  island.applySettings();
  State.loadIntegrationTasks();
});

afterEach(() => {
  setBridgeGate(null);
  settleHookTimers("drop");
  for (const id of [...State.sessionDiffs.keys()]) State.clearSessionDiffs(id);
  mock.timers.reset();
});

// ── The timeline ──────────────────────────────────────────────────────────────

const KINDS = new Set(["hook", "open", "focus", "diff", "permission", "question", "ci", "chat", "fold", "mood", "scene", "end"]);
const actions = DEMO_TIMELINE.map((s) => s.action);
const hooks = actions.filter((a) => a.payload).map((a) => a.payload);

test("the timeline is well formed: known steps, every scene once and in order, one end", () => {
  for (const step of DEMO_TIMELINE) {
    assert.ok(Number.isInteger(step.after) && step.after >= 0, `pause ${step.after}`);
    assert.ok(KINDS.has(step.action.kind), step.action.kind);
  }
  const scenes = actions.filter((a) => a.kind === "scene").map((a) => a.name);
  assert.deepEqual(scenes, [...SCENES]);
  assert.equal(actions.filter((a) => a.kind === "end").length, 1);
  assert.equal(actions.at(-1).kind, "end");
  for (const p of hooks) {
    assert.ok(p.hook_event_name, JSON.stringify(p));
    assert.ok([data.CLAUDE_SESSION, data.CODEX_SESSION].includes(p.session_id));
    if (p.request_id) assert.ok(data.isDemoRequest(p.request_id), p.request_id);
  }
});

test("the timeline tells the whole story, in order, in 60 to 90 seconds", () => {
  const story = [];
  const say = (s) => story.at(-1) !== s && story.push(s);
  for (const a of actions) {
    const p = a.payload;
    if (a.kind === "hook" && p.ciel_agent === "codex") say("codex");
    else if (a.kind === "hook" && p.tool_name === "Edit" && p.hook_event_name === "PostToolUse") say("diff");
    else if (a.kind === "permission") say(`permission:${p.tool_name}:${p.tool_input.command}`);
    else if (a.kind === "question") say(`question:${p.tool_input.questions[0].options.length}`);
    else if (a.kind === "hook" && p.hook_event_name === "Stop" && !p.ciel_agent) say("finished");
    else if (a.kind === "focus") say(`focus:${a.pill}`);
    else if (a.kind === "ci") say("ci");
    else if (a.kind === "chat") say("chat");
    else if (a.kind === "mood") say(`mood:${a.state}`);
  }
  assert.deepEqual(story, [
    "diff", "permission:Bash:npm test", "question:3", "codex", "diff", "codex", "finished", "codex",
    `focus:${data.GITHUB_ID}`, "ci", `focus:${FINANCE_ID}`, `focus:${CLAUDE}`, "chat", "mood:sleeping",
  ]);
  const ms = loopDuration();
  assert.ok(ms >= 60_000 && ms <= 90_000, `${ms} ms`);
});

test("everything shown is made up: no emails, no home folders, no drive letters", () => {
  const text = JSON.stringify([DEMO_TIMELINE, data.githubData(false, new Date()), new data.DemoFinance().summary()]);
  assert.doesNotMatch(text, /@[a-z0-9-]+\.[a-z]/i);
  assert.doesNotMatch(text, /[A-Z]:\\\\|\/home\/|\/Users\//);
  assert.match(text, /ciel-demo/);
});

test("the sample month: 1,240 of 2,000 USD, Groceries, Transport and Eating out on top", () => {
  const s = new data.DemoFinance().summary();
  assert.equal(s.spentMonth, 1240);
  assert.equal(s.budget, 2000);
  assert.equal(Math.round(s.percentUsed), 62);
  assert.deepEqual(s.topCategories.slice(0, 3).map((c) => c.category), ["Groceries", "Transport", "Eating out"]);
  const github = data.githubData(false, new Date()).pulse;
  assert.equal(github.myPRs.length, 2);
  assert.equal(github.mainCI[0].ci, "pending");
  assert.equal(data.githubData(true, new Date()).pulse.mainCI[0].ci, "success");
});

// ── The player, on a recording host ───────────────────────────────────────────

/**
 * Moves the mock clock 1 ms at a time: a timer set from inside another one
 * counts from the moment it was set, as in a browser (one big tick would
 * count it from the end of the tick).
 */
function advance(ms) {
  for (let i = 0; i < ms; i++) mock.timers.tick(1);
}

/** The pauses of the steps from `from` to `to`, both included. */
const pauses = (from, to) => DEMO_TIMELINE.slice(from, to + 1).reduce((ms, s) => ms + s.after, 0);
const PERMISSION = DEMO_TIMELINE.findIndex((s) => s.action.kind === "permission");

function recordingHost() {
  const log = [];
  const host = {
    log,
    reset: () => log.push("reset"),
    hook: (p) => log.push(`hook:${p.hook_event_name}${p.ciel_agent ? ":codex" : ""}`),
    open: (v) => log.push(`open:${v}`),
    focus: (id) => log.push(`focus:${id}`),
    toggleDiff: () => log.push("diff"),
    decide: (d) => log.push(`decide:${d}`),
    answer: () => log.push("answer"),
    ci: (passed) => log.push(`ci:${passed}`),
    chat: (q, instant) => log.push(`chat:${instant}`),
    fold: () => log.push("fold"),
    mood: (s) => log.push(`mood:${s}`),
    hold: () => log.push("hold"),
    setFast: (on) => log.push(`fast:${on}`),
  };
  return host;
}

test("the player runs the steps on their pauses, and a card answers itself after a while", () => {
  const host = recordingHost();
  const player = new DemoPlayer(host, { loop: true });
  player.start();
  assert.deepEqual(host.log, ["reset"]);
  advance(DEMO_TIMELINE[0].after - 1);
  assert.deepEqual(host.log, ["reset"]);
  advance(1);
  assert.deepEqual(host.log, ["reset", "hook:SessionStart"]);
  // Up to the permission card: it waits.
  advance(pauses(1, PERMISSION));
  assert.equal(host.log.at(-1), "hook:PermissionRequest");
  assert.ok(player.isWaiting);
  advance(AUTO_ANSWER_MS - 1);
  assert.equal(host.log.at(-1), "hook:PermissionRequest");
  advance(1);
  assert.equal(host.log.at(-1), "decide:allow");
  assert.ok(!player.isWaiting);
  // Allowed: the tests run next.
  advance(300);
  assert.equal(host.log.at(-1), "hook:PreToolUse");
  player.stop();
});

test("a click answers at once, and Deny skips what only an allowed command does", () => {
  const host = recordingHost();
  const player = new DemoPlayer(host, { loop: true });
  player.start();
  advance(pauses(0, PERMISSION));
  assert.ok(player.isWaiting);
  player.answered("deny");
  assert.ok(!player.isWaiting);
  advance(5000);
  const after = host.log.slice(host.log.indexOf("hook:PermissionRequest") + 1);
  assert.equal(after[0], "hook:PermissionRequest", "straight on to the question");
  assert.ok(!after.includes("hook:PreToolUse"));
  assert.ok(player.isWaiting);
  player.stop();
});

test("the timeline loops: the second time round starts from a fresh reset", () => {
  const host = recordingHost();
  const player = new DemoPlayer(host, { loop: true });
  player.start();
  advance(loopDuration());
  assert.equal(player.loops, 1);
  assert.equal(host.log.filter((l) => l === "reset").length, 2);
  // The first loop, as it ended: tested, answered, finished, chatted, asleep.
  const first = host.log.slice(0, host.log.lastIndexOf("reset"));
  for (const entry of ["decide:allow", "answer", "chat:false", "fold", "mood:sleeping", `focus:${FINANCE_ID}`]) {
    assert.ok(first.includes(entry), entry);
  }
  advance(loopDuration());
  assert.equal(player.loops, 2);
  player.stop();
  advance(loopDuration());
  assert.equal(player.loops, 2, "stopped");
});

test("?loop=0 plays once and stays on the last moment", () => {
  const host = recordingHost();
  const player = new DemoPlayer(host, { loop: false });
  player.start();
  advance(loopDuration() * 2);
  assert.equal(player.loops, 1);
  assert.ok(player.frozen);
  assert.equal(host.log.filter((l) => l === "reset").length, 1);
  assert.equal(host.log.at(-1), "mood:sleeping");
});

test("query parameters: a known scene, loop=0, the language, a transparent page", () => {
  assert.deepEqual(readParams("?scene=approval&loop=0&lang=fr&bg=transparent"),
    { scene: "approval", loop: false, lang: "fr", transparent: true });
  assert.deepEqual(readParams(""), { scene: null, loop: true, lang: "", transparent: false });
  assert.equal(readParams("?scene=nope").scene, null);
});

// ── On the real hooks pipeline and State ──────────────────────────────────────

test("a full loop on the island: session, card, question, Codex, finish, pills, chat, sleep", async () => {
  const session = startDemo(island, { app: true, loop: true });
  assert.equal(State.mode, "compact");
  assert.equal(task().name, "Claude Code");
  assert.equal(task(FINANCE_ID).name, "62% of budget");

  // Working, then the permission card.
  await playUntil(() => task().state === "working", 10_000, "working");
  assert.equal(task().name, "ciel-demo");
  assert.equal(State.mode, "expanded");
  await playUntil(() => State.pendingApproval != null, 30_000, "the permission card");
  assert.equal(State.pendingApproval.command, "Bash · npm test");
  assert.equal(State.view, "approval");
  assert.equal(task().state, "approval");
  assert.equal(State.sessionDiffs.get(CLAUDE).length, 2);
  assert.deepEqual(viewCommands, ["toggleDiff", "toggleDiff"]);

  // A click on Allow carries the script on at once.
  island.viewActions.decide("allow");
  assert.equal(State.pendingApproval, null);
  await playUntil(() => task().steps.at(-1) === "Runs · npm test", 2000, "the tests run");
  await playUntil(() => State.pendingApproval?.questions != null, 10_000, "the question");
  assert.equal(State.pendingApproval.questions[0].options.length, 3);
  assert.equal(State.view, "question");

  // Nobody answers: Ciel picks the first option after a while, and goes on.
  const waited = await playUntil(() => State.pendingApproval == null, AUTO_ANSWER_MS + 200, "the auto answer");
  assert.ok(waited >= AUTO_ANSWER_MS - 100, `answered after ${waited} ms`);
  await playUntil(() => task("agent_codex")?.state === "thinking", 5000, "Codex");
  assert.equal(task().state, "working", "Claude Code carries on meanwhile");
  assert.equal(State.focusId, CLAUDE);
  await playUntil(() => task().state === "finished", 15_000, "finished");
  assert.equal(task().finalLine, data.FINAL_LINE);
  assert.equal(State.view, "finished");
  await playUntil(() => task("agent_codex")?.state === "finished", 6000, "Codex finishes");
  assert.equal(task("agent_codex").pillBadge, "finished");

  await playUntil(() => State.focusId === data.GITHUB_ID, 10_000, "GitHub");
  assert.equal(State.integrations[data.GITHUB_ID].data.pulse.mainCI[0].ci, "pending");
  await playUntil(() => State.integrations[data.GITHUB_ID].data.pulse.mainCI[0].ci === "success", 5000, "CI");
  await playUntil(() => State.focusId === FINANCE_ID, 5000, "Finance");

  // The chat: typed, sent, streamed, answered.
  await playUntil(() => State.view === "prompt", 10_000, "the chat");
  await playUntil(() => State.chatHistory.length === 1, 5000, "the question sent");
  assert.equal(State.chatHistory[0].content, data.CHAT_QUESTION);
  assert.equal(State.stateOverride, "thinking");
  await playUntil(() => State.chatHistory.length === 2, 10_000, "the answer");
  assert.equal(State.chatHistory[1].content, data.CHAT_ANSWER);

  await playUntil(() => State.stateOverride === "sleeping", 15_000, "asleep");
  assert.equal(State.mode, "compact");
  await playUntil(() => session.player.loops === 1, 10_000, "the end of the loop");
  assert.equal(State.stateOverride, null);
  assert.deepEqual(State.chatHistory, []);
  assert.equal(task().name, "Claude Code");
  await stop(session);
});

test("start → stop gives the island back exactly as it was", async () => {
  realIsland();
  island.fsm.reveal();
  const before = picture();
  const realDiff = State.sessionDiffs.get(CLAUDE)[0];

  const session = startDemo(island, { app: true, loop: true });
  await play(30_000);
  assert.notDeepEqual(picture(), before);
  // Real hook events during the demo are ignored; a real card is sent back to its terminal.
  hook({ hook_event_name: "PreToolUse", cwd: "/work/real-project", session_id: "real", tool_name: "Bash", tool_input: { command: "ls" } });
  hook({ hook_event_name: "PermissionRequest", cwd: "/work/real-project", session_id: "real", request_id: "real-1", tool_name: "Bash" });
  assert.deepEqual(sent("approval_decline"), [{ requestId: "real-1" }]);

  await stop(session);
  assert.ok(session.stopped);
  assert.deepEqual(picture(), before);
  // The real diff still expires on its own clock; the demo's left nothing behind.
  assert.equal(State.findDiff(CLAUDE, realDiff.id)?.path, realDiff.path);
  mock.timers.tick(3_600_000);
  assert.equal(State.sessionDiffs.size, 0);
});

test("stopped while the chat's answer streams: the answer lands before your chat comes back", async () => {
  realIsland();
  const before = picture();
  const session = startDemo(island, { app: true, loop: true });
  await playUntil(() => State.chatHistory.length === 1, 120_000, "the demo's question");
  await play(1500);
  assert.equal(State.chatHistory.length, 1, "still streaming");
  await stop(session);
  assert.deepEqual(State.chatHistory, before.chatHistory);
  assert.deepEqual(picture(), before);
  // The chat field is free again: a question of yours goes to Rust.
  field.value = "mine";
  field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
  assert.deepEqual(sent("chat_send"), [{ query: "mine", context: null }]);
});

test("a session that stopped just before the demo is idle after it, not stuck finished", async () => {
  hook({ hook_event_name: "SessionStart", cwd: "/work/p", session_id: "s" });
  hook({ hook_event_name: "Stop", cwd: "/work/p", session_id: "s", last_assistant_message: "done" });
  assert.equal(task().state, "finished");
  const session = startDemo(island, { app: true, loop: true });
  await play(10_000);
  await stop(session);
  assert.equal(task().state, "idle");
  mock.timers.tick(10_000);
  assert.equal(task().state, "idle");
});

test("nothing the demo does reaches a command that writes", async () => {
  const session = startDemo(island, { app: true, loop: true });
  calls.length = 0;
  await play(loopDuration() + 2000, 200);
  assert.equal(session.player.loops, 1);

  // What the island's own buttons would send, mid-demo.
  await Bridge.saveSettings(State.settings);
  await Bridge.log("x");
  await Bridge.openUrl("https://github.com/demo-org/ciel-demo/pull/42");
  await Bridge.recapMarkShown("2026-10-19");
  await Bridge.recapClear();
  await Bridge.chatReset();
  await assert.rejects(Bridge.secretSet("github-token", "x"), /demo/);
  await assert.rejects(Bridge.secretClear("github-token"), /demo/);
  await assert.rejects(Bridge.hooksApply(true, "f"), /demo/);
  await assert.rejects(Bridge.agentHooksApply("codex", true, "f"), /demo/);
  await assert.rejects(Bridge.statusLineApply(true, "f"), /demo/);
  await assert.rejects(Bridge.ingestFile("/x.txt"), /demo/);
  await assert.rejects(Bridge.wallpaperSetDesktop("m", new Uint8Array(4)), /demo/);
  // An expense is added in memory, and shows on the card.
  const poll = await Bridge.financeAddExpense({ amount: 10, currency: "USD", category: "Groceries", note: "", date: State.integrations[FINANCE_ID].data.today });
  assert.equal(poll.summary.spentMonth, 1250);
  assert.equal(await Bridge.secretPresent("github-token"), true);

  const reached = calls.map(([cmd]) => cmd);
  for (const cmd of reached) assert.ok(WINDOW_COMMANDS.has(cmd), `${cmd} reached Rust during the demo`);
  for (const cmd of [
    "save_settings", "secret_set", "secret_clear", "hooks_apply", "agent_hooks_apply", "status_line_apply",
    "finance_add_expense", "finance_poll", "recap_mark_shown", "recap_clear", "log_line", "open_url",
    "approval_decision", "approval_answer", "approval_ack", "chat_send", "wallpaper_set_desktop",
  ]) {
    assert.deepEqual(sent(cmd), [], cmd);
  }
  await stop(session);
  // Once it is over, the bridge talks to Rust again.
  await Bridge.saveSettings(State.settings);
  assert.equal(sent("save_settings").length, 1);
});

test("in a browser page, not even the window commands go anywhere", async () => {
  const session = startDemo(island, { app: false, loop: true });
  calls.length = 0;
  await Bridge.setCollapsed(true);
  await Bridge.focusWindow(true);
  await play(5000);
  assert.deepEqual(calls, []);
  await stop(session);
});

test("a settings change made meanwhile is held, then applied once the demo is over", async () => {
  const seen = [];
  await onEvent("settings-changed", (s) => seen.push(s));
  const session = startDemo(island, { app: true, loop: true });
  emit("settings-changed", { soundVolume: 0.01 });
  emit("settings-changed", { soundVolume: 0.02 });
  assert.deepEqual(seen, []);
  await stop(session);
  assert.deepEqual(seen, [{ soundVolume: 0.02 }]);
});

test("the tray's Pause ends the demo first, then pauses", async () => {
  const seen = [];
  await onEvent("tray", (what) => seen.push([what, State.settings.activeIntegrations.includes(FINANCE_ID)]));
  const session = startDemo(island, { app: true, loop: true });
  emit("tray", "pause");
  mock.timers.tick(1);
  await microtasks();
  assert.ok(session.stopped);
  // Handed over after the island was restored: the demo's Finance pill is gone.
  assert.deepEqual(seen, [["pause", false]]);
});

// ── ?scene= ───────────────────────────────────────────────────────────────────

/** Starts frozen on `scene`, checks it, checks that nothing moves for a minute. */
async function frozen(scene, check) {
  const session = startDemo(island, { app: false, loop: true, scene });
  await microtasks();
  mock.timers.tick(200);
  await microtasks();
  assert.ok(session.player.frozen, scene);
  check();
  const index = session.player.index;
  const shown = picture();
  await play(60_000, 1000);
  assert.equal(session.player.index, index, `${scene}: the script moved on`);
  if (scene === "approval" || scene === "question") assert.deepEqual(picture().pendingApproval, shown.pendingApproval);
  assert.ok(State.isPinned, "a frozen scene stays open");
  check();
  await stop(session);
}

test("?scene=session: Claude Code at work in ciel-demo, two edits in", async () => {
  await frozen("session", () => {
    assert.equal(task().name, "ciel-demo");
    assert.equal(task().state, "working");
    assert.equal(State.sessionDiffs.get(CLAUDE).length, 2);
    assert.equal(State.view, "overview");
    assert.equal(State.pendingApproval, null);
  });
});

test("?scene=diff: the live diff is open", async () => {
  await frozen("diff", () => {
    assert.deepEqual(viewCommands, ["toggleDiff"]);
    assert.equal(State.sessionDiffs.get(CLAUDE).length, 1);
  });
});

test("?scene=approval: npm test waits for Allow or Deny, and never answers itself", async () => {
  await frozen("approval", () => {
    assert.equal(State.pendingApproval?.command, "Bash · npm test");
    assert.equal(State.view, "approval");
    assert.equal(task().state, "approval");
  });
});

test("?scene=question: three options wait", async () => {
  await frozen("question", () => {
    assert.deepEqual(State.pendingApproval?.questions?.[0].options.map((o) => o.label),
      data.QUESTION.options.map((o) => o.label));
    assert.equal(State.view, "question");
  });
});

test("?scene=finished: the finished card, with the session's last words", async () => {
  await frozen("finished", () => {
    assert.equal(task().state, "finished");
    assert.equal(task().finalLine, data.FINAL_LINE);
    assert.equal(State.view, "finished");
  });
});

test("?scene=finance: the Finance pill in front, at 62% of the budget", async () => {
  await frozen("finance", () => {
    assert.equal(State.focusId, FINANCE_ID);
    assert.equal(State.view, "overview");
    assert.equal(State.integrations[FINANCE_ID].data.spentMonth, 1240);
  });
});

test("?scene=chat: the question and its whole answer", async () => {
  await frozen("chat", () => {
    assert.equal(State.view, "prompt");
    assert.deepEqual(State.chatHistory.map((m) => m.content), [data.CHAT_QUESTION, data.CHAT_ANSWER]);
  });
});

// ── The website's buttons: playFrom() ─────────────────────────────────────────

test("playFrom: mid-loop, the permission card slides in live and waits, then the loop goes on", async () => {
  const session = startDemo(island, { app: false, loop: true });
  await play(3000);
  session.player.playFrom("approval");
  assert.equal(State.pendingApproval, null, "the card comes with its own step, live");
  await play(PLAY_FROM_MS + 100);
  assert.equal(State.pendingApproval?.command, "Bash · npm test");
  assert.equal(State.view, "approval");
  assert.ok(session.player.isWaiting, "it waits for Allow or Deny");
  assert.ok(!session.player.frozen, "and the script carries on afterwards");
  await playUntil(() => State.pendingApproval?.questions != null, AUTO_ANSWER_MS + 10_000, "the question that follows");
  await stop(session);
});

test("playFrom: the finished card, then the chat, each from its own first step", async () => {
  const session = startDemo(island, { app: false, loop: true });
  session.player.playFrom("finished");
  await play(PLAY_FROM_MS + 200);
  assert.equal(task().state, "finished");
  assert.equal(task().finalLine, data.FINAL_LINE);
  session.player.playFrom("chat");
  await playUntil(() => State.chatHistory[1]?.content === data.CHAT_ANSWER, 20_000, "the chat's answer");
  assert.equal(State.view, "prompt");
  assert.equal(State.chatHistory[0].content, data.CHAT_QUESTION);
  await stop(session);
});

test("playFrom: an unknown moment, or a stopped demo, changes nothing", async () => {
  const session = startDemo(island, { app: false, loop: true });
  await play(2000);
  const index = session.player.index;
  session.player.playFrom("nope");
  assert.equal(session.player.index, index);
  await stop(session);
  session.player.playFrom("approval");
  await play(1000);
  assert.equal(State.pendingApproval, null);
});
