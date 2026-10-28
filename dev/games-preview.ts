// The Games page of the Ciel window in a plain browser (`npx vite`,
// /dev/games-preview.html): chess and the maze, Ciel's brain silent.

import "../src/style.css";
import "../src/hub/hub.css";
import { hero } from "../src/hub/hero";
import { gamesPage } from "../src/hub/pages/games";
import type { HubContext } from "../src/hub/shell";

const ctx = { hero: (page: string, title: string, sub: string | null, actions: HTMLElement[] = [], compact = false) => hero(page, title, sub, actions, compact) } as unknown as HubContext;
gamesPage(document.getElementById("page")!, ctx);
