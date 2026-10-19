// Demo mode in the app: Settings → General → Try demo mode asks the island to
// play it (DEMO_EVENTS). While it plays, a "Demo" badge sits in the island so
// it is never taken for real activity; its Stop button, Esc, or Stop demo in
// Settings end it, and the island comes back exactly as it was.

import "./demo.css";
import { emitToWindow, onEvent } from "../core/bridge";
import { State } from "../core/state";
import { h } from "../views/dom";
import { tl } from "../i18n/i18n";
import type { Island } from "../island/island";
import { DEMO_EVENTS, SETTINGS_WINDOW, type DemoControl, type DemoStatus } from "./events";
import { startDemo, type DemoSession } from "./session";

export function registerDemo(island: Island) {
  let session: DemoSession | null = null;
  let badge: HTMLElement | null = null;
  let unsubscribe: (() => void) | null = null;

  const report = (status: DemoStatus = { running: session != null }) =>
    void emitToWindow(SETTINGS_WINDOW, DEMO_EVENTS.state, status);

  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || !session) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    void session.stop();
  };

  function start() {
    if (session) return report();
    // A real card waits for an answer: it is not put aside for a demo.
    if (State.pendingApproval) return report({ running: false, refused: "pending" });
    session = startDemo(island, {
      app: true,
      loop: true,
      onStopped: () => {
        session = null;
        badge?.remove();
        badge = null;
        unsubscribe?.();
        unsubscribe = null;
        window.removeEventListener("keydown", onKey, true);
        report();
      },
    });
    badge = buildBadge(() => void session?.stop());
    document.getElementById("island")?.append(badge);
    const syncBadge = () => badge?.setAttribute("data-mode", State.mode);
    unsubscribe = State.subscribe(syncBadge);
    syncBadge();
    window.addEventListener("keydown", onKey, true);
    // So Esc reaches the island straight away.
    island.takeKeyboard();
    report();
  }

  void onEvent<DemoControl>(DEMO_EVENTS.control, (what) => {
    if (what === "start") start();
    else if (what === "stop") void session?.stop();
    else report();
  });
}

/** "Demo", and a Stop button while the island is open. */
function buildBadge(stop: () => void): HTMLElement {
  return h(
    "div",
    { class: "demo-badge", title: tl("Esc stops the demo.") },
    h("span", { class: "demo-badge-label", text: tl("Demo") }),
    h("button", {
      class: "demo-badge-stop",
      text: tl("Stop demo"),
      onclick: (e: Event) => {
        e.stopPropagation();
        stop();
      },
    }),
  );
}
