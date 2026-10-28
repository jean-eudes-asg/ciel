// Chess for the Games page: the rules (every legal move, check, mate, draws,
// SAN for the move list) and Ciel's own player, a small alpha-beta search
// that runs in a worker (chess-worker.ts). Ciel's brain only talks about the
// game; the moves are always this file's, so they are always legal.
//
// The board is a 10x12 mailbox: index 21 is a8, 98 is h1, and the squares
// around the board hold OFF so a slide stops by itself.

export type Color = "w" | "b";
/** "P N B R Q K" for White, lower case for Black, "" for an empty square. */
type Square = string;
const OFF = "x";

export interface Move {
  from: number;
  to: number;
  piece: string;
  captured: string;
  promo: string;
  /** "e" en passant, "k"/"q" castling, "d" a pawn's two steps. */
  flag: string;
}

interface Undo {
  move: Move;
  castling: number;
  ep: number;
  half: number;
}

const N = -10, S = 10, E = 1, W = -1;
const KNIGHT = [N + N + E, N + N + W, S + S + E, S + S + W, E + E + N, E + E + S, W + W + N, W + W + S];
const DIAG = [N + E, N + W, S + E, S + W];
const LINE = [N, S, E, W];
const KING = [...DIAG, ...LINE];
const CASTLE = { K: 1, Q: 2, k: 4, q: 8 } as const;

export const sq = (name: string) => 21 + (name.charCodeAt(0) - 97) + (8 - Number(name[1])) * 10;
export const name = (i: number) => String.fromCharCode(97 + ((i - 21) % 10)) + String(8 - Math.floor((i - 21) / 10));
const colorOf = (p: Square): Color => (p === p.toUpperCase() ? "w" : "b");
const isPiece = (p: Square) => p !== "" && p !== OFF;

export const START = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

export class Chess {
  board: Square[] = [];
  turn: Color = "w";
  castling = 0;
  ep = -1;
  half = 0;
  full = 1;
  history: Undo[] = [];
  /** Position keys since the start, for threefold repetition. */
  seen: string[] = [];

  constructor(fen = START) {
    this.load(fen);
  }

  load(fen: string) {
    const [placement, turn, castling, ep, half, full] = fen.trim().split(/\s+/);
    this.board = new Array(120).fill(OFF);
    placement.split("/").forEach((row, r) => {
      let f = 0;
      for (const c of row) {
        if (/\d/.test(c)) {
          for (let k = 0; k < Number(c); k++) this.board[21 + r * 10 + f++] = "";
        } else {
          this.board[21 + r * 10 + f++] = c;
        }
      }
    });
    this.turn = turn === "b" ? "b" : "w";
    this.castling = [...(castling ?? "")].reduce((m, c) => m | (CASTLE[c as keyof typeof CASTLE] ?? 0), 0);
    this.ep = ep && ep !== "-" ? sq(ep) : -1;
    this.half = Number(half) || 0;
    this.full = Number(full) || 1;
    this.history = [];
    this.seen = [this.key()];
  }

  fen(): string {
    const rows: string[] = [];
    for (let r = 0; r < 8; r++) {
      let row = "";
      let empty = 0;
      for (let f = 0; f < 8; f++) {
        const p = this.board[21 + r * 10 + f];
        if (p === "") {
          empty++;
        } else {
          row += (empty || "") + p;
          empty = 0;
        }
      }
      rows.push(row + (empty || ""));
    }
    const c = Object.entries(CASTLE).filter(([, bit]) => this.castling & bit).map(([k]) => k).join("") || "-";
    return `${rows.join("/")} ${this.turn} ${c} ${this.ep < 0 ? "-" : name(this.ep)} ${this.half} ${this.full}`;
  }

  /** What makes two positions the same for repetition. */
  key(): string {
    return this.fen().split(" ").slice(0, 4).join(" ");
  }

  at(square: string): Square {
    return this.board[sq(square)];
  }

  king(color: Color): number {
    return this.board.indexOf(color === "w" ? "K" : "k");
  }

  /** Is `i` attacked by `by`? */
  attacked(i: number, by: Color): boolean {
    const b = this.board;
    const own = (p: string) => (by === "w" ? p : p.toLowerCase());
    const pawnFrom = by === "w" ? S : N;
    if (b[i + pawnFrom + E] === own("P") || b[i + pawnFrom + W] === own("P")) return true;
    for (const d of KNIGHT) if (b[i + d] === own("N")) return true;
    for (const d of KING) if (b[i + d] === own("K")) return true;
    for (const [dirs, a, c] of [[DIAG, "B", "Q"], [LINE, "R", "Q"]] as const) {
      for (const d of dirs) {
        let j = i + d;
        while (b[j] === "") j += d;
        if (b[j] === own(a) || b[j] === own(c)) return true;
      }
    }
    return false;
  }

  inCheck(color = this.turn): boolean {
    return this.attacked(this.king(color), color === "w" ? "b" : "w");
  }

  /** Every move by the side to move, legal or not (its king may be left in check). */
  pseudo(capturesOnly = false): Move[] {
    const out: Move[] = [];
    const b = this.board;
    const me = this.turn;
    const add = (from: number, to: number, flag = "", promo = "") =>
      out.push({ from, to, piece: b[from], captured: flag === "e" ? (me === "w" ? "p" : "P") : b[to], promo, flag });
    for (let i = 21; i <= 98; i++) {
      const p = b[i];
      if (!isPiece(p) || colorOf(p) !== me) continue;
      const kind = p.toUpperCase();
      if (kind === "P") {
        const up = me === "w" ? N : S;
        const last = me === "w" ? 2 : 9; // the row of the promotion, in tens
        const homeRow = me === "w" ? 8 : 3;
        const promote = (to: number) => Math.floor(to / 10) === last;
        const push = (from: number, to: number) => {
          if (promote(to)) for (const q of "QRBN") add(from, to, "", me === "w" ? q : q.toLowerCase());
          else add(from, to);
        };
        if (b[i + up] === "" && (!capturesOnly || promote(i + up))) {
          push(i, i + up);
          if (!capturesOnly && Math.floor(i / 10) === homeRow && b[i + 2 * up] === "") add(i, i + 2 * up, "d");
        }
        for (const side of [E, W]) {
          const to = i + up + side;
          if (isPiece(b[to]) && colorOf(b[to]) !== me) push(i, to);
          else if (to === this.ep) add(i, to, "e");
        }
        continue;
      }
      const slides = kind === "B" || kind === "R" || kind === "Q";
      const dirs = kind === "N" ? KNIGHT : kind === "B" ? DIAG : kind === "R" ? LINE : KING;
      for (const d of dirs) {
        let j = i + d;
        while (b[j] !== OFF) {
          if (b[j] === "") {
            if (!capturesOnly) add(i, j);
          } else {
            if (colorOf(b[j]) !== me) add(i, j);
            break;
          }
          if (!slides) break;
          j += d;
        }
      }
      if (kind === "K" && !capturesOnly) {
        const them: Color = me === "w" ? "b" : "w";
        const [k, q] = me === "w" ? [CASTLE.K, CASTLE.Q] : [CASTLE.k, CASTLE.q];
        if (this.castling & k && b[i + 1] === "" && b[i + 2] === "" && !this.attacked(i, them) && !this.attacked(i + 1, them) && !this.attacked(i + 2, them)) {
          add(i, i + 2, "k");
        }
        if (this.castling & q && b[i - 1] === "" && b[i - 2] === "" && b[i - 3] === "" && !this.attacked(i, them) && !this.attacked(i - 1, them) && !this.attacked(i - 2, them)) {
          add(i, i - 2, "q");
        }
      }
    }
    return out;
  }

  moves(): Move[] {
    return this.pseudo().filter((m) => this.legal(m));
  }

  legal(m: Move): boolean {
    this.make(m);
    const ok = !this.inCheck(this.turn === "w" ? "b" : "w");
    this.unmake();
    return ok;
  }

  make(m: Move) {
    const b = this.board;
    this.history.push({ move: m, castling: this.castling, ep: this.ep, half: this.half });
    b[m.to] = m.promo || m.piece;
    b[m.from] = "";
    if (m.flag === "e") b[m.to + (this.turn === "w" ? S : N)] = "";
    if (m.flag === "k") [b[m.to - 1], b[m.to + 1]] = [b[m.to + 1], ""];
    if (m.flag === "q") [b[m.to + 1], b[m.to - 2]] = [b[m.to - 2], ""];
    for (const i of [m.from, m.to]) {
      if (i === 95) this.castling &= ~(CASTLE.K | CASTLE.Q);
      if (i === 25) this.castling &= ~(CASTLE.k | CASTLE.q);
      if (i === 98) this.castling &= ~CASTLE.K;
      if (i === 91) this.castling &= ~CASTLE.Q;
      if (i === 28) this.castling &= ~CASTLE.k;
      if (i === 21) this.castling &= ~CASTLE.q;
    }
    this.ep = m.flag === "d" ? (m.from + m.to) / 2 : -1;
    this.half = m.piece.toUpperCase() === "P" || m.captured ? 0 : this.half + 1;
    if (this.turn === "b") this.full++;
    this.turn = this.turn === "w" ? "b" : "w";
  }

  unmake() {
    const u = this.history.pop();
    if (!u) return;
    const { move: m } = u;
    const b = this.board;
    this.turn = this.turn === "w" ? "b" : "w";
    if (this.turn === "b") this.full--;
    b[m.from] = m.piece;
    b[m.to] = m.flag === "e" ? "" : m.captured;
    if (m.flag === "e") b[m.to + (this.turn === "w" ? S : N)] = m.captured;
    if (m.flag === "k") [b[m.to + 1], b[m.to - 1]] = [b[m.to - 1], ""];
    if (m.flag === "q") [b[m.to - 2], b[m.to + 1]] = [b[m.to + 1], ""];
    this.castling = u.castling;
    this.ep = u.ep;
    this.half = u.half;
  }

  /** Plays a move in the game (keeps the repetition count; the search uses make). */
  play(m: Move) {
    this.make(m);
    this.seen.push(this.key());
  }

  takeBack() {
    if (!this.history.length) return;
    this.unmake();
    this.seen.pop();
  }

  san(m: Move): string {
    let s: string;
    const kind = m.piece.toUpperCase();
    if (m.flag === "k") s = "O-O";
    else if (m.flag === "q") s = "O-O-O";
    else if (kind === "P") {
      s = (m.captured ? name(m.from)[0] + "x" : "") + name(m.to) + (m.promo ? "=" + m.promo.toUpperCase() : "");
    } else {
      const rivals = this.moves().filter((o) => o.piece === m.piece && o.to === m.to && o.from !== m.from);
      let which = "";
      if (rivals.length) {
        const f = name(m.from)[0];
        const r = name(m.from)[1];
        if (!rivals.some((o) => name(o.from)[0] === f)) which = f;
        else if (!rivals.some((o) => name(o.from)[1] === r)) which = r;
        else which = f + r;
      }
      s = kind + which + (m.captured ? "x" : "") + name(m.to);
    }
    this.make(m);
    const check = this.inCheck();
    const mate = check && this.moves().length === 0;
    this.unmake();
    return s + (mate ? "#" : check ? "+" : "");
  }

  /** The SAN of every move played, from the start. */
  line(): string[] {
    const played = this.history.map((u) => u.move);
    const seen = this.seen;
    while (this.history.length) this.unmake();
    const out = played.map((m) => {
      const s = this.san(m);
      this.make(m);
      return s;
    });
    this.seen = seen;
    return out;
  }

  /** Why the game is over, or null while it goes on. */
  result(): { winner: Color | null; why: "mate" | "stalemate" | "fifty" | "repetition" | "material" } | null {
    if (this.moves().length === 0) {
      return this.inCheck() ? { winner: this.turn === "w" ? "b" : "w", why: "mate" } : { winner: null, why: "stalemate" };
    }
    if (this.half >= 100) return { winner: null, why: "fifty" };
    const now = this.key();
    if (this.seen.filter((k) => k === now).length >= 3) return { winner: null, why: "repetition" };
    const rest = this.board.filter((p) => isPiece(p) && p.toUpperCase() !== "K");
    if (rest.length === 0 || (rest.length === 1 && "NBnb".includes(rest[0]))) return { winner: null, why: "material" };
    return null;
  }
}

// ── Ciel's player ─────────────────────────────────────────────────────────────

const VALUE: Record<string, number> = { P: 100, N: 320, B: 330, R: 500, Q: 900, K: 0 };

// Where each piece likes to stand, from White's side, a8 first.
// (The "simplified evaluation function" tables every beginner engine starts with.)
const PST: Record<string, number[]> = {
  P: [0,0,0,0,0,0,0,0, 50,50,50,50,50,50,50,50, 10,10,20,30,30,20,10,10, 5,5,10,25,25,10,5,5, 0,0,0,20,20,0,0,0, 5,-5,-10,0,0,-10,-5,5, 5,10,10,-20,-20,10,10,5, 0,0,0,0,0,0,0,0],
  N: [-50,-40,-30,-30,-30,-30,-40,-50, -40,-20,0,0,0,0,-20,-40, -30,0,10,15,15,10,0,-30, -30,5,15,20,20,15,5,-30, -30,0,15,20,20,15,0,-30, -30,5,10,15,15,10,5,-30, -40,-20,0,5,5,0,-20,-40, -50,-40,-30,-30,-30,-30,-40,-50],
  B: [-20,-10,-10,-10,-10,-10,-10,-20, -10,0,0,0,0,0,0,-10, -10,0,5,10,10,5,0,-10, -10,5,5,10,10,5,5,-10, -10,0,10,10,10,10,0,-10, -10,10,10,10,10,10,10,-10, -10,5,0,0,0,0,5,-10, -20,-10,-10,-10,-10,-10,-10,-20],
  R: [0,0,0,0,0,0,0,0, 5,10,10,10,10,10,10,5, -5,0,0,0,0,0,0,-5, -5,0,0,0,0,0,0,-5, -5,0,0,0,0,0,0,-5, -5,0,0,0,0,0,0,-5, -5,0,0,0,0,0,0,-5, 0,0,0,5,5,0,0,0],
  Q: [-20,-10,-10,-5,-5,-10,-10,-20, -10,0,0,0,0,0,0,-10, -10,0,5,5,5,5,0,-10, -5,0,5,5,5,5,0,-5, 0,0,5,5,5,5,0,-5, -10,5,5,5,5,5,0,-10, -10,0,5,0,0,0,0,-10, -20,-10,-10,-5,-5,-10,-10,-20],
  K: [-30,-40,-40,-50,-50,-40,-40,-30, -30,-40,-40,-50,-50,-40,-40,-30, -30,-40,-40,-50,-50,-40,-40,-30, -30,-40,-40,-50,-50,-40,-40,-30, -20,-30,-30,-40,-40,-30,-30,-20, -10,-20,-20,-20,-20,-20,-20,-10, 20,20,0,0,0,0,20,20, 20,30,10,0,0,10,30,20],
  // The king walks to the middle once the queens are off.
  KE: [-50,-40,-30,-20,-20,-30,-40,-50, -30,-20,-10,0,0,-10,-20,-30, -30,-10,20,30,30,20,-10,-30, -30,-10,30,40,40,30,-10,-30, -30,-10,30,40,40,30,-10,-30, -30,-10,20,30,30,20,-10,-30, -30,-30,0,0,0,0,-30,-30, -50,-30,-30,-30,-30,-30,-30,-50],
};

const MATE = 100_000;

/** The position for White, in hundredths of a pawn. */
export function evaluate(c: Chess): number {
  const b = c.board;
  const endgame = !b.includes("Q") && !b.includes("q");
  let score = 0;
  for (let i = 21; i <= 98; i++) {
    const p = b[i];
    if (!isPiece(p)) continue;
    const kind = p.toUpperCase();
    const k = i - 21 - 2 * Math.floor((i - 21) / 10); // 0..63, a8 first
    const table = PST[kind === "K" && endgame ? "KE" : kind];
    if (p === kind) score += VALUE[kind] + table[k];
    else score -= VALUE[kind] + table[(7 - Math.floor(k / 8)) * 8 + (k % 8)];
  }
  return score;
}

const side = (c: Chess) => (c.turn === "w" ? 1 : -1);

/** Captures first, the biggest prize by the smallest piece. */
function order(moves: Move[], first?: Move): Move[] {
  const worth = (m: Move) =>
    (first && m.from === first.from && m.to === first.to ? 1e6 : 0) +
    (m.captured ? 10 * VALUE[m.captured.toUpperCase()] - VALUE[m.piece.toUpperCase()] + 1000 : 0) +
    (m.promo ? VALUE[m.promo.toUpperCase()] : 0);
  return moves.sort((a, b) => worth(b) - worth(a));
}

interface Search {
  nodes: number;
  stop: number;
  out: boolean;
}

function quiet(c: Chess, alpha: number, beta: number, s: Search): number {
  const stand = evaluate(c) * side(c);
  if (stand >= beta) return stand;
  if (stand > alpha) alpha = stand;
  for (const m of order(c.pseudo(true))) {
    c.make(m);
    if (c.inCheck(c.turn === "w" ? "b" : "w")) {
      c.unmake();
      continue;
    }
    const score = -quiet(c, -beta, -alpha, s);
    c.unmake();
    if (score >= beta) return score;
    if (score > alpha) alpha = score;
  }
  return alpha;
}

function negamax(c: Chess, depth: number, alpha: number, beta: number, ply: number, s: Search): number {
  if ((++s.nodes & 1023) === 0 && Date.now() > s.stop) s.out = true;
  if (s.out) return 0;
  // In check at the horizon: one more look, so a mate there is seen (a few times at most).
  if (depth <= 0 && (depth < -3 || !c.inCheck())) return quiet(c, alpha, beta, s);
  if (c.half >= 100) return 0;
  let legal = 0;
  for (const m of order(c.pseudo())) {
    c.make(m);
    if (c.inCheck(c.turn === "w" ? "b" : "w")) {
      c.unmake();
      continue;
    }
    legal++;
    const score = -negamax(c, depth - 1, -beta, -alpha, ply + 1, s);
    c.unmake();
    if (score >= beta) return score;
    if (score > alpha) alpha = score;
  }
  if (legal === 0) return c.inCheck() ? -MATE + ply : 0;
  return alpha;
}

export interface Level {
  depth: number;
  ms: number;
  /** Up to how many hundredths of a pawn Ciel may give away to play like a person. */
  slack: number;
}

export interface Choice {
  move: Move;
  /** For the side that moves, in hundredths of a pawn. */
  score: number;
}

/** Every root move scored at the deepest depth finished in time, best first. */
export function rank(c: Chess, level: Level): Choice[] {
  const s: Search = { nodes: 0, stop: Date.now() + level.ms, out: false };
  let ranked: Choice[] = c.moves().map((move) => ({ move, score: 0 }));
  for (let depth = 1; depth <= level.depth && ranked.length > 1; depth++) {
    const next: Choice[] = [];
    let floor = -MATE;
    for (const { move } of ranked) {
      c.make(move);
      // Only the moves within the level's slack of the best need a true score.
      const score = -negamax(c, depth - 1, -MATE, -floor, 1, s);
      c.unmake();
      if (s.out) break;
      next.push({ move, score });
      floor = Math.max(floor, score - level.slack - 1);
    }
    if (s.out) break;
    ranked = next.sort((a, b) => b.score - a.score);
  }
  return ranked;
}

/** Ciel's move: the best, or at his level one a little worse that a person might play. */
export function choose(c: Chess, level: Level, random = Math.random): Choice | null {
  const ranked = rank(c, level);
  if (!ranked.length) return null;
  const best = ranked[0].score;
  // Never misses a mate, never walks into one.
  if (Math.abs(best) > MATE / 2) return ranked[0];
  // Some room in the opening, so no two games start alike.
  const slack = Math.max(level.slack, c.full <= 4 ? 25 : 0);
  const near = ranked.filter((r) => r.score >= best - slack);
  return near[Math.floor(random() * near.length)];
}
