// The brain drew something (src-tauri/src/canvas.rs): the island opens on it,
// small; the Canvas page of the Ciel window shows it big.

import { onEvent } from "../core/bridge";
import { asMeta } from "../core/canvas";
import { State } from "../core/state";
import type { Island } from "./island";

export function registerCanvas(island: Island) {
  void onEvent<unknown>("canvas", (payload) => {
    const drawing = asMeta(payload);
    if (!drawing || State.paused) return;
    State.canvas = drawing;
    island.alert("canvas");
    State.notify();
  });
}
