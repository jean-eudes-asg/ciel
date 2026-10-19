// The sky stage in a worker: the page hands over its canvas
// (transferControlToOffscreen) and then only sends what to show. Building a
// scene here never makes the page stutter.

import { SkyStage, type StageEvent, type StageMessage } from "./sky-stage";

interface WorkerScope {
  onmessage: ((e: MessageEvent<StageMessage>) => void) | null;
  postMessage(msg: StageEvent): void;
}

const scope = self as unknown as WorkerScope;
let stage: SkyStage | null = null;

scope.onmessage = (e) => {
  const msg = e.data;
  if (msg.type === "init") {
    if (!msg.canvas) return;
    stage = new SkyStage(msg.canvas, msg, (event) => scope.postMessage(event));
    stage.handle({ type: "run", running: true });
    return;
  }
  stage?.handle(msg);
};
