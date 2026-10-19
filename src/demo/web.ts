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
//
// Embedded in a page of the same origin, it posts the island's mood to it
// ({ type: "ciel-demo-mood", mood }), and takes "ciel-demo-play" (a moment,
// now) and "ciel-demo-sound" (on or off) from it: Ciel's website paints its
// sky with his mood and has buttons and a sound switch around the demo.

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
import { startDemo, type DemoSession } from "./session";
import { isScene } from "./timeline";

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
  const session = startDemo(island, {
    app: false,
    loop: params.loop,
    scene: params.scene,
    // Muted until asked, then at the top of the app's slider (on the website
    // they play over a bed of sound); hovering opens the island, as a visitor expects.
    settings: { soundEnabled: false, soundVolume: 0.2, openOnHover: true },
  });

  // Whichever switch turns the sounds on (ours, the page's, the island's own).
  State.subscribe(loadSounds);

  if (window.parent !== window) {
    // On Ciel's website, the page around the iframe paints its sky from his
    // mood, plays a moment when asked, and has the one sound switch.
    tellMood();
    listenToPage(session);
  } else if (!params.scene) {
    // A frozen scene is for a picture: no sound button in it.
    document.body.append(soundToggle());
  }
}

/** The sounds are fetched the first time they are turned on. */
let soundsLoaded = false;
function loadSounds() {
  if (soundsLoaded || !State.settings.soundEnabled) return;
  soundsLoaded = true;
  void Sound.preload().then(() => Sound.resume());
}

function setSound(on: boolean) {
  State.settings.soundEnabled = on;
  Sound.setEnabled(on);
  Sound.resume();
  State.notify();
}

/**
 * Messages from the page this demo is embedded in (same origin only):
 * { type: "ciel-demo-play", scene } plays that moment now;
 * { type: "ciel-demo-sound", on } turns Ciel's sounds on or off.
 */
function listenToPage(session: DemoSession) {
  window.addEventListener("message", (e) => {
    if (e.origin !== location.origin || e.source !== window.parent) return;
    const data = e.data as { type?: unknown; scene?: unknown; on?: unknown } | null;
    if (data?.type === "ciel-demo-play" && isScene(data.scene)) session.player.playFrom(data.scene);
    else if (data?.type === "ciel-demo-sound" && typeof data.on === "boolean") setSound(data.on);
  });
}

/** The island's mood, for the page this demo is embedded in (same origin only). */
function tellMood() {
  let told: string | null = null;
  const tell = () => {
    const mood = State.effectiveState;
    if (mood === told) return;
    told = mood;
    window.parent.postMessage({ type: "ciel-demo-mood", mood }, location.origin);
  };
  State.subscribe(tell);
  tell();
}

/** The sounds, off by default. */
function soundToggle(): HTMLElement {
  const button = h("button", { class: "demo-sound", title: tl("Sound"), "aria-pressed": "false" });
  const draw = () => {
    const on = State.settings.soundEnabled;
    button.setAttribute("aria-pressed", String(on));
    button.replaceChildren(svg(on ? ICONS.speakerOn : ICONS.speakerOff, 14));
  };
  let shown: boolean | null = null;
  const sync = () => {
    // The island's own mute button flips the same setting.
    const on = State.settings.soundEnabled;
    if (on !== shown) {
      shown = on;
      draw();
    }
  };
  button.addEventListener("click", () => setSound(!State.settings.soundEnabled));
  State.subscribe(sync);
  sync();
  return button;
}

main();
