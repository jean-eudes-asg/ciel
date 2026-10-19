// Settings → General → Demo mode. The demo plays in the island (src/demo/app.ts):
// this row only asks it to start or stop, and shows which it is doing.

import { emitToWindow, onEvent } from "../core/bridge";
import { h } from "../views/dom";
import { t } from "../i18n/i18n";
import { DEMO_EVENTS, ISLAND_WINDOW, type DemoControl, type DemoStatus } from "../demo/events";

let status: DemoStatus = { running: false };
/** The row on screen redraws itself; the section is rebuilt now and then. */
let redraw: (() => void) | null = null;
let listening = false;

const ask = (what: DemoControl) => void emitToWindow(ISLAND_WINDOW, DEMO_EVENTS.control, what);

export function demoRows(): HTMLElement[] {
  // Listened to once, whatever number of times the section is drawn.
  if (!listening) {
    listening = true;
    void onEvent<DemoStatus>(DEMO_EVENTS.state, (next) => {
      status = next;
      redraw?.();
    });
  }
  const button = h("button", { onclick: () => ask(status.running ? "stop" : "start") });
  const note = h("span", { class: "hint" });
  const draw = () => {
    button.textContent = status.running ? t("Stop demo") : t("Try demo mode");
    note.textContent = status.refused === "pending" ? t("Answer the waiting request first.") : "";
  };
  redraw = draw;
  draw();
  ask("status");
  return [
    h("div", { class: "row" }, h("label", { text: t("Demo mode") }), button, note),
    h("div", {
      class: "hint",
      text: t("Plays a short scripted session with made-up projects and numbers. Nothing is saved, and your island comes back as it was. Esc stops it."),
    }),
  ];
}
