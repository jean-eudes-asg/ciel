// The games' own window in a plain browser (`npx vite`,
// /dev/games-window-preview.html), at whatever size the browser is.

import "../src/style.css";
import "../src/hub/hub.css";
import { games } from "../src/play/games";

document.body.classList.add("hub-window", "games-window");
document.getElementById("games")!.append(games().el);
