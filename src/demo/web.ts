// demo.html: the island in demo mode in a plain browser page — for anyone
// trying Ciel, the README's screenshots, and the live demo on Ciel's website
// (an iframe of about 900×420). No Tauri: every bridge call is answered by the
// demo's gate, nothing goes out on the network, and the sounds are only
// fetched once someone turns them on.
//
// Query parameters, all optional:
//   ?scene=diff|session|approval|question|finished|finance|chat  freeze on one moment
//   ?loop=0          play once, then stay asleep
//   ?lang=fr         one of the ten interface languages (else the browser's)
//   ?bg=transparent  no backdrop

import "../style.css";
import "./demo.css";
import { Island } from "../island/island";
import { registerFinanceHandlers } from "../island/finance";
import { State } from "../core/state";
import { Sound } from "../core/sound";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import { resolveLanguage, setLanguage, systemLanguages, tl } from "../i18n/i18n";
import { readParams } from "./params";
import { startDemo } from "./session";

function main() {
  const root = document.getElementById("root");
  if (!root) return;
  const params = readParams(location.search);
  document.documentElement.classList.add("ciel-demo");
  if (params.transparent) document.documentElement.dataset.bg = "transparent";
  setLanguage(resolveLanguage(params.lang, systemLanguages()));

  const island = new Island(root);
  // The Finance pill's label and its card's reading, through the gate.
  registerFinanceHandlers();
  startDemo(island, {
    app: false,
    loop: params.loop,
    scene: params.scene,
    // Muted until asked; hovering opens the island, as a visitor expects.
    settings: { soundEnabled: false, openOnHover: true },
  });

  // A frozen scene is for a picture: no sound button in it.
  if (!params.scene) document.body.append(soundToggle());
}

/** The sounds, off by default. They are fetched the first time they are turned on. */
function soundToggle(): HTMLElement {
  const button = h("button", { class: "demo-sound", title: tl("Sound"), "aria-pressed": "false" });
  const draw = () => {
    const on = State.settings.soundEnabled;
    button.setAttribute("aria-pressed", String(on));
    button.replaceChildren(svg(on ? ICONS.speakerOn : ICONS.speakerOff, 14));
  };
  let loaded = false;
  let shown: boolean | null = null;
  const sync = () => {
    // The island's own mute button flips the same setting.
    const on = State.settings.soundEnabled;
    if (on && !loaded) {
      loaded = true;
      void Sound.preload().then(() => Sound.resume());
    }
    if (on !== shown) {
      shown = on;
      draw();
    }
  };
  button.addEventListener("click", () => {
    State.settings.soundEnabled = !State.settings.soundEnabled;
    Sound.setEnabled(State.settings.soundEnabled);
    Sound.resume();
    State.notify();
  });
  State.subscribe(sync);
  sync();
  return button;
}

main();
