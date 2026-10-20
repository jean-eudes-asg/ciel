// The open island's home in each layout, in a plain browser (`npx vite`,
// /dev/overview-preview.html): one pill in detail across it, two side by side
// in detail, more as detail and list. Each case sets the state, then draws the
// real overview once.

import "../src/style.css";
import { State } from "../src/core/state";
import { IDLE_SPOTIFY, Spotify } from "../src/core/spotify";
import { YtMusic } from "../src/core/ytmusic";
import { makeDiffStep } from "../src/core/diff";
import { buildViews, type ViewActions, type ViewHost } from "../src/views/views";

/** A cover: two colours, as the extension and Spotify send them. */
function cover(a: string, b: string): string {
  const c = document.createElement("canvas");
  c.width = c.height = 120;
  const g = c.getContext("2d")!;
  const grad = g.createLinearGradient(0, 0, 120, 120);
  grad.addColorStop(0, a);
  grad.addColorStop(1, b);
  g.fillStyle = grad;
  g.fillRect(0, 0, 120, 120);
  return c.toDataURL("image/jpeg");
}

const actions = new Proxy({}, { get: () => () => {} }) as unknown as ViewActions;
const hosts: ViewHost[] = [];

function pills(main: string, others: string[]) {
  State.settings = { ...State.settings, mainPill: main, activeIntegrations: others };
  State.loadIntegrationTasks();
  State.focusId = main;
}

/** Claude at work: a feed, three files edited, its plan's usage. */
function claudeAtWork() {
  const task = State.tasks.find((t) => t.id === "integration_claude")!;
  const diff = (path: string, added: number, removed: number) =>
    State.appendSessionDiff(task.id, { id: 0, path, added, removed, hunks: [], tooLarge: false, isNewFile: false });
  const a = diff("src/core/overview.ts", 48, 0);
  diff("src/views/views.ts", 212, 96);
  const c = diff("src/style.css", 131, 2);
  task.state = "working";
  task.steps = ["Read src/views/views.ts", makeDiffStep("overview.ts", 48, 0, a), makeDiffStep("style.css", 131, 2, c), "Running npm test"];
  task.stepIndex = 3;
  const now = Date.now();
  State.planUsage = { fiveHour: { usedPct: 42, resetsAt: now + 2.4 * 3_600_000 }, sevenDay: { usedPct: 18, resetsAt: now + 3.2 * 86_400_000 }, updatedAt: now };
}

function spotifyPlaying() {
  const art = cover("#1DB954", "#0b3d1f");
  Spotify.state = {
    ...IDLE_SPOTIFY, running: true, installed: true, playing: true, position: 74, positionAt: Date.now(), volume: 62,
    track: { id: "spotify:track:x", title: "Get Lucky", artist: "Daft Punk", album: "Random Access Memories", duration: 369, artUrl: "art:1" },
    player: "Spotify", spotify: true, canSeek: true, canShuffle: true, canRepeat: true, hasVolume: true, shuffle: true,
  };
  Spotify.artwork = { artUrl: "art:1", dataUrl: art };
}

function ytMusicPlaying() {
  YtMusic.state = {
    ...IDLE_SPOTIFY, running: true, installed: true, playing: true, position: 101, positionAt: Date.now(),
    track: { id: "ytm:x", title: "Midnight City", artist: "M83", album: "Hurry Up, We're Dreaming", duration: 244, artUrl: cover("#FF0033", "#3a0010") },
    canSeek: true,
  };
}

function gmailInbox() {
  const mails = [
    ["GitHub", "[ciel] CI passed on main"], ["Stripe", "Your weekly summary"], ["Léa Martin", "Re: the website — love the new sky!"],
    ["Vercel", "Deployment ready: ciel-site"], ["Linear", "3 issues assigned to you"], ["Figma", "Comments on Ciel icon v4"],
  ].map(([from, subject]) => ({ from, subject, url: "https://mail.google.com/" }));
  State.integrations["integration_gmail"] = { loaded: true, configured: true, error: null, data: { unread: 7, mails } };
}

/** The Calendar pill: one meeting under way, the next in a few minutes, then later ones. */
function calendarDay() {
  const now = Date.now();
  const at = (min: number) => now + min * 60_000;
  const events = [
    { id: "a", title: "Stand-up", start: at(-5), end: at(10), allDay: false, join: "https://meet.google.com/abc-defg-hij" },
    { id: "b", title: "Q4 budget review with finance", start: at(12), end: at(42), allDay: false, join: "https://us02web.zoom.us/j/81234" },
    { id: "c", title: "Design critique", start: at(150), end: at(210), allDay: false, where: "Room 4" },
    { id: "d", title: "Offsite", start: at(60 * 24), end: at(60 * 48), allDay: true },
    { id: "e", title: "1:1 with Ana", start: at(60 * 26), end: at(60 * 26 + 30), allDay: false },
    { id: "f", title: "Sprint planning", start: at(60 * 50), end: at(60 * 51), allDay: false },
  ];
  State.integrations["integration_calendar"] = { loaded: true, configured: true, error: null, data: { events, calendars: 2, problems: [] } };
}

const CASES: [string, () => void][] = [
  ["One pill: Claude at work — its feed, the files it edited, its plan", () => {
    pills("integration_claude", []);
    claudeAtWork();
  }],
  ["One pill: Gmail — six mails in two columns", () => {
    pills("integration_claude", ["integration_gmail"]);
    State.settings.mainPill = "integration_claude";
    // Gmail alone: the main pill is always there, so this one shows it on its own.
    State.tasks = State.tasks.filter((t) => t.id === "integration_gmail");
    State.focusId = "integration_gmail";
    gmailInbox();
  }],
  ["One pill: Calendar — under way, coming up, the next days", () => {
    pills("integration_claude", ["integration_calendar"]);
    State.tasks = State.tasks.filter((t) => t.id === "integration_calendar");
    State.focusId = "integration_calendar";
    calendarDay();
  }],
  ["Two pills: Claude at work, Calendar on the right", () => {
    pills("integration_claude", ["integration_calendar"]);
    claudeAtWork();
    calendarDay();
  }],
  ["One pill: Spotify", () => {
    pills("integration_claude", ["integration_spotify"]);
    State.tasks = State.tasks.filter((t) => t.id === "integration_spotify");
    State.focusId = "integration_spotify";
    spotifyPlaying();
  }],
  ["Two pills: Claude at work, Spotify on the right", () => {
    pills("integration_claude", ["integration_spotify"]);
    claudeAtWork();
    spotifyPlaying();
  }],
  ["Two pills: Claude idle, YouTube Music on the right (focused)", () => {
    pills("integration_claude", ["integration_youtube_music"]);
    State.focusId = "integration_youtube_music";
    ytMusicPlaying();
  }],
  ["Four pills: the focused one in detail, the others listed", () => {
    pills("integration_claude", ["integration_spotify", "integration_gmail", "integration_youtube_music"]);
    claudeAtWork();
    spotifyPlaying();
    gmailInbox();
  }],
];

State.mode = "expanded";
State.view = "overview";
const root = document.getElementById("cases")!;
for (const [label, setup] of CASES) {
  setup();
  const host = buildViews(actions, () => {}).get("overview")!;
  host.el.classList.add("on");
  host.sync();
  hosts.push(host);
  const open = document.createElement("div");
  open.className = "open";
  const head = document.createElement("div");
  head.className = "head";
  const body = document.createElement("div");
  body.className = "body";
  body.append(host.el);
  const ciel = document.createElement("i");
  ciel.className = "ciel";
  open.append(ciel, head, body);
  const box = document.createElement("div");
  box.className = "case";
  const caption = document.createElement("div");
  caption.textContent = label;
  box.append(caption, open);
  root.append(box);
}

// The feeds move on their own clock.
const frame = (now: number) => {
  for (const host of hosts) host.tick?.(now);
  requestAnimationFrame(frame);
};
requestAnimationFrame(frame);
