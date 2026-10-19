// demo.html's query parameters (see web.ts).

import { isScene, type SceneName } from "./timeline";

export interface WebParams {
  /** Freeze on this moment. */
  scene: SceneName | null;
  /** False with `?loop=0`: play once. */
  loop: boolean;
  /** `?lang=fr`; "" follows the browser. */
  lang: string;
  /** `?bg=transparent`: no backdrop. */
  transparent: boolean;
}

export function readParams(search: string): WebParams {
  const q = new URLSearchParams(search);
  const scene = q.get("scene");
  return {
    scene: isScene(scene) ? scene : null,
    loop: q.get("loop") !== "0",
    lang: q.get("lang") ?? "",
    transparent: q.get("bg") === "transparent",
  };
}
