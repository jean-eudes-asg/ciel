// Tools → Timers: a quick field ("5", "25m tea", "18:30 call mum"), lengths
// to pick, Focus rounds, and what is set — counting down beside the rain
// clock. Rust keeps them (src-tauri/src/timers.rs), so they ring with the
// island shut.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import type { TimerItem } from "../core/tools";
import { PRESETS, clock, msLeft, ordered, parseQuick, progress, type Quick } from "../core/timers";
import { h, svg } from "../views/dom";
import { ICONS } from "../views/icons";
import { twelveHours } from "../views/clockformat";
import type { ViewActions } from "../views/views";
import { language, t, tl } from "../i18n/i18n";
import { Tools } from "./store";
import { RainClock, type RainMood } from "./rainclock";
import { roundButton, typingField } from "./ui";
import type { ToolPanel } from "./view";

const MIN = 60_000;

/** "18:30", "Tomorrow 09:00", "Mon 07:15". */
export function dueText(due: number, now = Date.now()): string {
  const lang = language();
  const d = new Date(due);
  const time = new Intl.DateTimeFormat(lang, {
    hour: "numeric",
    minute: "2-digit",
    hour12: twelveHours(State.settings.clockFormat, lang),
  }).format(d);
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((day(d) - day(new Date(now))) / 86_400_000);
  if (days <= 0) return time;
  if (days === 1) return t("Tomorrow {time}", { time });
  return `${new Intl.DateTimeFormat(lang, { weekday: "short" }).format(d)} ${time}`;
}

const REPEATS = [
  { id: "", label: tl("Once") },
  { id: "daily", label: tl("Every day") },
  { id: "weekdays", label: tl("Weekdays") },
  { id: "weekly", label: tl("Every week") },
] as const;

export function timerName(x: TimerItem): string {
  if (x.label) return x.label;
  if (x.kind === "focus") return x.phase === "break" ? t("Break") : t("Focus");
  return x.kind === "reminder" ? t("Reminder") : t("Timer");
}

export function buildTimers(actions: ViewActions): ToolPanel {
  const rain = new RainClock();
  const big = h("div", { class: "rain-time" });
  const caption = h("div", { class: "rain-caption" });

  const input = typingField(h("input", {
    class: "tool-input",
    type: "text",
    spellcheck: "false",
    placeholder: tl("5, 25m tea, 18:30 call mum…"),
  }) as HTMLInputElement);
  const repeat = h("select", { class: "tool-select", title: tl("Repeat") }) as HTMLSelectElement;
  for (const r of REPEATS) repeat.append(h("option", { value: r.id, text: r.label }));
  const go = h("button", { class: "btn primary tool-go", onclick: () => submit() }, h("span", { text: tl("Start") }));
  const hint = h("div", { class: "tool-hint" });
  const quickRow = h("div", { class: "tool-row" }, input, repeat, go);

  const presets = h("div", { class: "tool-chips" });
  for (const p of PRESETS) {
    presets.append(h("button", {
      class: "tool-chip",
      text: t("{minutes} min", { minutes: p.minutes }),
      onclick: () => start({ kind: "timer", label: "", ms: p.minutes * MIN }),
    }));
  }
  const focusChip = h("button", {
    class: "tool-chip focus",
    title: tl("25 minutes of focus, 5 of break, and a longer break every fourth round"),
    onclick: () => {
      actions.blip();
      void Bridge.focusStart(25 * MIN, 5 * MIN);
    },
  }, svg(ICONS.timer, 10), h("span", { text: tl("Focus 25 / 5") }));
  presets.append(focusChip);

  const list = h("div", { class: "tool-list timers-list" });
  const el = h("div", { class: "tool tool-timers" },
    h("div", { class: "rain-side" }, rain.el, big, caption),
    h("div", { class: "tool-main" }, quickRow, hint, presets, list),
  );

  let parsed: Quick | null = null;
  input.addEventListener("input", () => {
    parsed = parseQuick(input.value);
    paintHint();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      submit();
    }
  });

  function paintHint() {
    const reminder = parsed?.kind === "reminder";
    repeat.style.display = reminder ? "" : "none";
    go.firstElementChild!.textContent = reminder ? t("Remind me") : t("Start");
    if (!input.value.trim()) hint.textContent = "";
    else if (!parsed) hint.textContent = t("A length (5, 25m, 1h30) or a time (18:30, 6pm).");
    else if (parsed.kind === "reminder") hint.textContent = `${t("Reminder")} · ${dueText(parsed.due)}${parsed.label ? ` · ${parsed.label}` : ""}`;
    else hint.textContent = `${t("Timer")} · ${clock(parsed.ms)}${parsed.label ? ` · ${parsed.label}` : ""}`;
    hint.classList.toggle("bad", !!input.value.trim() && !parsed);
  }

  paintHint();

  function start(q: Quick) {
    actions.blip();
    if (q.kind === "timer") void Bridge.timerStart(q.label, q.ms);
    else void Bridge.reminderAdd(q.label, q.due, repeat.value);
  }

  function submit() {
    if (!parsed) {
      input.focus();
      return;
    }
    start(parsed);
    input.value = "";
    parsed = null;
    repeat.value = "";
    paintHint();
    input.blur();
  }

  const act = (x: TimerItem, action: string, value?: number) => {
    actions.blip();
    void Bridge.timerAction(x.id, action, value);
  };

  /** One row; its countdown text is kept to be updated each second. */
  const clocks = new Map<string, { text: HTMLElement; bar: HTMLElement }>();
  function row(x: TimerItem, now: number): HTMLElement {
    const icon = x.kind === "reminder" ? svg(ICONS.bell2, 12, { stroke: 2 }) : svg(ICONS.timer, 12);
    const when = h("span", { class: "timer-when" });
    const bar = h("i", { class: "timer-bar" });
    clocks.set(x.id, { text: when, bar });
    const buttons = h("div", { class: "timer-actions" });
    if (x.ringing) {
      if (x.kind === "reminder") buttons.append(h("button", { class: "tool-chip", text: tl("Snooze 10 min"), onclick: () => act(x, "snooze") }));
      else buttons.append(h("button", { class: "tool-chip", text: tl("Again"), onclick: () => act(x, "again") }));
      buttons.append(h("button", { class: "tool-chip strong", text: x.kind === "reminder" ? tl("Done") : tl("Stop"), onclick: () => act(x, "done") }));
    } else {
      if (x.kind !== "reminder") {
        buttons.append(h("button", { class: "tool-chip", text: t("+{minutes} min", { minutes: 1 }), title: tl("One more minute"), onclick: () => act(x, "add", MIN) }));
        buttons.append(x.left != null
          ? roundButton(ICONS.play, tl("Resume"), () => act(x, "resume"), 0)
          : roundButton(ICONS.pause, tl("Pause"), () => act(x, "pause"), 0));
      }
      buttons.append(roundButton(ICONS.xmark, tl("Cancel"), () => act(x, "cancel"), 0));
    }
    const name = h("span", { class: "timer-name", text: timerName(x) });
    const meta = h("div", { class: "timer-meta" }, name, when);
    if (x.kind === "focus") meta.append(h("span", { class: "timer-tag", text: t("Round {round}", { round: x.round + (x.phase === "break" ? 0 : 1) }) }));
    if (x.repeat) meta.append(svg(ICONS.repeatSmall, 10, { stroke: 2 }));
    const r = h("div", { class: `timer-row ${x.kind}${x.ringing ? " ringing" : ""}${x.phase === "break" ? " break" : ""}` },
      h("span", { class: "timer-icon" }, icon),
      h("div", { class: "timer-text" }, meta, x.kind === "reminder" ? null : h("div", { class: "timer-track" }, bar)),
      buttons,
    );
    paintClock(x, now);
    return r;
  }

  function paintClock(x: TimerItem, now: number) {
    const c = clocks.get(x.id);
    if (!c) return;
    if (x.ringing) c.text.textContent = x.kind === "reminder" ? dueText(x.due, now) : t("Time's up");
    else if (x.kind === "reminder") c.text.textContent = dueText(x.due, now);
    else c.text.textContent = clock(msLeft(x, now));
    c.bar.style.width = `${Math.round(progress(x, now) * 100)}%`;
  }

  let key = "";
  let second = -1;
  const empty = h("div", { class: "tool-empty", text: tl("Nothing ticking. Type a length or a time, or pick one.") });

  function paintSide(now: number) {
    const timers = ordered(Tools.timers, now);
    const lead = timers.find((x) => x.kind !== "reminder") ?? null;
    let mood: RainMood = "idle";
    if (lead?.ringing) mood = "rang";
    else if (lead?.left != null) mood = "paused";
    else if (lead?.kind === "focus") mood = lead.phase === "break" ? "break" : "focus";
    else if (lead) mood = "timer";
    rain.mood = mood;
    rain.progress = lead ? (lead.ringing ? 1 : progress(lead, now)) : 0;
    big.textContent = lead ? (lead.ringing ? t("Time's up") : clock(msLeft(lead, now))) : "";
    caption.textContent = lead ? timerName(lead) : t("Rain clock");
    big.classList.toggle("rang", !!lead?.ringing);
  }

  return {
    el,
    shown() {
      void Bridge.timersList().then((l) => {
        if (l) {
          Tools.timers = l;
          State.notify();
        }
      });
    },
    sync() {
      const now = Date.now();
      const items = ordered(Tools.timers, now);
      const next = items.map((x) => `${x.id}:${x.ringing}:${x.left != null}:${x.phase}:${x.round}:${x.repeat}:${x.label}`).join("|");
      if (next !== key) {
        key = next;
        clocks.clear();
        list.replaceChildren(...(items.length ? items.map((x) => row(x, now)) : [empty]));
      }
      paintSide(now);
    },
    tick(nowMs: number) {
      const now = Date.now();
      const s = Math.floor(now / 1000);
      if (s !== second) {
        second = s;
        for (const x of Tools.timers) paintClock(x, now);
        paintSide(now);
      }
      return rain.frame(nowMs) || Tools.timers.some((x) => x.left == null && x.kind !== "reminder");
    },
    focus() {
      input.focus();
    },
  };
}
