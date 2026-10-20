// The YouTube Music pill: a browser tab's player, read there by Ciel for
// Chrome (extension/media.js) and reported by src-tauri/src/ytmusic.rs in
// Spotify's shape, shown by the same card and pill (src/views/spotify.ts)
// through its source (src/core/player.ts). The tab and the bridge are tested
// in ytmusic.rs and hook/src/chrome.rs.

import { test } from "node:test";
import assert from "node:assert/strict";
import { emit, sent } from "./tauri.mjs";
import { installFakeDom } from "./fakedom.mjs";
import { IDLE_SPOTIFY, Spotify } from "../src/core/spotify.ts";
import { YTMUSIC_ID, YtMusic, ytCover, ytIdle } from "../src/core/ytmusic.ts";
import { YTMUSIC_PLAYER, playerFor, SPOTIFY_PLAYER } from "../src/core/player.ts";
import { registerSpotifyHandlers } from "../src/island/spotify.ts";
import { buildPlayerCard } from "../src/views/spotify.ts";
import { DEFAULT_SETTINGS, State } from "../src/core/state.ts";
import { setLanguage } from "../src/i18n/i18n.ts";

installFakeDom();
setLanguage("en");

const COVER = "data:image/jpeg;base64,AAAA";

/** A tab playing, as ytmusic.rs reports it. */
const playing = (over = {}) => ({
  ...IDLE_SPOTIFY, running: true, installed: true, playing: true, position: 30, positionAt: 1_000,
  track: { id: "ytm:Song", title: "Midnight City", artist: "M83", album: "Hurry Up, We're Dreaming", duration: 244, artUrl: COVER },
  player: "", spotify: false, canSeek: true, canShuffle: false, canRepeat: false, hasVolume: false, volume: 0,
  ...over,
});

test("the music pills: each has its player, other pills none", () => {
  assert.equal(playerFor(YTMUSIC_ID), YTMUSIC_PLAYER);
  assert.equal(playerFor("integration_spotify"), SPOTIFY_PLAYER);
  assert.equal(playerFor("integration_github"), null);
});

test("the cover comes with the track, and only a data URL is one", () => {
  assert.equal(ytCover(playing()), COVER);
  assert.equal(ytCover(playing({ track: { ...playing().track, artUrl: "https://lh3.example/x.jpg" } })), null);
  assert.equal(ytCover(IDLE_SPOTIFY), null);
});

test("the idle card: playing nothing, no tab open, or no extension to read it", () => {
  assert.equal(ytIdle({ ...IDLE_SPOTIFY, running: true, installed: true }), "notPlaying");
  assert.equal(ytIdle({ ...IDLE_SPOTIFY, installed: true }), "notOpen");
  assert.equal(ytIdle({ ...IDLE_SPOTIFY }), "noExtension");

  const card = buildPlayerCard(YTMUSIC_PLAYER);
  YtMusic.state = { ...IDLE_SPOTIFY };
  card.sync();
  assert.match(card.el.textContent, /YouTube Music.*Needs Ciel for Chrome.*Set it up/);
  card.el.find("BUTTON")[0].fire("click");
  assert.ok(sent("open_settings_window").length > 0, "the bridge is set up in Settings");

  YtMusic.state = { ...IDLE_SPOTIFY, installed: true };
  card.sync();
  assert.match(card.el.textContent, /Not open.*Open YouTube Music/);
  card.el.find("BUTTON")[0].fire("click");
  assert.deepEqual(sent("open_url").at(-1), { url: "https://music.youtube.com/" });
});

test("the playing card: the tab's track, its own red, and only what a tab takes", () => {
  const card = buildPlayerCard(YTMUSIC_PLAYER);
  YtMusic.state = playing({ playing: false });
  card.sync();
  const text = card.el.textContent;
  assert.ok(text.includes("Midnight City"));
  assert.ok(text.includes("M83 · Hurry Up, We're Dreaming"), "artist and album, no player name added");
  const [shuffle, prev, play, next, repeat] = card.el.querySelector("np-buttons").children;
  assert.equal(shuffle.style.display, "none");
  assert.equal(repeat.style.display, "none");

  const before = sent("ytmusic_control").length;
  const spotifyBefore = sent("spotify_control").length;
  prev.fire("click");
  next.fire("click");
  play.fire("click");
  assert.deepEqual(sent("ytmusic_control").slice(before), [
    { action: "previous", value: null },
    { action: "next", value: null },
    { action: "playPause", value: null },
  ]);
  assert.equal(YtMusic.state.playing, true, "the page shows the click at once");
  assert.equal(sent("spotify_control").length, spotifyBefore, "nothing goes to Spotify");
});

test("a tab's report reaches the pill: its name is the track's, and Ciel dances to it", () => {
  State.settings = { ...DEFAULT_SETTINGS, activeIntegrations: [YTMUSIC_ID] };
  State.loadIntegrationTasks();
  registerSpotifyHandlers({ revealSilently() {} });
  Spotify.state = { ...IDLE_SPOTIFY };
  emit("ytmusic", playing());
  assert.equal(YtMusic.state.track.title, "Midnight City");
  assert.equal(State.tasks.find((t) => t.id === YTMUSIC_ID)?.name, "Midnight City");
  assert.equal(State.spotifyPlaying, true);
  emit("ytmusic", { ...IDLE_SPOTIFY, installed: true });
  assert.equal(State.tasks.find((t) => t.id === YTMUSIC_ID)?.name, "YouTube Music");
  assert.equal(State.spotifyPlaying, false);
  // Not declared: it plays, but Ciel has no music pill to dance to.
  State.settings = { ...DEFAULT_SETTINGS, activeIntegrations: [] };
  emit("ytmusic", playing());
  assert.equal(State.spotifyPlaying, false);
});
