// Plays the demo's timeline (timeline.ts) against a host: the real island in
// the app and in the browser (host.ts), a recording stand-in in the tests.
// One step at a time on a timer; a card waits for its answer, which arrives
// through `answered()` — from a click, or from Ciel answering it himself after
// a while. Nothing here touches the DOM or the bridge.

import type { HookPayload } from "../island/hooks";
import type { BotStateName } from "../core/layout";
import { DEMO_TIMELINE, type DemoStep, type SceneName } from "./timeline";

/** What the player asks of the island. */
export interface DemoHost {
  /** Back to the first moment of the loop: fresh pills, data and chat, the island showing. */
  reset(): void;
  hook(payload: HookPayload): void;
  open(view: "overview" | "prompt"): void;
  focus(pillId: string): void;
  toggleDiff(): void;
  /** Answers the permission card, as its button would. */
  decide(decision: "allow" | "deny"): void;
  /** Answers the question card, as its options would. */
  answer(answers: Record<string, string>): void;
  ci(passed: boolean): void;
  /** Types `question` in the chat and sends it; `instant` skips the typing. */
  chat(question: string, instant: boolean): void;
  fold(): void;
  mood(state: BotStateName | null): void;
  /** A frozen scene: the island stays open on it. */
  hold(): void;
  /** Skipping ahead to a scene: true while it runs every step at once (no sounds, no streaming). */
  setFast(fast: boolean): void;
}

/** How a demo card was answered. "terminal": handed to the terminal, which counts as a no. */
export type DemoAnswer = "allow" | "deny" | "answer" | "terminal";

export interface PlayerOptions {
  /** Start again after the last step (else stay on it). */
  loop: boolean;
  /** Skip ahead to this moment and stay there. */
  scene?: SceneName | null;
  steps?: readonly DemoStep[];
}

export class DemoPlayer {
  /** Loops completed. */
  loops = 0;
  /** The step to run next. */
  index = 0;
  /** Frozen on a scene, or at the end of a single play. */
  frozen = false;

  private readonly host: DemoHost;
  private readonly options: PlayerOptions;
  private readonly steps: readonly DemoStep[];
  private timer: number | null = null;
  private autoTimer: number | null = null;
  /** The card being waited on, if any. */
  private waiting: "permission" | "question" | null = null;
  private allowed = true;
  private running = false;

  constructor(host: DemoHost, options: PlayerOptions) {
    this.host = host;
    this.options = options;
    this.steps = options.steps ?? DEMO_TIMELINE;
  }

  get isWaiting(): boolean {
    return this.waiting != null;
  }

  start() {
    this.running = true;
    this.begin();
    if (this.options.scene) this.skipTo(this.options.scene);
    else this.schedule();
  }

  stop() {
    this.running = false;
    this.clearTimers();
    this.waiting = null;
  }

  /** A demo card got its answer: the script carries on from there. */
  answered(how: DemoAnswer) {
    if (!this.running || this.frozen || this.waiting == null) return;
    if (this.waiting === "permission") this.allowed = how === "allow";
    this.waiting = null;
    if (this.autoTimer != null) window.clearTimeout(this.autoTimer);
    this.autoTimer = null;
    this.index++;
    this.schedule();
  }

  // ── Running ─────────────────────────────────────────────────────────────────

  private begin() {
    this.clearTimers();
    this.index = 0;
    this.allowed = true;
    this.waiting = null;
    this.frozen = false;
    this.host.reset();
  }

  private schedule() {
    if (!this.running) return;
    const step = this.steps[this.index];
    if (!step) return;
    this.timer = window.setTimeout(() => {
      this.timer = null;
      this.runNext();
    }, step.after);
  }

  private runNext() {
    const step = this.steps[this.index];
    if (!step || !this.running) return;
    if (step.ifAllowed && !this.allowed) {
      this.index++;
      return this.schedule();
    }
    const action = step.action;
    if (action.kind === "end") {
      this.loops++;
      if (this.options.loop) {
        this.begin();
        this.schedule();
      } else {
        this.frozen = true;
      }
      return;
    }
    this.run(step, false);
    if (action.kind === "permission" || action.kind === "question") {
      // Ciel answers it himself if nobody does, so the loop goes on.
      this.waiting = action.kind;
      this.autoTimer = window.setTimeout(() => {
        this.autoTimer = null;
        this.autoAnswer(step);
        // A host whose card was already gone never reports back: go on anyway.
        if (this.waiting) this.answered(action.kind === "permission" ? "allow" : "answer");
      }, action.autoMs);
      return;
    }
    this.index++;
    this.schedule();
  }

  /** Runs one step now. */
  private run(step: DemoStep, fast: boolean) {
    const a = step.action;
    switch (a.kind) {
      case "hook":
      case "permission":
      case "question":
        this.host.hook(a.payload);
        break;
      case "open":
        this.host.open(a.view);
        break;
      case "focus":
        this.host.focus(a.pill);
        break;
      case "diff":
        this.host.toggleDiff();
        break;
      case "ci":
        this.host.ci(a.passed);
        break;
      case "chat":
        this.host.chat(a.question, fast);
        break;
      case "fold":
        this.host.fold();
        break;
      case "mood":
        this.host.mood(a.state);
        break;
      case "scene":
      case "end":
        break;
    }
  }

  private autoAnswer(step: DemoStep) {
    const a = step.action;
    if (a.kind === "permission") this.host.decide("allow");
    else if (a.kind === "question") this.host.answer(a.auto);
  }

  /**
   * `?scene=`: every step up to the scene runs at once — cards answered on the
   * spot, the chat sent without typing — then the island stays on that moment.
   * A card the scene is about stays up, unanswered.
   */
  private skipTo(scene: SceneName) {
    this.host.setFast(true);
    try {
      while (this.index < this.steps.length) {
        const step = this.steps[this.index];
        const a = step.action;
        if (a.kind === "scene" && a.name === scene) break;
        this.index++;
        if (a.kind === "end") break;
        if (step.ifAllowed && !this.allowed) continue;
        this.run(step, true);
        if (a.kind !== "permission" && a.kind !== "question") continue;
        const next = this.steps[this.index]?.action;
        const isTheScene = next?.kind === "scene" && next.name === scene;
        if (!isTheScene) this.autoAnswer(step);
      }
    } finally {
      this.host.setFast(false);
    }
    this.frozen = true;
    this.host.hold();
  }

  private clearTimers() {
    if (this.timer != null) window.clearTimeout(this.timer);
    if (this.autoTimer != null) window.clearTimeout(this.autoTimer);
    this.timer = null;
    this.autoTimer = null;
  }
}
