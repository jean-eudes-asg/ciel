// Flow dictation, the voice models and the Notetaker, as the Ciel window and
// the pill see them (src-tauri/src/flow, speech, notes).

import { invoke } from "@tauri-apps/api/core";
import type { AiStatus } from "./brain";
import { t } from "../i18n/i18n";

const IS_TAURI = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!IS_TAURI) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    console.error(`[ciel] ${cmd} failed`, err);
    return null;
  }
}

async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!IS_TAURI) throw new Error("not running inside Ciel");
  return invoke<T>(cmd, args);
}

export type FlowKey = "right_ctrl" | "right_alt" | "right_shift" | "left_ctrl_super";
/** Where dictation shows: in the island (the default), or in the pill on an edge. */
export type Edge = "island" | "bottom" | "left" | "right";
export type Style = "verbatim" | "natural" | "polished";

export interface FlowPrefs {
  enabled: boolean;
  key: FlowKey;
  edge: Edge;
  mic: string;
  keepDays: number;
  cleanup: boolean;
  defaultStyle: Style;
  summaryEngine: "claude" | "off";
  openAtLaunch: boolean;
  /** « OK Ciel »: listen for it and answer with Ciel's brain (Claude or a local model). */
  voice: boolean;
  voiceOutput: VoiceOutput;
  voiceGender: "female" | "male";
  /** Rust moved dictation into the island once (store.rs); kept as it came. */
  movedToIsland?: boolean;
}

export type ModelState =
  | { state: "missing" }
  | { state: "downloading"; fraction: number }
  | { state: "ready" }
  | { state: "failed"; error: string };

export interface ModelsStatus {
  parakeet: ModelState;
  downloadMb: number;
}

export interface FlowStatus {
  prefs: FlowPrefs;
  models: ModelsStatus;
  /** "ok", or why the key can't be watched ("wayland", "no-display"). */
  keys: string;
  hasClaudeKey: boolean;
  /** Ciel's brain for « OK Ciel », the clean-up and the summaries (core/brain.ts). */
  ai: AiStatus;
  mics: { name: string; default: boolean }[];
  platform: "windows" | "linux";
  autostarted: boolean;
  userName?: string | null;
}

export interface FlowEntry {
  id: string;
  date: number;
  text: string;
  app?: string | null;
  words: number;
  seconds: number;
}

export interface Totals {
  words: number;
  seconds: number;
  dictations: number;
  wordsPerMinute: number;
  minutesSaved: number;
}

export interface FlowStats {
  week: Totals;
  month: Totals;
  all: Totals;
  daily: [string, number][];
  apps: Record<string, number>;
  streak: number;
}

export interface Replacement {
  id: string;
  from: string;
  to: string;
}

export interface Snippet {
  id: string;
  trigger: string;
  text: string;
}

export interface AppStyle {
  id: string;
  name: string;
  style: Style;
}

export interface Speaker {
  id: string;
  name: string;
  color: number;
  isMe: boolean;
}

export interface NoteLine {
  id: string;
  speaker: string;
  start: number;
  end: number;
  text: string;
}

export interface Todo {
  id: string;
  text: string;
  owner?: string | null;
  due?: string | null;
  done: boolean;
}

export interface Note {
  id: string;
  title: string;
  titleEdited: boolean;
  start: number;
  end?: number | null;
  app?: string | null;
  status: "recording" | "processing" | "ready" | "failed";
  speakers: Speaker[];
  lines: NoteLine[];
  summary?: string | null;
  keyPoints: string[];
  todos: Todo[];
  error?: string | null;
  startLocal: string;
}

export interface NotesState {
  notes: Note[];
  recording: string | null;
  working: string[];
}

export interface Announcement {
  id: number;
  icon: string;
  tint: string;
  text: string;
  action?: string | null;
  actionId?: string | null;
}

export interface PillState {
  phase: "idle" | "listening" | "thinking" | "locked" | "copied" | "failed" | "pastedAgain";
  edge: Edge;
  /** "wayland" where the desktop holds the shortcut. */
  key: FlowKey | "wayland" | "";
  noteStartedAt: number | null;
  announcement: Announcement | null;
  enabled: boolean;
  /** « OK Ciel » is talking with you. */
  voice: VoiceView | null;
}

export interface VoiceView {
  phase: "listening" | "thinking" | "speaking";
  heard: string;
  answer: string;
}

export type VoiceOutput = "system" | "elevenlabs" | "off";

// ── What the pill, or the island, shows ───────────────────────────────────────

export type PillLook = "resting" | "toolbar" | "note" | "live" | "handsFree" | "message" | "voice";

/** A dictation first, then « OK Ciel », then a call; else at rest, or the toolbar while hovered. */
export function pillLook(s: PillState, hovered = false): PillLook {
  switch (s.phase) {
    case "locked":
      return "handsFree";
    case "listening":
    case "thinking":
      return "live";
    case "copied":
    case "failed":
    case "pastedAgain":
      return "message";
    default:
      if (s.voice) return "voice";
      if (s.noteStartedAt) return "note";
      return hovered ? "toolbar" : "resting";
  }
}

/** The "message" look's line, in the current language. */
export function pillMessage(phase: PillState["phase"]): string | null {
  switch (phase) {
    case "copied":
      return t("Copied · press Ctrl+V to paste");
    case "failed":
      return t("Microphone unavailable");
    case "pastedAgain":
      return t("Last dictation pasted");
    default:
      return null;
  }
}

/**
 * Where dot `i` goes (0…1): the voice while you speak, a slow wave while
 * thinking, a lively one while Ciel speaks (`speaking`: his voice is heard).
 */
export function dotGoal(s: PillState, voice: readonly number[], now: number, i: number, speaking: boolean): number {
  const wave = () => 0.25 + 0.25 * Math.sin(now / 160 - i * 0.7);
  if (s.phase === "thinking") return wave();
  if (s.phase === "listening" || s.phase === "locked") return voice[i] ?? 0;
  switch (s.voice?.phase) {
    case "thinking":
      return wave();
    case "speaking":
      return speaking ? 0.2 + 0.35 * Math.abs(Math.sin(now / 110 + i * 1.3) * Math.sin(now / 270 - i * 0.6)) : 0.05;
    case "listening":
      return 0.08 + 0.08 * Math.sin(now / 400 - i * 0.5);
    default:
      return 0;
  }
}

/** The pill's own window shows (pill.rs `update`): dictation is on or busy,
 *  and it doesn't show in the island. */
export function pillShown(s: PillState): boolean {
  if (s.edge === "island") return false;
  return s.enabled || s.noteStartedAt !== null || s.announcement !== null || s.voice !== null || s.phase !== "idle";
}

/** Dictation shows in the island and has something to show: a look, or a message. */
export function islandFlowActive(s: PillState): boolean {
  return s.edge === "island" && (s.announcement != null || pillLook(s) !== "resting");
}

export const FlowBridge = {
  status: () => call<FlowStatus>("flow_status"),
  setPrefs: (prefs: FlowPrefs) => call<void>("flow_set_prefs", { prefs }),
  history: () => call<FlowEntry[]>("flow_history"),
  deleteEntry: (id: string) => call<void>("flow_delete", { id }),
  clear: (everything: boolean) => call<void>("flow_clear", { everything }),
  stats: () => call<FlowStats>("flow_stats"),
  words: () => call<{ words: string[]; replacements: Replacement[] }>("flow_words"),
  setWords: (words: { words: string[]; replacements: Replacement[] }) => call<void>("flow_set_words", { words }),
  snippets: () => call<Snippet[]>("flow_snippets"),
  setSnippets: (snippets: Snippet[]) => call<void>("flow_set_snippets", { snippets }),
  styles: () => call<AppStyle[]>("flow_styles"),
  setStyles: (styles: AppStyle[]) => call<void>("flow_set_styles", { styles }),
  runningApps: () => call<{ id: string; name: string }[]>("flow_running_apps"),
  copy: (text: string) => call<boolean>("flow_copy", { text }),
  pasteAgain: () => call<void>("flow_paste_again"),
  download: () => call<void>("flow_models_download"),
  removeModels: () => call<void>("flow_models_remove"),

  notes: () => call<NotesState>("notes_state"),
  startNote: () => callOrThrow<string>("notes_start"),
  stopNote: () => call<void>("notes_stop"),
  renameNote: (id: string, title: string) => call<void>("notes_rename", { id, title }),
  renameSpeaker: (id: string, speaker: string, name: string) => call<void>("notes_rename_speaker", { id, speaker, name }),
  toggleTodo: (id: string, todo: string) => call<void>("notes_toggle_todo", { id, todo }),
  deleteNote: (id: string) => call<void>("notes_delete", { id }),
  summarize: (id: string) => call<void>("notes_summarize", { id }),
  retryNote: (id: string) => call<void>("notes_retry", { id }),
  markdown: (id: string) => call<string>("notes_markdown", { id }),
  exportNote: (id: string) => callOrThrow<string>("notes_export", { id }),

  pillState: () => call<PillState>("pill_state"),
  pillHot: (rects: [number, number, number, number][]) => call<void>("pill_hot", { rects }),
  pillHover: (inside: boolean) => call<void>("pill_hover", { inside }),
  pillAction: (action: string, id?: number) => call<void>("pill_action", { action, id: id ?? null }),
  /** The pill has said what « OK Ciel » answered. */
  voiceDone: (id: number) => call<void>("voice_done", { id, text: null }),
  /** speech-dispatcher or espeak-ng, where the webview has no voice. */
  sayNative: (text: string, lang: string | null) => call<boolean>("voice_say_native", { text, lang }),
};

/** One colour per person in a call: you first, then the others. */
export const SPEAKER_COLORS = ["#3B9EFF", "#34D399", "#A78BFA", "#F7B32B", "#F472B6", "#22D3EE"];

export function speakerColor(index: number): string {
  return SPEAKER_COLORS[Math.max(0, index) % SPEAKER_COLORS.length];
}

/** 75 → "1:15", 3725 → "1:02:05". */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const two = (n: number) => String(n).padStart(2, "0");
  if (s >= 3600) return `${Math.floor(s / 3600)}:${two(Math.floor((s % 3600) / 60))}:${two(s % 60)}`;
  return `${Math.floor(s / 60)}:${two(s % 60)}`;
}
