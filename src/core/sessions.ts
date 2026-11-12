// Every agent session Ciel hears from, kept one by one for the agents' list
// (views/sessions.ts). The pills hold one agent each — two Claude Code
// sessions share Claude Code's pill, and the pill shows whichever spoke last.
// Here each session keeps its own line: what it was asked, what it is doing,
// since when, in which project.
//
// Fed by the hook handler (island/hooks.ts) after it has updated the pill.
// Nothing here is saved: the list is what is running now, and a session that
// ends, or says nothing for a few hours, leaves it.

export type SessionPhase = "asking" | "working" | "done" | "error" | "idle";

export interface AgentSession {
  /** Its pill and its session id: unique in the list. */
  key: string;
  pillId: string;
  sessionId: string;
  /** The agent's name: Claude Code, Codex… */
  agent: string;
  color: string;
  /** The project folder's name. */
  project: string;
  cwd: string;
  /** What it was last asked, on one line. Empty until a prompt is seen. */
  title: string;
  /** What it is doing, or the last thing it said. */
  line: string;
  /** "asking" is never kept: it is read from the card that waits (rows()). */
  phase: Exclude<SessionPhase, "asking">;
  /** When the phase last changed, ms since 1970. */
  since: number;
  /** Its last event. */
  seen: number;
}

export interface SessionEvent {
  event: string;
  pillId: string;
  sessionId: string;
  agent: string;
  color: string;
  project: string;
  cwd: string;
  /** UserPromptSubmit: what was asked. */
  prompt?: string;
  /** The pill's latest step once the event is handled. */
  line?: string;
  /** Stop: the agent's last words, on one line. */
  final?: string | null;
  now: number;
}

/** A session as the list shows it. */
export interface SessionRow extends Omit<AgentSession, "phase"> {
  phase: SessionPhase;
}

export interface SessionCounts {
  asking: number;
  working: number;
  done: number;
  error: number;
}

/** A session that has said nothing for this long is gone (its terminal was closed). */
const STALE_MS = 4 * 3600_000;
const MAX_SESSIONS = 12;
const MAX_TITLE = 160;

const WORKING = new Set([
  "UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "SubagentStart", "SubagentStop",
  "PreCompact", "PermissionRequest",
]);
/** Events that may start a row; a stray notification never does. */
const OPENS = new Set([...WORKING, "SessionStart", "Stop", "StopFailure"]);

/** The list's order: who needs you first, then who works, then the rest. */
const RANK: Record<SessionPhase, number> = { asking: 0, working: 1, error: 2, done: 3, idle: 4 };

function oneLine(text: string): string {
  const line = text.replace(/\s+/g, " ").trim();
  return line.length > MAX_TITLE ? `${line.slice(0, MAX_TITLE - 1)}…` : line;
}

/** The phase an event puts its session in, or null when it changes nothing. */
export function phaseAfter(event: string): AgentSession["phase"] | null {
  if (WORKING.has(event)) return "working";
  if (event === "Stop") return "done";
  if (event === "StopFailure") return "error";
  if (event === "SessionStart" || event === "Interrupt") return "idle";
  return null;
}

export class SessionStore {
  list: AgentSession[] = [];

  /** One hook event, once the pill has been updated for it. */
  observe(e: SessionEvent): void {
    const key = `${e.pillId}:${e.sessionId}`;
    const at = this.list.findIndex((s) => s.key === key);
    if (e.event === "SessionEnd") {
      if (at >= 0) this.list.splice(at, 1);
      return;
    }
    let session = at >= 0 ? this.list[at] : null;
    if (!session) {
      if (!OPENS.has(e.event)) return;
      session = {
        key, pillId: e.pillId, sessionId: e.sessionId, agent: e.agent, color: e.color,
        project: e.project, cwd: e.cwd, title: "", line: "", phase: "idle", since: e.now, seen: e.now,
      };
      this.list.push(session);
    }
    session.seen = e.now;
    session.agent = e.agent;
    session.color = e.color;
    if (e.cwd) {
      session.cwd = e.cwd;
      session.project = e.project;
    }
    const phase = phaseAfter(e.event);
    if (phase && phase !== session.phase) {
      session.phase = phase;
      session.since = e.now;
    }
    if (e.event === "UserPromptSubmit") {
      if (e.prompt) session.title = oneLine(e.prompt);
      session.line = "";
      // A new turn: the clock starts again, even from "working".
      session.since = e.now;
    } else if (e.event === "Stop") {
      session.line = e.final ? oneLine(e.final) : "";
    } else if (phase === "working" && e.line) {
      session.line = oneLine(e.line);
    }
    this.prune(e.now);
  }

  /** Stale sessions go; past the cap, the ones heard from longest ago. */
  prune(now: number): void {
    this.list = this.list.filter((s) => now - s.seen < STALE_MS);
    if (this.list.length > MAX_SESSIONS) {
      this.list = [...this.list].sort((a, b) => b.seen - a.seen).slice(0, MAX_SESSIONS);
    }
  }

  /** The sessions that are done or failed leave the list ("Clear"). */
  clearFinished(): void {
    this.list = this.list.filter((s) => s.phase !== "done" && s.phase !== "error");
  }

  /**
   * The list as shown: the session whose card waits is "asking", whatever its
   * last event said; who needs you comes first, then the latest to change.
   */
  rows(waiting: { pillId: string; sessionId: string } | null, now: number): SessionRow[] {
    return this.list
      .filter((s) => now - s.seen < STALE_MS)
      .map((s): SessionRow => ({
        ...s,
        phase: waiting && waiting.pillId === s.pillId && waiting.sessionId === s.sessionId ? "asking" : s.phase,
      }))
      .sort((a, b) => RANK[a.phase] - RANK[b.phase] || b.since - a.since || a.key.localeCompare(b.key));
  }

  /** Swaps the whole list for another, and hands back the one that was there (the demo). */
  swap(next: AgentSession[] = []): AgentSession[] {
    const was = this.list;
    this.list = next;
    return was;
  }
}

export function countSessions(rows: readonly SessionRow[]): SessionCounts {
  const counts: SessionCounts = { asking: 0, working: 0, done: 0, error: 0 };
  for (const row of rows) {
    if (row.phase !== "idle") counts[row.phase] += 1;
  }
  return counts;
}

/**
 * How long ago, in the fewest characters: "now", "45s", "12m", "3h". The list
 * is glanced at, not read; the unit letters are the same in every language
 * the interface has.
 */
export function ageText(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 5) return "";
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h`;
}

export const Sessions = new SessionStore();
