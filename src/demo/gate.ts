// The demo's bridge gate (see BridgeGate in core/bridge.ts). While the demo
// runs, every command the island sends lands here first:
//   - in the app, the few that only move, size or focus the island's window go
//     on to Rust — the island has to open and take clicks;
//   - a real agent's request is still handed back to its terminal at once, so
//     no agent ever waits on the demo;
//   - everything else is answered from the demo's made-up data, or swallowed.
// So nothing the demo shows is read from the user's machine, and nothing is
// written: no settings, no keychain, no hooks, no recap history, no finance
// file, no log line.
//
// Events from Rust: real hook events are ignored (a permission request is
// declined, as when the island is paused), pollers' updates are dropped, and
// what must not be lost — a settings change made meanwhile, the tray's Pause
// or Weekly recap — is held and handed over once the demo has stopped.

import { Bridge, type BridgeGate } from "../core/bridge";
import type { NewExpense } from "../core/finance";
import { t } from "../i18n/i18n";
import { DEMO_HOOKS, DEMO_KEYS, isDemoRequest, type DemoFinance } from "./data";
import type { DemoAnswer } from "./player";

/** Commands that only move, size or focus the island's window. */
export const WINDOW_COMMANDS: ReadonlySet<string> = new Set([
  "set_collapsed", "set_island_rect", "focus_window", "fit_zoom", "reposition", "open_settings_window",
]);

/**
 * Commands that would change something outside the page and that report
 * failure: they are refused with a reason rather than pretending to work.
 */
const REFUSED: ReadonlySet<string> = new Set([
  "ingest_file", "recap_save_png", "secret_set", "secret_clear", "hooks_preview", "hooks_apply",
  "agent_hooks_preview", "agent_hooks_apply", "status_line_preview", "status_line_apply", "local_connect",
  "wallpaper_set_desktop", "wallpaper_set_lock_screen", "wallpaper_photos_fetch",
]);

/** Events from Rust that would put real data on the demo's screen. */
const DROPPED: ReadonlySet<string> = new Set([
  "integration", "github-alerts", "spotify", "spotify-artwork", "chat-delta", "recap-check", "voice-ask",
]);

export interface GateHooks {
  /** A demo card was answered (the island's own buttons call through here). */
  answered(how: DemoAnswer): void;
  /** The chat's answer; it streams through `chat-delta` meanwhile. */
  chat(query: string): Promise<{ text: string }>;
  /** Something that must end the demo first (the tray's Pause, its Weekly recap). */
  stop(): void;
  /** An event held back, to hand over once the demo has stopped. */
  hold(name: string, payload: unknown): void;
}

export interface DemoGateOptions {
  /** In the app, the window commands go through to Rust. */
  app: boolean;
  finance: DemoFinance;
  /** The model the chat says it uses. */
  model: () => string;
}

const answer = (value: unknown) => ({ value });
const refuse = () => answer(Promise.reject(new Error(t("Not available in the demo."))));

export function demoGate(options: DemoGateOptions, hooks: GateHooks): BridgeGate {
  return {
    command(cmd, args) {
      const a = args ?? {};
      if (options.app && WINDOW_COMMANDS.has(cmd)) return undefined;
      if (REFUSED.has(cmd)) return refuse();
      switch (cmd) {
        // The demo's cards are answered here; a real agent's request is
        // declined straight to its terminal, demo or not.
        case "approval_decision":
          if (!isDemoRequest(a.requestId)) return options.app ? undefined : answer(null);
          hooks.answered(a.decision === "allow" ? "allow" : "deny");
          return answer(null);
        case "approval_answer":
          if (!isDemoRequest(a.requestId)) return options.app ? undefined : answer(null);
          hooks.answered("answer");
          return answer(null);
        case "approval_decline":
          if (!isDemoRequest(a.requestId)) return options.app ? undefined : answer(null);
          hooks.answered("terminal");
          return answer(null);

        // Reads, from the made-up data.
        case "secret_present":
          return answer(DEMO_KEYS.has(String(a.key)));
        case "agent_hooks_status":
          return answer({ ...DEMO_HOOKS });
        case "chat_send":
          return answer(hooks.chat(String(a.query ?? "")));
        case "chat_models":
          return answer([{ id: options.model(), label: options.model() }]);
        case "finance_poll":
          return answer(options.finance.poll(a.stamp));
        case "finance_add_expense":
          return answer(options.finance.add(a.expense as NewExpense));
        case "desktop_cloud_pick_up":
          // Ciel stays in the island: where he lives on the desktop is saved.
          return answer(false);
      }
      // Anything else: nothing to say (a read), or nothing done (a write).
      return answer(null);
    },

    event(name, payload) {
      if (name === "hook") {
        const p = payload as { hook_event_name?: string; request_id?: string } | null;
        // Never keep an agent waiting on a demo: its terminal asks at once.
        if (p?.hook_event_name === "PermissionRequest" && p.request_id && !isDemoRequest(p.request_id)) {
          void Bridge.approvalDecline(p.request_id);
        }
        return false;
      }
      if (DROPPED.has(name)) return false;
      if (name === "settings-changed") {
        hooks.hold(name, payload);
        return false;
      }
      if (name === "tray" && (payload === "pause" || payload === "recap")) {
        hooks.hold(name, payload);
        hooks.stop();
        return false;
      }
      return true;
    },
  };
}
