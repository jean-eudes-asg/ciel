// The player's host on the real island: each step goes through the code a real
// event or a click would — handleHook for hook events, the views' own actions
// for the buttons, Ctrl+E's command for the diff, keys typed in the chat field
// — and sample data goes where the integrations keep theirs.

import { State } from "../core/state";
import { Sound } from "../core/sound";
import type { BotStateName } from "../core/layout";
import { handleHook, settleHookTimers } from "../island/hooks";
import { handleGitHubAlerts } from "../island/integrations";
import { applyFinance } from "../island/finance";
import type { Island } from "../island/island";
import {
  GITHUB_ID, WATCHED_PR, demoIntegrations, demoPlans, githubData, type DemoFinance,
} from "./data";
import type { DemoHost } from "./player";

/** What the demo needs of the island (the tests hand in a stand-in). */
export type DemoIsland = Pick<
  Island,
  | "alert" | "setView" | "reveal" | "dropPin" | "collapse" | "viewCommand" | "applySettings"
  | "setPinned" | "takeKeyboard" | "fsm" | "viewActions"
>;

/** Typing speed in the chat field, ms per character. */
const TYPE_MS = 42;

export interface IslandHost extends DemoHost {
  /** Cancels whatever is still on a timer (the typing). */
  dispose(): void;
}

export function islandHost(island: DemoIsland, finance: DemoFinance, now: () => Date = () => new Date()): IslandHost {
  const timers = new Set<number>();
  let fast = false;
  /** Ctrl+E pressed while skipping ahead: whether the diff should end up open. */
  let diffWanted = false;
  /** The sounds as the person set them, while skipping ahead mutes them. */
  let soundWas: boolean | null = null;

  const later = (fn: () => void, ms: number) => {
    const id = window.setTimeout(() => {
      timers.delete(id);
      fn();
    }, ms);
    timers.add(id);
  };

  /**
   * Opens the island on `view`, as a click would. An island opened by a hover
   * becomes an ordinary open one, so the demo is not folded away under it.
   */
  const show = (view: "overview" | "prompt") => island.alert(view);

  return {
    reset() {
      // Whatever the last loop left: cards, timers, diffs, the chat, moods.
      settleHookTimers("drop");
      for (const id of [...State.sessionDiffs.keys()]) State.clearSessionDiffs(id);
      State.pendingApproval = null;
      State.focusBeforeApproval = null;
      State.isPinned = false;
      island.fsm.pinned = false;
      State.stateOverride = null;
      State.chatHistory = [];
      State.droppedFile = null;
      State.promptContext = null;
      State.noteMessage = null;
      State.searchResult = null;
      State.showingPlanDetail = false;
      State.cardSelection = null;
      // The pills the demo declares, fresh, and their sample data.
      State.tasks = [];
      State.focusId = null;
      State.integrations = demoIntegrations(now());
      State.loadIntegrationTasks();
      finance.reset();
      applyFinance(finance.poll(null)!);
      const plans = demoPlans(now().getTime());
      State.planUsage = plans.claude;
      State.codexPlanUsage = plans.codex;
      if (State.mode === "expanded") island.collapse();
      island.reveal();
      State.notify();
    },

    hook(payload) {
      handleHook(island as Island, payload);
    },

    open(view) {
      show(view);
    },

    focus(id) {
      if (State.mode !== "expanded" || State.view !== "overview") show("overview");
      island.viewActions.setFocus(id);
    },

    toggleDiff() {
      if (fast) {
        diffWanted = !diffWanted;
        return;
      }
      if (State.view === "overview") island.viewCommand("toggleDiff");
    },

    decide(decision) {
      if (State.pendingApproval) island.viewActions.decide(decision);
    },

    answer(answers) {
      if (State.pendingApproval?.questions) island.viewActions.answer(answers);
    },

    ci(passed) {
      const info = State.integrations[GITHUB_ID];
      State.integrations[GITHUB_ID] = {
        data: githubData(passed, now()), error: null, loaded: true, configured: info?.configured ?? true,
      };
      // The poller's own path for a CI run that turned green.
      if (passed) handleGitHubAlerts([{ kind: "ciPassed", prId: WATCHED_PR }]);
      State.notify();
    },

    chat(question, instant) {
      show("prompt");
      const field = document.querySelector<HTMLInputElement>("#views .chat-input");
      if (!field) return;
      const send = () => field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      if (instant) {
        field.value = question;
        send();
        return;
      }
      field.value = "";
      let typed = 0;
      const type = () => {
        typed++;
        field.value = question.slice(0, typed);
        if (typed < question.length) later(type, TYPE_MS);
        else later(send, 350);
      };
      // The field takes the focus a moment after the chat opens.
      later(type, 400);
    },

    fold() {
      island.collapse();
    },

    mood(state: BotStateName | null) {
      State.stateOverride = state;
      State.notify();
    },

    hold() {
      // A frozen moment stays as it is: no return to idle 5 s after a Stop,
      // no card withdrawn after its timeout.
      settleHookTimers("drop");
      island.setPinned(true);
    },

    setFast(on) {
      fast = on;
      if (on) {
        soundWas = Sound.enabled;
        Sound.setEnabled(false);
        diffWanted = false;
        return;
      }
      if (soundWas != null) Sound.setEnabled(soundWas);
      soundWas = null;
      // The overview has to have drawn the session once before a diff opens on it.
      if (diffWanted) {
        diffWanted = false;
        later(() => {
          if (State.view === "overview") island.viewCommand("toggleDiff");
        }, 120);
      }
    },

    dispose() {
      for (const id of timers) window.clearTimeout(id);
      timers.clear();
    },
  };
}
