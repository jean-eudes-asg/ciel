// Games, for a break between two tasks (src/play/games.ts), here or taken
// out to their own window (src/play/window.ts) to play beside something else.

import { h } from "../../views/dom";
import { t } from "../../i18n/i18n";
import { Bridge, onEvent } from "../../core/bridge";
import { emptyState } from "../hero";
import type { HubContext } from "../shell";
import { button, icon } from "../ui";
import { games } from "../../play/games";

export function gamesPage(container: HTMLElement, ctx: HubContext) {
  const holder = h("div", { class: "games-holder" });
  container.append(ctx.hero("games", t("Games"), t("Five games with Ciel, for a break between two tasks."), [], true), holder);
  let shown: { leave(): void } | null = null;

  const draw = (out: boolean) => {
    shown?.leave();
    shown = null;
    if (out) {
      holder.replaceChildren(
        emptyState(t("Your game is in its own window."), "idle"),
        h("div", { class: "games-back" }, button(t("Bring it back"), () => void Bridge.gamesWindow(false), "primary")),
      );
      return;
    }
    const popOut = h("button", { class: "hub-btn small games-out", title: t("Play in its own window, beside your work"), onclick: () => void Bridge.gamesWindow(true) },
      icon("window"), h("span", { text: t("Own window") }));
    const g = games([popOut]);
    shown = g;
    holder.replaceChildren(g.el);
  };

  draw(false);
  void Bridge.gamesWindowOpen().then((out) => {
    if (out) draw(true);
  });
  const unlisten = onEvent<boolean>("games-window", draw);
  ctx.onLeave = () => {
    shown?.leave();
    void unlisten.then((stop) => stop());
  };
}
