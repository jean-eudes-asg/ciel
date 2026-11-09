// The Tools' pure parts (src/core/clip.ts, timers.ts, weather.ts, lyrics.ts,
// live.ts) and Ciel without a coding tool (src/core/pills.ts).

import { test } from "node:test";
import assert from "node:assert/strict";
import {
  classify, cleanLink, colorForms, counts, evalMath, linkHost, matches, parseColor, preview, transform,
} from "../src/core/clip.ts";
import { clock, msLeft, ordered, parseQuick, progress } from "../src/core/timers.ts";
import { rainLine, skyOf, temp, themeForWeather, unitsFor, unitsOnPick, wind } from "../src/core/weather.ts";
import { lineAt, parseLrc } from "../src/core/lyrics.ts";
import { holdsIsland, pickLive, ticks } from "../src/core/live.ts";
import {
  DEFAULT_MAIN_PILL, NO_MAIN_PILL, chooseMainPill, orderPills, sanitizeDeclared, toggleDeclared,
} from "../src/core/pills.ts";
import { State } from "../src/core/state.ts";

// ── Clipboard ─────────────────────────────────────────────────────────────────

test("a copied text is recognised for what it is", () => {
  assert.equal(classify("https://github.com/jean-eudes-asg/ciel"), "link");
  assert.equal(classify("www.example.com/a"), "link");
  assert.equal(classify("#7FB4EA"), "color");
  assert.equal(classify("rgb(127, 180, 234)"), "color");
  assert.equal(classify("someone@example.org"), "email");
  assert.equal(classify("12*7"), "math");
  assert.equal(classify("+33 6 12 34 56 78"), "phone");
  assert.equal(classify("C:\\Users\\me\\notes.txt"), "path");
  assert.equal(classify('{"a": [1, 2]}'), "json");
  assert.equal(classify("const a = 1;\nif (a) {\n  run();\n}"), "code");
  assert.equal(classify("Thanks, see you on Monday."), "text");
  // Words that happen to be hex, a bare number, a date: not colours, not sums.
  assert.equal(classify("facade"), "text");
  assert.equal(classify("123456"), "text");
  assert.equal(classify("2026-11-07"), "text");
});

test("a link loses its trackers and keeps the rest", () => {
  assert.equal(
    cleanLink("https://example.com/a?id=42&utm_source=news&utm_medium=mail&fbclid=x#top"),
    "https://example.com/a?id=42#top",
  );
  assert.equal(cleanLink("https://example.com/b?utm_campaign=z"), "https://example.com/b");
  assert.equal(cleanLink("https://youtu.be/abc?si=TRACK"), "https://youtu.be/abc");
  // Nothing to clean, or not a link at all.
  assert.equal(cleanLink("https://example.com/c?q=sky"), null);
  assert.equal(cleanLink("not a link"), null);
  assert.equal(linkHost("https://www.github.com/x"), "github.com");
});

test("colours read in every spelling and convert", () => {
  assert.deepEqual(parseColor("#7fb4ea"), { r: 127, g: 180, b: 234, a: 1 });
  assert.deepEqual(parseColor("#abc"), { r: 170, g: 187, b: 204, a: 1 });
  assert.deepEqual(parseColor("c0ffee"), { r: 192, g: 255, b: 238, a: 1 });
  assert.equal(parseColor("rgb(300, 0, 0)"), null);
  assert.deepEqual(parseColor("hsl(0, 100%, 50%)"), { r: 255, g: 0, b: 0, a: 1 });
  const forms = colorForms({ r: 127, g: 180, b: 234, a: 1 });
  assert.equal(forms.hex, "#7FB4EA");
  assert.equal(forms.rgb, "rgb(127, 180, 234)");
  assert.match(forms.hsl, /^hsl\(210, 7\d%, 7\d%\)$/);
  assert.equal(colorForms({ r: 0, g: 0, b: 0, a: 0.5 }).hex, "#00000080");
});

test("sums are worked out by hand, never run as code", () => {
  assert.equal(evalMath("12*7"), 84);
  assert.equal(evalMath("1 200 / 3 + 5"), 405);
  assert.equal(evalMath("2^10"), 1024);
  assert.equal(evalMath("(4+6) × 2,5"), 25);
  assert.equal(evalMath("10 - 4"), 6);
  assert.equal(evalMath("7 % 3"), 1);
  for (const not of ["42", "-5", "2026-11-07", "alert(1)", "1+", "Math.PI*2", "", "3 apples + 2"]) {
    assert.equal(evalMath(not), null, not);
  }
});

test("text turns into its other forms", () => {
  assert.equal(transform("hello world", "upper"), "HELLO WORLD");
  assert.equal(transform("HELLO World", "lower"), "hello world");
  assert.equal(transform("the sky over lyon", "title"), "The Sky Over Lyon");
  assert.equal(transform("  a   b \n\n\n\n c  ", "trim"), "a b\n\nc");
  assert.equal(transform('{"a":1}', "json"), '{\n  "a": 1\n}');
  assert.deepEqual(counts("one two  three\nfour"), { words: 4, chars: 19, lines: 2 });
  assert.equal(preview("a\n\nb   c", 10), "a b c");
  assert.equal(preview("x".repeat(20), 10), `${"x".repeat(9)}…`);
});

test("the search finds every word, in the text or the app", () => {
  const item = { text: "Invoice for October", app: "Outlook" };
  assert.ok(matches(item, "invoice outlook"));
  assert.ok(matches(item, ""));
  assert.ok(!matches(item, "invoice gmail"));
});

// ── Timers ────────────────────────────────────────────────────────────────────

const NOW = new Date(2026, 10, 7, 15, 0, 0); // Saturday 7 November 2026, 15:00

test("the quick field reads lengths in any language's way of writing them", () => {
  assert.deepEqual(parseQuick("5", NOW), { kind: "timer", label: "", ms: 5 * 60_000 });
  assert.deepEqual(parseQuick("25m tea", NOW), { kind: "timer", label: "tea", ms: 25 * 60_000 });
  assert.deepEqual(parseQuick("tea 4 min", NOW), { kind: "timer", label: "tea", ms: 4 * 60_000 });
  assert.deepEqual(parseQuick("1h30", NOW), { kind: "timer", label: "", ms: 90 * 60_000 });
  assert.deepEqual(parseQuick("1h 30m pasta", NOW), { kind: "timer", label: "pasta", ms: 90 * 60_000 });
  assert.deepEqual(parseQuick("90s", NOW), { kind: "timer", label: "", ms: 90_000 });
  assert.deepEqual(parseQuick("in 10 minutes check the oven", NOW), { kind: "timer", label: "check the oven", ms: 10 * 60_000 });
  assert.deepEqual(parseQuick("2,5 min", NOW), { kind: "timer", label: "", ms: 150_000 });
  assert.equal(parseQuick("", NOW), null);
  assert.equal(parseQuick("tea", NOW), null);
  assert.equal(parseQuick("30h", NOW), null);
});

test("a time of day is a reminder, tomorrow when it has gone", () => {
  const at = (h, m, day = 7) => new Date(2026, 10, day, h, m).getTime();
  assert.deepEqual(parseQuick("18:30 call mum", NOW), { kind: "reminder", label: "call mum", due: at(18, 30) });
  assert.deepEqual(parseQuick("call mum at 6pm", NOW), { kind: "reminder", label: "call mum", due: at(18, 0) });
  assert.deepEqual(parseQuick("18h30 dentiste", NOW), { kind: "reminder", label: "dentiste", due: at(18, 30) });
  assert.deepEqual(parseQuick("9:15 stand-up", NOW), { kind: "reminder", label: "stand-up", due: at(9, 15, 8) });
  assert.deepEqual(parseQuick("12am", NOW), { kind: "reminder", label: "", due: at(0, 0, 8) });
  assert.equal(parseQuick("25:00", NOW), null);
});

test("what is left, how far along, and in which order", () => {
  const now = 1_000_000;
  const timer = { id: "a", kind: "timer", label: "", due: now + 60_000, duration: 240_000, repeat: "", ringing: false, phase: "", round: 0, focusMs: 0, breakMs: 0 };
  assert.equal(msLeft(timer, now), 60_000);
  assert.equal(progress(timer, now), 0.75);
  assert.equal(msLeft({ ...timer, due: 0, left: 5000 }, now), 5000);
  assert.equal(progress({ ...timer, kind: "reminder" }, now), 0);
  assert.equal(clock(59_200), "1:00");
  assert.equal(clock(3_723_000), "1:02:03");
  assert.equal(clock(7000), "0:07");
  const rang = { ...timer, id: "r", ringing: true, due: now - 1 };
  const later = { ...timer, id: "l", due: now + 600_000 };
  assert.deepEqual(ordered([later, timer, rang], now).map((t) => t.id), ["r", "a", "l"]);
});

// ── Weather ───────────────────────────────────────────────────────────────────

test("WMO codes become a kind of sky, and the sky's theme when it matters", () => {
  assert.equal(skyOf(0), "clear");
  assert.equal(skyOf(2), "partly");
  assert.equal(skyOf(45), "fog");
  assert.equal(skyOf(53), "drizzle");
  assert.equal(skyOf(81), "rain");
  assert.equal(skyOf(73), "snow");
  assert.equal(skyOf(96), "storm");
  assert.equal(themeForWeather(0), null);
  assert.equal(themeForWeather(2), null);
  assert.equal(themeForWeather(3), "cloudy");
  assert.equal(themeForWeather(61), "cloudy");
  assert.equal(themeForWeather(65), "storm");
  assert.equal(themeForWeather(95), "storm");
});

test("°F where the region writes them, and the units follow", () => {
  assert.equal(unitsFor("auto", ["en-US"]), "f");
  assert.equal(unitsFor("auto", ["en-GB", "en-US"]), "c");
  assert.equal(unitsFor("auto", ["fr"]), "c");
  assert.equal(unitsFor("c", ["en-US"]), "c");
  assert.equal(temp(20, "f"), 68);
  assert.equal(temp(-0.4, "c"), -0);
  assert.deepEqual(wind(16.09, "f"), { value: 10, unit: "mph" });
});

test("a place picked sets automatic units from its country, never chosen ones", () => {
  assert.deepEqual(unitsOnPick("auto", "CY"), { weatherUnits: "c" });
  assert.deepEqual(unitsOnPick("auto", "us"), { weatherUnits: "f" });
  assert.deepEqual(unitsOnPick("auto", ""), {});
  assert.deepEqual(unitsOnPick("f", "CY"), {});
});

test("what is said about rain: when it starts, when it stops", () => {
  const base = { place: "Lyon", lat: 0, lon: 0, at: 0, now: {}, hours: [], days: [], rainAt: null, raining: false, rainUntil: null };
  const now = 1_000_000;
  assert.equal(rainLine(base, now), null);
  assert.deepEqual(rainLine({ ...base, rainAt: now + 20 * 60_000 }, now), { key: "Rain in {minutes} min", minutes: 20 });
  assert.deepEqual(rainLine({ ...base, rainAt: now + 30_000 }, now), { key: "Rain starting now", minutes: 0 });
  assert.deepEqual(rainLine({ ...base, raining: true, rainUntil: now + 9 * 60_000 }, now), { key: "Rain stops in {minutes} min", minutes: 9 });
  assert.deepEqual(rainLine({ ...base, raining: true }, now), { key: "Rain for a while", minutes: 0 });
});

// ── Lyrics ────────────────────────────────────────────────────────────────────

test("timed lyrics read in order, and the line sung is the last begun", () => {
  const lines = parseLrc("[ar:Someone]\n[00:12.50]Second\n[00:01.00][00:30.00]Chorus\n[00:05]First\nno stamp");
  assert.deepEqual(lines.map((l) => [l.t, l.text]), [[1, "Chorus"], [5, "First"], [12.5, "Second"], [30, "Chorus"]]);
  assert.equal(lineAt(lines, 0.2), -1);
  assert.equal(lineAt(lines, 1), 0);
  assert.equal(lineAt(lines, 11), 1);
  assert.equal(lineAt(lines, 12.4), 2);
  assert.equal(lineAt(lines, 999), 3);
});

// ── Live activities ───────────────────────────────────────────────────────────

test("the folded island shows what matters most, and only a timer holds it", () => {
  const now = 1_000_000;
  const timer = (over) => ({ id: "t", kind: "timer", label: "", due: now + 120_000, duration: 300_000, repeat: "", ringing: false, phase: "", round: 0, focusMs: 0, breakMs: 0, ...over });
  const controls = (over) => ({ volume: 0.5, muted: false, micMuted: false, mics: 1, brightness: null, wifi: null, bluetooth: null, nightLight: null, awake: false, awakeUntil: null, ...over });
  const rainy = { rainAt: now + 10 * 60_000, raining: false };
  const none = { timers: [], controls: null, weather: null, now };
  assert.equal(pickLive(none), null);
  assert.deepEqual(pickLive({ ...none, lyric: "la la" }), { kind: "lyric", text: "la la" });
  assert.equal(pickLive({ ...none, controls: controls({ awake: true }), lyric: "la" }).kind, "awake");
  assert.equal(pickLive({ ...none, weather: rainy, controls: controls({ awake: true }) }).kind, "rain");
  assert.equal(pickLive({ ...none, timers: [timer()], weather: rainy }).kind, "timer");
  assert.equal(pickLive({ ...none, timers: [timer(), timer({ id: "r", ringing: true })] }).kind, "rang");
  assert.equal(pickLive({ ...none, timers: [timer({ ringing: true })], controls: controls({ micMuted: true }) }).kind, "mic");
  // A timer far off, or a reminder, doesn't take the clock's place.
  assert.equal(pickLive({ ...none, timers: [timer({ due: now + 3 * 3_600_000 })] }), null);
  assert.equal(pickLive({ ...none, timers: [timer({ kind: "reminder" })] }), null);
  // A paused one shows, without ticking.
  const paused = pickLive({ ...none, timers: [timer({ due: 0, left: 9 * 3_600_000 })] });
  assert.equal(paused.kind, "timer");
  assert.equal(ticks(paused), false);
  assert.equal(holdsIsland(pickLive({ ...none, timers: [timer()] })), true);
  assert.equal(holdsIsland(pickLive({ ...none, controls: controls({ micMuted: true }) })), false);
  assert.equal(holdsIsland(null), false);
});

// ── Ciel without a coding tool ────────────────────────────────────────────────

test("the main pill can be none: nothing is always on, everything else still works", () => {
  const d = { mainPill: NO_MAIN_PILL, activeIntegrations: ["integration_spotify", "integration_system"] };
  assert.deepEqual(sanitizeDeclared(d), d);
  // An unknown main pill still falls back to the default; "" is not unknown.
  assert.equal(sanitizeDeclared({ mainPill: "nope", activeIntegrations: [] }).mainPill, DEFAULT_MAIN_PILL);
  assert.deepEqual(chooseMainPill({ mainPill: DEFAULT_MAIN_PILL, activeIntegrations: [] }, NO_MAIN_PILL), { mainPill: "", activeIntegrations: [] });
  assert.deepEqual(toggleDeclared(d, "integration_github"), [...d.activeIntegrations, "integration_github"]);
  assert.deepEqual(orderPills([{ id: "integration_system" }, { id: "integration_spotify" }], NO_MAIN_PILL).map((t) => t.id), [
    "integration_spotify", "integration_system",
  ]);

  const saved = { ...State.settings };
  try {
    State.tasks = [];
    State.focusId = null;
    State.settings = { ...State.settings, mainPill: NO_MAIN_PILL, activeIntegrations: [] };
    State.loadIntegrationTasks();
    assert.deepEqual(State.tasks, []);
    assert.equal(State.focusId, null);
    assert.equal(State.defaultView(), "empty");
    State.settings.activeIntegrations = ["integration_system"];
    State.loadIntegrationTasks();
    assert.deepEqual(State.tasks.map((t) => t.id), ["integration_system"]);
    assert.equal(State.focusId, "integration_system");
    assert.equal(State.isKept("integration_system"), true);
  } finally {
    State.settings = saved;
    State.tasks = [];
    State.focusId = null;
    State.loadIntegrationTasks();
  }
});
