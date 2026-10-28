// International draughts on the Games page (draughts.ts), against the same
// five rivals as chess (opponent.ts): the 10x10 board in his sky's colours,
// click or drag a piece to where it can go, a capture hops square by square
// and the pieces taken go when it ends. White moves first. His moves come
// from worker.ts, in a person's time; the game waits in this window's
// storage.

import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { button, segmented } from "../hub/ui";
import { Draughts, SIZE, evaluate, notation, type Move, type Side } from "./draughts";
import { rival, type RivalId } from "./cast";
import { opponent, thinkingTime } from "./opponent";
import { Sfx } from "./sfx";

interface Saved {
  you: Side;
  rival: RivalId;
  moves: number[][];
}

const KEY = "ciel.draughts";
const HOP_MS = 190;
/** A swing this big, in hundredths of a man, and the rival says something. */
const SWING = 180;

function load(): Saved {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "") as Saved;
    if (Array.isArray(s.moves) && (s.you === "w" || s.you === "b")) return { ...s, rival: rival(s.rival).id };
  } catch {
    // A first game.
  }
  return { you: "w", rival: "cumulus", moves: [] };
}

function pieceSvg(p: string): string {
  const dark = p.toLowerCase() === "b";
  const [top, bottom, ring] = dark ? ["#4a5262", "#1f242d", "#6b7484"] : ["#fbf8f1", "#d9d2c4", "#b9b0a0"];
  const crown = p === p.toUpperCase()
    ? `<path d="M34 56 L32 40 L42 48 L50 36 L58 48 L68 40 L66 56 Z" fill="${dark ? "#e9c46a" : "#b8862f"}" stroke="${dark ? "#14171d" : "#5b4a2a"}" stroke-width="2" stroke-linejoin="round"/>`
    : `<circle cx="50" cy="48" r="17" fill="none" stroke="${ring}" stroke-width="3"/>`;
  return `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
<ellipse cx="50" cy="58" rx="38" ry="30" fill="${bottom}" stroke="${dark ? "#0b0d11" : "#6f6656"}" stroke-width="2.5"/>
<ellipse cx="50" cy="48" rx="38" ry="30" fill="${top}" stroke="${dark ? "#0b0d11" : "#6f6656"}" stroke-width="2.5"/>
<ellipse cx="50" cy="48" rx="28" ry="21" fill="none" stroke="${ring}" stroke-width="2.5" opacity="0.8"/>
${crown}
</svg>`;
}

export function draughtsGame(): { el: HTMLElement; leave(): void } {
  const saved = load();
  const game = new Draughts();
  for (const path of saved.moves) {
    const m = game.moves().find((m) => m.path.join() === path.join());
    if (!m) break;
    game.play(m);
  }

  const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  let asked = 0;
  let thinking = false;
  let selected = -1;
  let hopping = false;
  let foeScore = 0;

  const board = h("div", { class: "cb-board ck-board" });
  const squares = h("div", { class: "cb-squares" });
  const pieces = h("div", { class: "cb-pieces" });
  board.append(squares, pieces);
  const status = h("div", { class: "cb-status" });
  const list = h("div", { class: "cb-moves" });
  const rival_ = opponent(saved.rival, () => el, (r) => {
    saved.rival = r.id;
    newGame();
  });
  const { face } = rival_;

  const sidePick = segmented<Side>([["w", t("White")], ["b", t("Black")]], saved.you, (v) => {
    saved.you = v;
    newGame();
  });
  const backBtn = button(t("Take back"), () => takeBack(), "", true);
  const side = h("div", { class: "cb-side" },
    rival_.picker,
    rival_.card,
    status,
    list,
    h("div", { class: "cb-row" }, backBtn, button(t("New game"), () => newGame(), "primary", true)),
    h("div", { class: "cb-row" }, h("span", { class: "cb-label", text: t("You play") }), sidePick),
  );
  const el = h("div", { class: "cb" }, board, side);

  const last = SIZE - 1;
  const flip = () => saved.you === "b";
  const spot = (i: number) => (flip() ? [last - (i % SIZE), last - Math.floor(i / SIZE)] : [i % SIZE, Math.floor(i / SIZE)]);
  const at = (c: number, r: number) => (flip() ? (last - r) * SIZE + (last - c) : r * SIZE + c);
  const place = (node: HTMLElement, i: number) => {
    const [c, r] = spot(i);
    node.style.transform = `translate(${c * 100}%, ${r * 100}%)`;
  };

  const mineMoves = () => (game.turn === saved.you && !thinking && !hopping && !game.result() ? game.moves() : []);

  function draw() {
    clear(squares);
    clear(pieces);
    const last = game.history.at(-1)?.move;
    const moves = mineMoves();
    const movable = new Set(moves.map((m) => m.path[0]));
    const targets = new Set(moves.filter((m) => m.path[0] === selected).map((m) => m.path[m.path.length - 1]));
    for (let r = 0; r < SIZE; r++) {
      for (let c = 0; c < SIZE; c++) {
        const i = at(c, r);
        const dark = (Math.floor(i / SIZE) + (i % SIZE)) % 2 === 1;
        const cls = ["cb-sq", dark ? "dark" : "light"];
        if (last?.path.includes(i)) cls.push("last");
        if (i === selected) cls.push("picked");
        if (targets.has(i)) cls.push("target");
        if (movable.has(i) && moves[0]?.taken.length) cls.push("must");
        squares.append(h("div", { class: cls.join(" ") }));
      }
    }
    for (let i = 0; i < SIZE * SIZE; i++) {
      const p = game.board[i];
      if (!p) continue;
      const node = h("div", { class: "cb-piece ck-piece" });
      node.innerHTML = pieceSvg(p);
      node.dataset.sq = String(i);
      place(node, i);
      pieces.append(node);
    }
    drawSide();
  }

  function drawSide() {
    const over = game.result();
    let text: string;
    if (over) text = over.winner === null ? t("A draw.") : over.winner === saved.you ? t("You win!") : t("{0} wins.", { 0: rival_.foe.name });
    else text = thinking ? t("{0} is thinking…", { 0: rival_.foe.name }) : t("Your move");
    status.textContent = text;
    status.classList.toggle("over", !!over);
    status.classList.toggle("thinking", thinking && !over);
    clear(list);
    const played = game.history.map((u) => notation(u.move));
    for (let k = 0; k < played.length; k += 2) {
      list.append(h("span", { class: "n", text: `${k / 2 + 1}.` }), h("span", { text: played[k] }), h("span", { text: played[k + 1] ?? "" }));
    }
    list.scrollTop = list.scrollHeight;
    backBtn.toggleAttribute("disabled", game.history.length < 1);
  }

  function talk(event: string) {
    const at = game.history.length;
    const mine = game.board.filter((p) => p && p.toLowerCase() === saved.you).length;
    const theirs = game.board.filter((p) => p && p.toLowerCase() !== saved.you).length;
    rival_.talk([
      `The game is international draughts (10x10, flying kings). The user plays ${saved.you === "w" ? "White" : "Black"}, you play the other side.`,
      `${game.history.length} moves played. The user has ${mine} pieces left, you have ${theirs}.`,
      event,
    ], () => Math.abs(game.history.length - at) <= 1);
  }

  function store() {
    saved.moves = game.history.map((u) => u.move.path);
    try {
      localStorage.setItem(KEY, JSON.stringify(saved));
    } catch {
      // Not kept.
    }
  }

  /** Plays `m` with its hops shown one square at a time. */
  function hop(m: Move, done: () => void) {
    hopping = true;
    selected = -1;
    draw();
    const node = pieces.querySelector<HTMLElement>(`[data-sq="${m.path[0]}"]`);
    node?.classList.add("sliding");
    m.path.slice(1).forEach((sq, k) => {
      window.setTimeout(() => {
        if (node) place(node, sq);
        // The pieces taken stay until the move ends, then go together.
        if (m.taken.length) {
          pieces.querySelector<HTMLElement>(`[data-sq="${m.taken[k]}"]`)?.classList.add("hit");
          Sfx.capture();
        } else {
          Sfx.move();
        }
      }, k * HOP_MS + 30);
    });
    window.setTimeout(() => {
      for (const i of m.taken) pieces.querySelector<HTMLElement>(`[data-sq="${i}"]`)?.classList.add("gone");
    }, (m.path.length - 1) * HOP_MS + 60);
    window.setTimeout(() => {
      hopping = false;
      game.play(m);
      if (m.crowns) Sfx.star();
      store();
      draw();
      done();
    }, m.path.length * HOP_MS + 160);
  }

  function after() {
    const over = game.result();
    if (over) return ended(over.winner);
    if (game.turn !== saved.you) foeMoves();
  }

  function ended(winner: Side | null) {
    if (winner === null) {
      rival_.react("wink");
      Sfx.tune("draw");
      talk("The game just ended in a draw.");
    } else if (winner === saved.you) {
      face.setState("dizzy");
      Sfx.tune("win");
      rival_.react(null, "dizzy");
      talk("The user just won the game against you.");
    } else {
      rival_.react("proud", "proud");
      Sfx.tune("lose");
      talk("You just won the game against the user.");
    }
  }

  let due = 0;
  function foeMoves() {
    thinking = true;
    rival_.setThinking(true);
    drawSide();
    const legal = game.moves().length;
    due = performance.now() + thinkingTime(rival_.foe, legal, game.history.length < 6, !!game.history.at(-1)?.move.taken.length) * 1000;
    worker.postMessage({ game: "draughts", id: ++asked, moves: game.history.map((u) => u.move.path), level: rival_.foe.draughts });
  }

  worker.onmessage = (e: MessageEvent<{ id: number; path: number[] | null; score: number }>) => {
    const r = e.data;
    if (r.id !== asked || !r.path) return;
    window.setTimeout(() => {
      if (r.id !== asked) return;
      const m = game.moves().find((m) => m.path.join() === r.path!.join());
      if (!m) return;
      thinking = false;
      rival_.setThinking(false);
      const before = foeScore;
      foeScore = r.score;
      if (m.taken.length >= 2 || m.crowns) rival_.react("happy", "pop");
      if (foeScore - before >= SWING) talk("The user just made a mistake that gives you the advantage.");
      else if (before - foeScore >= SWING) {
        rival_.react("annoyed");
        talk("The user just played a strong move that turns the game their way.");
      }
      hop(m, after);
    }, Math.max(0, due - performance.now()));
  };

  function takeBack() {
    asked++;
    thinking = false;
    rival_.setThinking(false);
    game.takeBack();
    if (game.turn !== saved.you) game.takeBack();
    selected = -1;
    foeScore = evaluate(game) * (saved.you === "w" ? -1 : 1);
    store();
    Sfx.move();
    draw();
    if (game.turn !== saved.you && !game.result()) foeMoves();
  }

  function newGame() {
    asked++;
    thinking = false;
    const fresh = new Draughts();
    game.board = fresh.board;
    game.turn = fresh.turn;
    game.quiet = 0;
    game.history = [];
    game.seen = fresh.seen;
    foeScore = 0;
    selected = -1;
    rival_.say(null);
    face.setState("idle", true);
    face.greet();
    Sfx.tune("start");
    store();
    draw();
    talk("A new game starts. Greet the user in your own way and wish them a good game.");
    if (saved.you !== "w") foeMoves();
  }

  // ── Your moves ──
  const squareAt = (x: number, y: number) => {
    const r = board.getBoundingClientRect();
    const c = Math.floor(((x - r.left) / r.width) * SIZE);
    const row = Math.floor(((y - r.top) / r.height) * SIZE);
    return c < 0 || c > last || row < 0 || row > last ? -1 : at(c, row);
  };

  function tryMove(from: number, to: number): boolean {
    // ponytail: two captures to the same square taking different pieces play the first; a picker if it ever matters.
    const m = mineMoves().find((m) => m.path[0] === from && m.path[m.path.length - 1] === to);
    if (!m) return false;
    hop(m, after);
    return true;
  }

  let drag: { node: HTMLElement; from: number; x: number; y: number; moved: boolean } | null = null;
  board.addEventListener("pointerdown", (e) => {
    const moves = mineMoves();
    if (!moves.length) return;
    const i = squareAt(e.clientX, e.clientY);
    if (selected >= 0 && !moves.some((m) => m.path[0] === i)) {
      if (!tryMove(selected, i)) {
        selected = -1;
        draw();
      }
      return;
    }
    if (!moves.some((m) => m.path[0] === i)) {
      // A piece that can't move while another must take: show which.
      if (game.board[i] && game.board[i].toLowerCase() === saved.you) Sfx.bump();
      return;
    }
    selected = i;
    Sfx.pick();
    draw();
    const node = pieces.querySelector<HTMLElement>(`[data-sq="${i}"]`);
    if (!node) return;
    board.setPointerCapture(e.pointerId);
    drag = { node, from: i, x: e.clientX, y: e.clientY, moved: false };
  });
  board.addEventListener("pointermove", (e) => {
    rival_.look({ x: e.clientX, y: e.clientY });
    if (!drag) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 4) return;
    drag.moved = true;
    const [c, r] = spot(drag.from);
    const size = board.clientWidth / SIZE;
    drag.node.classList.add("dragging");
    drag.node.style.transform = `translate(${c * size + dx}px, ${r * size + dy}px) scale(1.1)`;
  });
  board.addEventListener("pointerleave", () => rival_.look(null));
  board.addEventListener("pointerup", (e) => {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (!d.moved) return;
    const to = squareAt(e.clientX, e.clientY);
    if (to === d.from || !tryMove(d.from, to)) {
      d.node.classList.remove("dragging");
      place(d.node, d.from);
    }
  });

  draw();
  if (!game.result() && game.turn !== saved.you) foeMoves();

  return {
    el,
    leave: () => {
      rival_.leave();
      worker.terminate();
    },
  };
}
