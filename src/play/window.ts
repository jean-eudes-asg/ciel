// The games' own window (games.html): the Games page out of the Ciel window,
// to play beside something else, as big as you like. Rust makes it hidden at
// launch (src-tauri/src/play.rs); the game is built when it is shown and
// dropped when it is hidden, so a hidden window draws nothing.

import "../style.css";
import "../hub/hub.css";
import { h } from "../views/dom";
import { t, resolveLanguage, setLanguage, systemLanguages } from "../i18n/i18n";
import { Bridge, onEvent } from "../core/bridge";
import { Sound } from "../core/sound";
import type { Settings } from "../core/state";
import { icon } from "../hub/ui";
import { games } from "./games";

const root = document.getElementById("games")!;
document.body.classList.add("hub-window", "games-window");
let shown: { leave(): void } | null = null;
let onTop = false;

function follow(s: Partial<Settings> | null | undefined) {
  if (!s) return;
  if (s.language !== undefined) setLanguage(resolveLanguage(s.language, systemLanguages()));
  if (s.soundEnabled !== undefined) Sound.setEnabled(s.soundEnabled);
  if (s.soundVolume !== undefined) Sound.setVolume(s.soundVolume);
}

function open() {
  shown?.leave();
  const pin = h("button", { class: "hub-btn small games-pin", title: t("Keep this window over the others") }, icon("lock"), h("span", { text: t("On top") }));
  pin.classList.toggle("on", onTop);
  pin.addEventListener("click", () => {
    onTop = !onTop;
    pin.classList.toggle("on", onTop);
    void Bridge.gamesOnTop(onTop);
  });
  const back = h("button", { class: "hub-btn small", title: t("Back into the Ciel window"), onclick: () => void Bridge.gamesWindow(false) },
    icon("home"), h("span", { text: t("Back to Ciel") }));
  const g = games([pin, back]);
  shown = g;
  root.replaceChildren(g.el);
}

function close() {
  shown?.leave();
  shown = null;
  onTop = false;
  root.replaceChildren();
}

void (async () => {
  follow((await Bridge.boot())?.settings);
  await onEvent<boolean>("games-shown", open);
  await onEvent<boolean>("games-hidden", close);
  await onEvent<Settings>("settings-changed", follow);
})();
