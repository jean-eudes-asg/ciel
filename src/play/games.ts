// The five games and their tabs, as the Ciel window's Games page and the
// games' own window (window.ts) both show them. The game left open is the one
// shown next time, in either.

import "./play.css";
import { h } from "../views/dom";
import { t } from "../i18n/i18n";
import { segmented } from "../hub/ui";
import { chessGame } from "./chessboard";
import { draughtsGame } from "./draughtsboard";
import { mazeGame } from "./maze";
import { cloudsGame } from "./cloudsgame";
import { starsGame } from "./starsgame";

const GAMES = {
  chess: chessGame,
  draughts: draughtsGame,
  maze: mazeGame,
  clouds: cloudsGame,
  stars: starsGame,
};
type Game = keyof typeof GAMES;
const KEY = "ciel.game";

/** The tabs, `extra` beside them (the window's buttons), and the game picked. */
export function games(extra: HTMLElement[] = []): { el: HTMLElement; leave(): void } {
  let which: Game = "chess";
  try {
    const kept = localStorage.getItem(KEY);
    if (kept && kept in GAMES) which = kept as Game;
  } catch {
    // Chess, then.
  }
  const area = h("div", { class: "games-area" });
  let current: { el: HTMLElement; leave(): void } | null = null;
  const show = (game: Game) => {
    current?.leave();
    current = GAMES[game]();
    area.replaceChildren(current.el);
    try {
      localStorage.setItem(KEY, game);
    } catch {
      // Not remembered.
    }
  };
  const tabs = segmented<Game>(
    [["chess", t("Chess")], ["draughts", t("Draughts")], ["maze", t("Maze")], ["clouds", t("Clouds")], ["stars", t("Constellations")]],
    which,
    show,
  );
  show(which);
  return {
    el: h("div", { class: "games" }, h("div", { class: "games-top" }, tabs, h("span", { class: "spacer" }), ...extra), area),
    leave: () => current?.leave(),
  };
}
