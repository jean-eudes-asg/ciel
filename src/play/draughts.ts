// International draughts for the Games page: 10x10, the 50 dark squares,
// 20 men each, White first. Men step forward diagonally and take both ways;
// kings fly, any distance along a diagonal, and take a piece any distance
// away, landing on any free square beyond it. Taking is compulsory and the
// most pieces must be taken; the pieces taken stay on the board until the
// move ends (a piece is never jumped twice, and they block). A man is crowned
// only when his move ends on the far row. You lose when you can't move; 25
// moves each of kings only without a capture, or a position seen three
// times, is a draw. And the rivals' player, an alpha-beta search like the
// chess one, run in the same worker.
//
// Squares are 0..99, row by row from the top, where Black starts; standard
// numbers (1..50) only for the move list.

export type Side = "w" | "b";
/** "w"/"b" a man, "W"/"B" a king, "" empty. */
type Piece = string;

export interface Move {
  /** Every square the piece stands on, first to last. */
  path: number[];
  /** The squares of the pieces taken. */
  taken: number[];
  /** A man crowned at the end of it. */
  crowns: boolean;
}

export const SIZE = 10;
const DIRS: [number, number][] = [[1, 1], [1, -1], [-1, 1], [-1, -1]];
const sideOf = (p: Piece): Side => (p.toLowerCase() === "w" ? "w" : "b");
const row = (i: number) => Math.floor(i / SIZE);
const col = (i: number) => i % SIZE;
const on = (r: number, c: number) => r >= 0 && r < SIZE && c >= 0 && c < SIZE;
const king = (p: Piece) => p === "W" || p === "B";

/** The standard number of a dark square, 1 to 50. */
export const number = (i: number) => row(i) * 5 + Math.floor(col(i) / 2) + 1;

/** "32-28", or "28x19" for a capture: from and to, as draughts players write. */
export function notation(m: Move): string {
  return `${number(m.path[0])}${m.taken.length ? "x" : "-"}${number(m.path[m.path.length - 1])}`;
}

export class Draughts {
  board: Piece[] = new Array(SIZE * SIZE).fill("");
  turn: Side = "w";
  /** Plies of kings only, without a capture: 50 of them is a draw. */
  quiet = 0;
  history: { move: Move; was: Piece[]; quiet: number }[] = [];
  seen: string[] = [];

  constructor() {
    for (let i = 0; i < SIZE * SIZE; i++) {
      if ((row(i) + col(i)) % 2 === 0) continue;
      if (row(i) < 4) this.board[i] = "b";
      if (row(i) > 5) this.board[i] = "w";
    }
    this.seen = [this.key()];
  }

  key(): string {
    return this.turn + this.board.map((p) => p || ".").join("");
  }

  private lastRow(p: Piece, i: number) {
    return (p === "w" && row(i) === 0) || (p === "b" && row(i) === SIZE - 1);
  }

  /** Every capture sequence for the piece on `from`, each followed to its end. */
  private captures(from: number): Move[] {
    const out: Move[] = [];
    const p = this.board[from];
    const mine = sideOf(p);
    const flies = king(p);
    // The square the piece left is free while it travels.
    const free = (i: number) => i === from || !this.board[i];
    const walk = (at: number, path: number[], taken: number[]) => {
      let further = false;
      for (const [dr, dc] of DIRS) {
        let r = row(at) + dr;
        let c = col(at) + dc;
        // A king glides over free squares to the piece it takes.
        if (flies) {
          while (on(r, c) && free(r * SIZE + c)) {
            r += dr;
            c += dc;
          }
        }
        if (!on(r, c)) continue;
        const over = r * SIZE + c;
        if (free(over) || sideOf(this.board[over]) === mine || taken.includes(over)) continue;
        // Then lands just beyond it, or (a king) anywhere further while free.
        let lr = r + dr;
        let lc = c + dc;
        while (on(lr, lc) && free(lr * SIZE + lc)) {
          const land = lr * SIZE + lc;
          further = true;
          walk(land, [...path, land], [...taken, over]);
          if (!flies) break;
          lr += dr;
          lc += dc;
        }
      }
      if (!further && taken.length) out.push({ path, taken, crowns: !flies && this.lastRow(p, at) });
    };
    walk(from, [from], []);
    return out;
  }

  moves(): Move[] {
    let captures: Move[] = [];
    const steps: Move[] = [];
    for (let i = 0; i < SIZE * SIZE; i++) {
      const p = this.board[i];
      if (!p || sideOf(p) !== this.turn) continue;
      captures.push(...this.captures(i));
      if (captures.length) continue;
      const forward = p === "w" ? -1 : 1;
      for (const [dr, dc] of DIRS) {
        if (!king(p) && dr !== forward) continue;
        let r = row(i) + dr;
        let c = col(i) + dc;
        while (on(r, c) && !this.board[r * SIZE + c]) {
          const to = r * SIZE + c;
          steps.push({ path: [i, to], taken: [], crowns: !king(p) && this.lastRow(p, to) });
          if (!king(p)) break;
          r += dr;
          c += dc;
        }
      }
    }
    if (!captures.length) return steps;
    // The most pieces must be taken; two ways to take the same ones are one move.
    const most = Math.max(...captures.map((m) => m.taken.length));
    captures = captures.filter((m) => m.taken.length === most);
    const seen = new Set<string>();
    return captures.filter((m) => {
      const k = `${m.path[0]}>${m.path[m.path.length - 1]}:${[...m.taken].sort((a, b) => a - b).join()}`;
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
  }

  make(m: Move) {
    const from = m.path[0];
    const to = m.path[m.path.length - 1];
    const p = this.board[from];
    this.history.push({ move: m, was: m.taken.map((i) => this.board[i]).concat(p), quiet: this.quiet });
    this.board[from] = "";
    for (const i of m.taken) this.board[i] = "";
    this.board[to] = m.crowns ? p.toUpperCase() : p;
    this.quiet = m.taken.length || !king(p) ? 0 : this.quiet + 1;
    this.turn = this.turn === "w" ? "b" : "w";
  }

  unmake() {
    const u = this.history.pop();
    if (!u) return;
    const m = u.move;
    this.turn = this.turn === "w" ? "b" : "w";
    this.board[m.path[m.path.length - 1]] = "";
    m.taken.forEach((i, k) => (this.board[i] = u.was[k]));
    this.board[m.path[0]] = u.was[u.was.length - 1];
    this.quiet = u.quiet;
  }

  play(m: Move) {
    this.make(m);
    this.seen.push(this.key());
  }

  takeBack() {
    if (!this.history.length) return;
    this.unmake();
    this.seen.pop();
  }

  /** Who won, or a draw, or null while it goes on. */
  result(): { winner: Side | null } | null {
    if (this.moves().length === 0) return { winner: this.turn === "w" ? "b" : "w" };
    if (this.quiet >= 50) return { winner: null };
    const now = this.key();
    if (this.seen.filter((k) => k === now).length >= 3) return { winner: null };
    return null;
  }
}

// ── The rivals' player ────────────────────────────────────────────────────────

const MAN = 100;
/** A flying king is worth about three men. */
const KING = 300;
const WIN = 100_000;

/** The position for White, in hundredths of a man. */
export function evaluate(d: Draughts): number {
  let score = 0;
  for (let i = 0; i < SIZE * SIZE; i++) {
    const p = d.board[i];
    if (!p) continue;
    const sign = sideOf(p) === "w" ? 1 : -1;
    if (king(p)) {
      score += sign * KING;
      continue;
    }
    const ahead = p === "w" ? SIZE - 1 - row(i) : row(i);
    const centre = col(i) >= 3 && col(i) <= 6 ? 5 : 0;
    // Men at home guard the crowning row.
    const guard = ahead === 0 ? 6 : 0;
    score += sign * (MAN + ahead * 3 + centre + guard);
  }
  return score;
}

export interface Level {
  depth: number;
  ms: number;
  slack: number;
}

interface Search {
  nodes: number;
  stop: number;
  out: boolean;
}

function negamax(d: Draughts, depth: number, alpha: number, beta: number, ply: number, s: Search): number {
  if ((++s.nodes & 1023) === 0 && Date.now() > s.stop) s.out = true;
  if (s.out) return 0;
  const moves = d.moves();
  if (!moves.length) return -WIN + ply;
  // Captures are forced: the search goes on through them, past the horizon.
  if (depth <= 0 && !moves[0].taken.length) return evaluate(d) * (d.turn === "w" ? 1 : -1);
  if (d.quiet >= 50) return 0;
  for (const m of moves) {
    d.make(m);
    const score = -negamax(d, depth - 1, -beta, -alpha, ply + 1, s);
    d.unmake();
    if (score >= beta) return score;
    if (score > alpha) alpha = score;
  }
  return alpha;
}

export interface Choice {
  move: Move;
  score: number;
}

export function rank(d: Draughts, level: Level): Choice[] {
  const s: Search = { nodes: 0, stop: Date.now() + level.ms, out: false };
  let ranked: Choice[] = d.moves().map((move) => ({ move, score: 0 }));
  for (let depth = 1; depth <= level.depth && ranked.length > 1; depth++) {
    const next: Choice[] = [];
    let floor = -WIN;
    for (const { move } of ranked) {
      d.make(move);
      const score = -negamax(d, depth - 1, -WIN, -floor, 1, s);
      d.unmake();
      if (s.out) break;
      next.push({ move, score });
      floor = Math.max(floor, score - level.slack - 1);
    }
    if (s.out) break;
    ranked = next.sort((a, b) => b.score - a.score);
  }
  return ranked;
}

export function choose(d: Draughts, level: Level, random = Math.random): Choice | null {
  const ranked = rank(d, level);
  if (!ranked.length) return null;
  const best = ranked[0].score;
  if (Math.abs(best) > WIN / 2) return ranked[0];
  const near = ranked.filter((r) => r.score >= best - level.slack);
  return near[Math.floor(random() * near.length)];
}
