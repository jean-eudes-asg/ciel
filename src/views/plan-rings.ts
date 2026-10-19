// Plan usage at a glance, after Linotch's rings: while the island is folded, a
// small ring per plan beside Ciel shows how much of its busier window (5 hours
// or the week) is gone, in the gauges' green, orange and red, its track tinted
// with the agent's own colour. Hovering one shows the plan's card under the
// island — or, when hovering opens the island (General → Open on hover),
// opens it on that card; a click opens it on that card too. The card lives
// on the overview, so with no session running the island opens as usual.
//
// A ring shows when its pill does (Settings → Plan usage) and uses the pills'
// numbers (core/plan.ts): Claude's come with Claude Code's status line, and
// Codex is asked when its ring comes into view or is hovered, as its pill is —
// never on a timer. Numbers that have not moved in a while are drawn dimmed.

import { State } from "../core/state";
import { dominantPct, planColor, type PlanUsage } from "../core/plan";
import { pillColor } from "../core/pill-colors";
import { tl } from "../i18n/i18n";
import { h } from "./dom";
import { PlanCard, refreshCodexPlanUsage } from "./usage";

const NS = "http://www.w3.org/2000/svg";
/** The ring's size in the 32 px strip, its radius and stroke. */
const SIZE = 20;
const R = 8;
const STROKE = 2.4;
const ARC = 2 * Math.PI * R;

/** Older than this, a reading is drawn dimmed (its card says how old). */
const STALE_MS = { claude: 2 * 3_600_000, codex: 30 * 60_000 };
/** The track takes this much of the agent's colour. */
const TRACK_TINT = 30;

type Plan = "claude" | "codex";

/** Which rings the folded island shows, Claude first. */
export function ringPlans(): Plan[] {
  const s = State.settings;
  const plans: Plan[] = [];
  if (s.showPlanInNotch && s.planRelayInstalled) plans.push("claude");
  if (s.showCodexPlanInNotch) plans.push("codex");
  return plans;
}

const usageOf = (plan: Plan): PlanUsage | null => (plan === "codex" ? State.codexPlanUsage : State.planUsage);

const agentColor = (plan: Plan) =>
  plan === "codex"
    ? pillColor("agent_codex", "#2DD4BF", State.settings.pillColors)
    : pillColor("integration_claude", "#F5F6F8", State.settings.pillColors);

/** What a ring shows: the percentage (null without numbers), its colour, and whether it is stale. */
export function ringReading(plan: Plan, now = Date.now()): { pct: number | null; color: string; stale: boolean } {
  const u = usageOf(plan);
  const pct = dominantPct(u, now);
  const stale = !u || now - u.updatedAt > STALE_MS[plan];
  return { pct, color: planColor(pct), stale };
}

interface Ring {
  el: HTMLElement;
  arc: SVGCircleElement;
  track: SVGCircleElement;
  text: SVGTextElement;
}

function circle(stroke: string): SVGCircleElement {
  const c = document.createElementNS(NS, "circle");
  c.setAttribute("cx", String(SIZE / 2));
  c.setAttribute("cy", String(SIZE / 2));
  c.setAttribute("r", String(R));
  c.setAttribute("fill", "none");
  c.setAttribute("stroke", stroke);
  c.setAttribute("stroke-width", String(STROKE));
  return c;
}

function buildRing(plan: Plan): Ring {
  const svgEl = document.createElementNS(NS, "svg");
  svgEl.setAttribute("viewBox", `0 0 ${SIZE} ${SIZE}`);
  svgEl.setAttribute("width", String(SIZE));
  svgEl.setAttribute("height", String(SIZE));
  svgEl.setAttribute("aria-hidden", "true");
  const track = circle("#303030");
  const arc = circle("#6B7079");
  arc.setAttribute("stroke-linecap", "round");
  arc.setAttribute("stroke-dasharray", `${ARC} ${ARC}`);
  // From twelve o'clock, clockwise.
  arc.setAttribute("transform", `rotate(-90 ${SIZE / 2} ${SIZE / 2})`);
  const text = document.createElementNS(NS, "text");
  text.setAttribute("x", String(SIZE / 2));
  text.setAttribute("y", String(SIZE / 2));
  text.setAttribute("text-anchor", "middle");
  text.setAttribute("dominant-baseline", "central");
  svgEl.append(track, arc, text);
  const el = h("span", { class: `plan-ring plan-ring-${plan}` });
  el.append(svgEl);
  return { el, arc, track, text };
}

function drawRing(ring: Ring, plan: Plan, now: number) {
  const { pct, color, stale } = ringReading(plan, now);
  const shown = pct == null ? 0 : Math.max(pct, 2);
  ring.arc.setAttribute("stroke", color);
  ring.arc.setAttribute("stroke-dashoffset", String(ARC * (1 - shown / 100)));
  ring.arc.style.opacity = pct == null ? "0" : "1";
  ring.track.setAttribute("stroke", `color-mix(in srgb, ${agentColor(plan)} ${TRACK_TINT}%, #26282c)`);
  ring.text.textContent = pct == null ? "–" : String(Math.round(pct));
  ring.text.classList.toggle("three", pct != null && Math.round(pct) >= 100);
  ring.el.classList.toggle("stale", stale);
  const name = plan === "codex" ? tl("Codex plan usage") : tl("Claude plan usage");
  ring.el.title = pct == null ? String(name) : `${name}: ${Math.round(pct)}%`;
}

export interface PlanRings {
  /** The rings, for the island to place. */
  el: HTMLElement;
  /** The hovered ring's card, under the island. */
  pop: HTMLElement;
  /** Follows State: which rings, their numbers, shown or not. */
  sync(): void;
  /** Their width, to place them (0 when there are none). */
  readonly width: number;
}

export function buildPlanRings(): PlanRings {
  const el = h("div", { id: "plan-rings" });
  const card = new PlanCard("plan-pop-card");
  const tail = h("i", { class: "plan-pop-tail" });
  const pop = h("div", { id: "plan-pop" }, tail, card.el);
  const rings = new Map<Plan, Ring>();
  let hovered: Plan | null = null;
  let hideTimer: number | null = null;
  let shownKey = "";
  let wasShown = false;

  const showPop = (plan: Plan, ring: Ring) => {
    if (hideTimer != null) window.clearTimeout(hideTimer);
    hideTimer = null;
    hovered = plan;
    if (plan === "codex") refreshCodexPlanUsage();
    card.sync(Date.now(), plan === "codex");
    // The tail points at the ring: its centre, from the island's right edge.
    const island = el.offsetParent as HTMLElement | null;
    const ringRight = island ? island.clientWidth - (el.offsetLeft + ring.el.offsetLeft + SIZE / 2) : 0;
    pop.style.setProperty("--tail-right", `${Math.max(14, ringRight - 6)}px`);
    pop.classList.add("on");
  };
  const hidePop = () => {
    if (hideTimer != null) window.clearTimeout(hideTimer);
    // A moment's grace, so going from one ring to the next doesn't blink.
    hideTimer = window.setTimeout(() => {
      hideTimer = null;
      hovered = null;
      pop.classList.remove("on");
    }, 140);
  };

  /** The island, as it opens, shows this plan's card. */
  const openOnCard = (plan: Plan) => {
    State.planDetailIsCodex = plan === "codex";
    State.showingPlanDetail = true;
    if (plan === "codex") refreshCodexPlanUsage();
    State.notify();
  };

  const ringFor = (plan: Plan): Ring => {
    let ring = rings.get(plan);
    if (ring) return ring;
    const made = buildRing(plan);
    made.el.addEventListener("mouseenter", () => {
      if (State.settings.openOnHover) openOnCard(plan);
      else showPop(plan, made);
    });
    made.el.addEventListener("mouseleave", hidePop);
    // Before the island's own handler opens it.
    made.el.addEventListener("mousedown", () => openOnCard(plan));
    rings.set(plan, made);
    return made;
  };

  return {
    el,
    pop,
    get width() {
      const n = el.childElementCount;
      return n ? n * SIZE + (n - 1) * 6 : 0;
    },
    sync() {
      const plans = State.mode === "compact" ? ringPlans() : [];
      const show = plans.length > 0;
      el.classList.toggle("on", show);
      if (!show) {
        if (hovered) {
          hovered = null;
          pop.classList.remove("on");
        }
        wasShown = false;
        return;
      }
      // Coming into view is when Codex is asked, as its pill is when it shows.
      if (!wasShown && plans.includes("codex")) refreshCodexPlanUsage();
      wasShown = true;
      const key = plans.join("|");
      if (key !== shownKey) {
        shownKey = key;
        el.replaceChildren(...plans.map((p) => ringFor(p).el));
      }
      const now = Date.now();
      for (const p of plans) drawRing(ringFor(p), p, now);
      if (hovered && plans.includes(hovered)) card.sync(now, hovered === "codex");
    },
  };
}
