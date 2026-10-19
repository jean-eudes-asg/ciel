// One run of the demo: the island's state put aside, the bridge gated, the
// timeline playing on the real island — and, on stop, everything given back
// exactly as it was.

import { emitLocal, setBridgeGate } from "../core/bridge";
import { State, type Settings } from "../core/state";
import { settleHookTimers } from "../island/hooks";
import { t } from "../i18n/i18n";
import { CHAT_ANSWER, CHAT_QUESTION, DemoFinance, demoSettings } from "./data";
import { demoGate } from "./gate";
import { islandHost, type DemoIsland } from "./host";
import { DemoPlayer } from "./player";
import { restoreSnapshot, takeSnapshot } from "./snapshot";
import type { SceneName } from "./timeline";

export interface DemoOptions {
  /** In the app (window commands reach Rust), rather than in a browser page. */
  app: boolean;
  /** Start again at the end (else stay on the last moment). */
  loop: boolean;
  /** Freeze on this moment instead of playing. */
  scene?: SceneName | null;
  /** Settings of the demo's own on top of the person's (the browser page mutes it). */
  settings?: Partial<Settings>;
  /** Called once the island is back as it was. */
  onStopped?: () => void;
}

export interface DemoSession {
  readonly player: DemoPlayer;
  /** Ends the demo; resolves once the island is back as it was. */
  stop(): Promise<void>;
  readonly stopped: boolean;
}

/** A day: the compact island never slips away while the demo runs. */
const KEEP_SHOWN_S = 86_400;

/** Words of the streamed answer per chunk, and the pause between chunks (ms). */
const STREAM_WORDS = 2;
const STREAM_MS = 60;
/** The typing dots before the first words. */
const STREAM_DELAY_MS = 900;

export function startDemo(island: DemoIsland, options: DemoOptions): DemoSession {
  // A session that stopped a moment ago is idle now rather than after the demo.
  settleHookTimers("run");
  const snapshot = takeSnapshot(island);
  const finance = new DemoFinance();
  const timers = new Set<number>();
  /** Chat answers still streaming: finished at once when the demo stops. */
  const streaming = new Set<() => void>();
  const held: [string, unknown][] = [];
  let fast = false;
  let stopped = false;
  let stopping: Promise<void> | null = null;

  const later = (fn: () => void, ms: number) => {
    const id = window.setTimeout(() => {
      timers.delete(id);
      fn();
    }, ms);
    timers.add(id);
  };

  /** The chat's answer: the scripted one, streamed a few words at a time. */
  const chat = (query: string): Promise<{ text: string }> => {
    const text = query.trim() === CHAT_QUESTION
      ? CHAT_ANSWER
      : t("This is a demo, so my answers are scripted. In Ciel, the chat talks to the model you connect.");
    if (fast) return Promise.resolve({ text });
    return new Promise((resolve) => {
      const parts = text.match(/\S+\s*/g) ?? [text];
      let shown = 0;
      const complete = () => {
        streaming.delete(complete);
        resolve({ text });
      };
      streaming.add(complete);
      const step = () => {
        if (!streaming.has(complete)) return;
        shown = Math.min(parts.length, shown + STREAM_WORDS);
        emitLocal("chat-delta", parts.slice(0, shown).join(""));
        if (shown < parts.length) later(step, STREAM_MS);
        else later(complete, 200);
      };
      later(step, STREAM_DELAY_MS);
    });
  };

  const host = islandHost(island, finance);
  const setFast = host.setFast;
  host.setFast = (on) => {
    fast = on;
    setFast(on);
  };
  const player = new DemoPlayer(host, { loop: options.loop, scene: options.scene ?? null });

  const session: DemoSession = {
    player,
    get stopped() {
      return stopped;
    },
    stop() {
      stopping ??= finish();
      return stopping;
    },
  };

  async function finish() {
    player.stop();
    host.dispose();
    for (const id of timers) window.clearTimeout(id);
    timers.clear();
    // An answer still streaming ends now; whatever waits on it, and on any
    // other demo answer already given, runs before the island is restored.
    for (const done of [...streaming]) done();
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    settleHookTimers("drop");
    restoreSnapshot(island, snapshot);
    setBridgeGate(null);
    stopped = true;
    // What happened meanwhile: a change made in Settings, the tray's Pause.
    for (const [name, payload] of held) emitLocal(name, payload);
    options.onStopped?.();
  }

  setBridgeGate(
    demoGate(
      { app: options.app, finance, model: () => State.settings.model },
      {
        answered: (how) => player.answered(how),
        chat,
        stop: () => void session.stop(),
        hold: (name, payload) => {
          // Only the latest settings matter.
          const at = held.findIndex(([n]) => n === name);
          if (name === "settings-changed" && at >= 0) held.splice(at, 1);
          held.push([name, payload]);
        },
      },
    ),
  );

  // The demo's island: its pills and settings; awake, and kept in view.
  State.paused = false;
  State.settings = demoSettings(State.settings, options.settings);
  island.applySettings();
  island.fsm.petitToHiddenDelay = KEEP_SHOWN_S;
  player.start();
  return session;
}
