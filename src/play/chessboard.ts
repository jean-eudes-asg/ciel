// Chess on the Games page, against one of Ciel's family (cast.ts): the board
// in his sky's colours (click or drag a piece), him in his own sky beside it,
// with his face for the game and a line from his brain now and then, the
// moves, and who is better. His moves come from worker.ts, and he takes
// a person's time over them. The game is kept in this window's storage, so a
// break can end mid-game.

import { h, clear } from "../views/dom";
import { t } from "../i18n/i18n";
import { button, segmented } from "../hub/ui";
import { Chess, START, name, type Color, type Move } from "./chess";
import { pieceSvg } from "./pieces";
import { rival, type RivalId } from "./cast";
import { opponent, thinkingTime } from "./opponent";
import { Sfx } from "./sfx";

interface Saved {
  you: Color;
  rival: RivalId;
  moves: string[];
}

const KEY = "ciel.chess";
/** After a line from the rival, this many of your moves pass before the next one (but the end and a hint). */
const QUIET_MOVES = 3;
/** How much a move must change the game, in hundredths of a pawn, for the rival to say something. */
const SWING = 250;

const uci = (m: Move) => name(m.from) + name(m.to) + m.promo.toLowerCase();

function load(): Saved {
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? "") as Saved;
    if (Array.isArray(s.moves) && (s.you === "w" || s.you === "b")) return { ...s, rival: rival(s.rival).id };
  } catch {
    // A first game.
  }
  return { you: "w", rival: "cumulus", moves: [] };
}

function save(s: Saved) {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // Not kept: the game still goes on.
  }
}

export function chessGame(): { el: HTMLElement; leave(): void } {
  const saved = load();
  const game = new Chess();
  for (const u of saved.moves) {
    const m = game.moves().find((m) => uci(m) === u);
    if (!m) break;
    game.play(m);
  }

  const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
  let asked = 0;
  let thinking = false;
  let selected = -1;
  let hint: [number, number] | null = null;
  let slide: Move | null = null;
  /** The rival's last score for the game, from his side. */
  let foeScore = 0;
  let sinceLine = QUIET_MOVES;

  const board = h("div", { class: "cb-board" });
  const squares = h("div", { class: "cb-squares" });
  const pieces = h("div", { class: "cb-pieces" });
  const arrow = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  arrow.setAttribute("viewBox", "0 0 800 800");
  arrow.classList.add("cb-arrow");
  const promo = h("div", { class: "cb-promo" });
  board.append(squares, pieces, arrow, promo);
  const evalFill = h("i", {});
  const evalBar = h("div", { class: "cb-eval" }, evalFill);

  const status = h("div", { class: "cb-status" });
  const list = h("div", { class: "cb-moves" });
  const rival_ = opponent(saved.rival, () => el, (r) => {
    saved.rival = r.id;
    newGame();
  });
  const { face } = rival_;

  const sidePick = segmented<Color>([["w", t("White")], ["b", t("Black")]], saved.you, (v) => {
    saved.you = v;
    newGame();
  });
  const hintBtn = button(t("Hint"), () => askHint(), "", true);
  const backBtn = button(t("Take back"), () => takeBack(), "", true);

  const side = h("div", { class: "cb-side" },
    rival_.picker,
    rival_.card,
    status,
    list,
    h("div", { class: "cb-row" }, hintBtn, backBtn, button(t("New game"), () => newGame(), "primary", true)),
    h("div", { class: "cb-row" }, h("span", { class: "cb-label", text: t("You play") }), sidePick),
  );
  const el = h("div", { class: "cb" }, evalBar, board, side);

  // ── Drawing ──
  const flip = () => saved.you === "b";
  /** Column and row on screen of a mailbox square. */
  const spot = (i: number) => {
    const f = (i - 21) % 10;
    const r = Math.floor((i - 21) / 10);
    return flip() ? [7 - f, 7 - r] : [f, r];
  };
  const at = (col: number, row: number) => (flip() ? 21 + (7 - col) + (7 - row) * 10 : 21 + col + row * 10);
  const place = (node: HTMLElement, i: number) => {
    const [c, r] = spot(i);
    node.style.transform = `translate(${c * 100}%, ${r * 100}%)`;
  };

  function draw() {
    clear(squares);
    clear(pieces);
    const last = game.history.at(-1)?.move;
    const targets = new Set(selected >= 0 ? game.moves().filter((m) => m.from === selected).map((m) => m.to) : []);
    const checked = game.inCheck() ? game.king(game.turn) : -1;
    for (let row = 0; row < 8; row++) {
      for (let col = 0; col < 8; col++) {
        const i = at(col, row);
        const cls = ["cb-sq", (col + row) % 2 ? "dark" : "light"];
        if (last && (i === last.from || i === last.to)) cls.push("last");
        if (i === selected) cls.push("picked");
        if (i === checked) cls.push("check");
        if (targets.has(i)) cls.push(game.board[i] ? "target take" : "target");
        const s = h("div", { class: cls.join(" ") });
        if (col === 0) s.append(h("span", { class: "cb-rank", text: String(8 - Math.floor((i - 21) / 10)) }));
        if (row === 7) s.append(h("span", { class: "cb-file", text: name(i)[0] }));
        squares.append(s);
      }
    }
    for (let i = 21; i <= 98; i++) {
      const p = game.board[i];
      if (!p || p === "x") continue;
      const node = h("div", { class: "cb-piece" });
      node.innerHTML = pieceSvg(p);
      node.dataset.sq = String(i);
      const moving = slide && (i === slide.to || (slide.flag === "k" && i === slide.to - 1) || (slide.flag === "q" && i === slide.to + 1));
      if (moving && slide) {
        const from = i === slide.to ? slide.from : slide.flag === "k" ? slide.to + 1 : slide.to - 2;
        place(node, from);
        node.classList.add("sliding");
        requestAnimationFrame(() => requestAnimationFrame(() => place(node, i)));
      } else {
        place(node, i);
      }
      pieces.append(node);
    }
    slide = null;
    drawArrow();
    drawSide();
  }

  function drawArrow() {
    arrow.innerHTML = "";
    if (!hint) return;
    const [a, b] = hint.map((i) => spot(i).map((v) => v * 100 + 50));
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]);
    const ux = (b[0] - a[0]) / len;
    const uy = (b[1] - a[1]) / len;
    const end = [b[0] - ux * 26, b[1] - uy * 26];
    arrow.innerHTML = `<line x1="${a[0]}" y1="${a[1]}" x2="${end[0]}" y2="${end[1]}" stroke-width="16" stroke-linecap="round"/>` +
      `<polygon points="${b[0]},${b[1]} ${end[0] - uy * 24},${end[1] + ux * 24} ${end[0] + uy * 24},${end[1] - ux * 24}"/>`;
  }

  function drawSide() {
    const over = game.result();
    let text: string;
    if (over?.why === "mate") text = over.winner === saved.you ? t("Checkmate. You win!") : t("Checkmate. {0} wins.", { 0: rival_.foe.name });
    else if (over) text = over.why === "stalemate" ? t("Stalemate: a draw.") : t("A draw.");
    else text = thinking ? t("{0} is thinking…", { 0: rival_.foe.name }) : t("Your move");
    status.textContent = text;
    status.classList.toggle("over", !!over);
    status.classList.toggle("thinking", thinking && !over);
    clear(list);
    const sans = game.line();
    for (let k = 0; k < sans.length; k += 2) {
      list.append(h("span", { class: "n", text: `${k / 2 + 1}.` }), h("span", { text: sans[k] }), h("span", { text: sans[k + 1] ?? "" }));
    }
    list.scrollTop = list.scrollHeight;
    // Who is better, from White's side, as a bar beside the board.
    const white = saved.you === "w" ? -foeScore : foeScore;
    const share = 50 + 50 * Math.tanh(white / 600);
    evalFill.style.height = `${flip() ? share : 100 - share}%`;
    evalBar.classList.toggle("flip", flip());
    hintBtn.toggleAttribute("disabled", thinking || !!over || game.turn !== saved.you);
    backBtn.toggleAttribute("disabled", game.history.length < 1);
  }

  /** Asks the brain for a line from the rival about `event`; nothing without a brain. */
  function talk(event: string) {
    const sans = game.line();
    const moves = sans.map((s, k) => (k % 2 ? s : `${k / 2 + 1}. ${s}`)).join(" ");
    const at = game.history.length;
    rival_.talk([
      `The game is chess. The user plays ${saved.you === "w" ? "White" : "Black"}, you play ${saved.you === "w" ? "Black" : "White"}.`,
      moves ? `Moves so far: ${moves}` : "No move played yet.",
      event,
    ], () => Math.abs(game.history.length - at) <= 1);
    sinceLine = 0;
  }

  // ── Playing ──
  function store() {
    saved.moves = game.history.map((u) => uci(u.move));
    save(saved);
  }

  function sound(m: Move) {
    if (m.flag === "k" || m.flag === "q") Sfx.castle();
    else if (m.captured) Sfx.capture();
    else Sfx.move();
    if (game.inCheck()) window.setTimeout(() => Sfx.check(), 120);
  }

  function played(m: Move) {
    hint = null;
    selected = -1;
    slide = m;
    store();
    sound(m);
    const over = game.result();
    draw();
    if (over) return ended(over.winner);
    if (game.turn !== saved.you) foeMoves();
  }

  function ended(winner: Color | null) {
    if (winner === null) {
      rival_.react("wink");
      Sfx.tune("draw");
      talk("The game just ended in a draw.");
    } else if (winner === saved.you) {
      face.setState("dizzy");
      Sfx.tune("win");
      rival_.react(null, "dizzy");
      talk("The user just checkmated you and won the game.");
    } else {
      rival_.react("proud", "proud");
      Sfx.tune("lose");
      talk("You just checkmated the user and won the game.");
    }
  }

  type Reply = { id: number; from?: number; to?: number; promo: string; score: number; hint?: boolean };
  /** When the rival's move may be played: a person's time over it, however quick the search was. */
  let due = 0;

  function foeMoves() {
    thinking = true;
    rival_.setThinking(true);
    drawSide();
    const recapture = !!game.history.at(-1)?.move.captured;
    due = performance.now() + thinkingTime(rival_.foe, game.moves().length, game.full <= 4, recapture) * 1000;
    worker.postMessage({ game: "chess", fen: game.fen(), level: rival_.foe.chess, id: ++asked });
  }

  worker.onmessage = (e: MessageEvent<Reply>) => {
    const r = e.data;
    if (r.id !== asked) return;
    if (r.hint) return showHint(r);
    window.setTimeout(() => {
      if (r.id === asked) answer(r);
    }, Math.max(0, due - performance.now()));
  };

  function answer(r: Reply) {
    const m = game.moves().find((m) => m.from === r.from && m.to === r.to && m.promo === r.promo);
    if (!m) return;
    thinking = false;
    rival_.setThinking(false);
    const before = foeScore;
    foeScore = r.score;
    const yourSan = game.history.length ? game.line().at(-1) : null;
    const san = game.san(m);
    game.play(m);
    if (m.captured && "QRqr".includes(m.captured)) rival_.react("happy", "pop");
    sinceLine++;
    if (yourSan && sinceLine >= QUIET_MOVES) {
      if (foeScore - before >= SWING) {
        rival_.react("surprised", "wink");
        talk(`The user just played ${yourSan}, a mistake that gives you the advantage; you answered ${san}.`);
      } else if (before - foeScore >= SWING) {
        rival_.react("annoyed");
        talk(`The user just played ${yourSan}, a strong move that turns the game their way; you answered ${san}.`);
      }
    }
    played(m);
  }

  function showHint(r: Reply) {
    const m = game.moves().find((m) => m.from === r.from && m.to === r.to && m.promo === r.promo);
    if (!m) return;
    hint = [m.from, m.to];
    drawArrow();
    Sfx.pick();
    talk(`The user asked for a hint. The suggested move for them is ${game.san(m)}. Encourage them to find why, without explaining it.`);
  }

  function askHint() {
    if (thinking || game.turn !== saved.you) return;
    worker.postMessage({ game: "chess", fen: game.fen(), level: rival_.foe.chess, id: ++asked, hint: true });
  }

  function takeBack() {
    asked++;
    thinking = false;
    rival_.setThinking(false);
    // Back to your turn: your last move and the rival's answer.
    game.takeBack();
    if (game.turn !== saved.you) game.takeBack();
    hint = null;
    selected = -1;
    store();
    Sfx.move();
    draw();
    if (game.turn !== saved.you && !game.result()) foeMoves();
  }

  function newGame() {
    asked++;
    thinking = false;
    game.load(START);
    foeScore = 0;
    hint = null;
    selected = -1;
    rival_.say(null);
    face.setState("idle", true);
    face.greet();
    Sfx.tune("start");
    store();
    draw();
    talk("A new game starts. Greet the user in your own way and wish them a good game.");
    if (saved.you === "b") foeMoves();
  }

  // ── Your moves: click, or drag ──
  function tryMove(from: number, to: number) {
    const options = game.moves().filter((m) => m.from === from && m.to === to);
    if (!options.length) return false;
    if (options.length === 1) {
      game.play(options[0]);
      played(options[0]);
      return true;
    }
    // A promotion: the four pieces over the square.
    clear(promo);
    promo.classList.add("on");
    for (const m of options) {
      const b = h("button", { class: "cb-promo-piece" });
      b.innerHTML = pieceSvg(m.promo);
      b.addEventListener("click", () => {
        promo.classList.remove("on");
        game.play(m);
        played(m);
      });
      promo.append(b);
    }
    return true;
  }

  const squareAt = (x: number, y: number) => {
    const r = board.getBoundingClientRect();
    const col = Math.floor(((x - r.left) / r.width) * 8);
    const row = Math.floor(((y - r.top) / r.height) * 8);
    return col < 0 || col > 7 || row < 0 || row > 7 ? -1 : at(col, row);
  };
  const mine = (i: number) => {
    const p = game.board[i];
    return !!p && p !== "x" && (p === p.toUpperCase() ? "w" : "b") === saved.you;
  };

  let drag: { node: HTMLElement; from: number; x: number; y: number; moved: boolean } | null = null;
  board.addEventListener("pointerdown", (e) => {
    if (thinking || game.result() || game.turn !== saved.you || promo.classList.contains("on")) return;
    const i = squareAt(e.clientX, e.clientY);
    if (selected >= 0 && !mine(i)) {
      if (!tryMove(selected, i)) {
        selected = -1;
        draw();
      }
      return;
    }
    if (!mine(i)) return;
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
    const size = board.clientWidth / 8;
    drag.node.classList.add("dragging");
    drag.node.style.transform = `translate(${c * size + dx}px, ${r * size + dy}px) scale(1.12)`;
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
