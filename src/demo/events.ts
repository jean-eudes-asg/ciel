// What the settings window and the island say to each other about the demo.
// Kept apart from the rest of src/demo so the settings window imports nothing
// of the island's.

export const DEMO_EVENTS = {
  /** Settings → island: "start", "stop", or "status" (answer with DEMO_EVENTS.state). */
  control: "demo-control",
  /** Island → settings: a DemoStatus. */
  state: "demo-state",
} as const;

export type DemoControl = "start" | "stop" | "status";

export interface DemoStatus {
  running: boolean;
  /** Why "start" did nothing: a real request is waiting for an answer. */
  refused?: "pending";
}

/** The window labels (src-tauri: the island, and the hub labelled "settings"). */
export const ISLAND_WINDOW = "island";
export const SETTINGS_WINDOW = "settings";
