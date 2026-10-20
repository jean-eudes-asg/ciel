// Spotify's reports → the page (src-tauri/src/spotify.rs). The parts of
// SpotifyController.swift that touch the app: the pill wears the track's
// title, music starting shows the compact island without a sound, and the
// card reads the player again whenever it comes on screen (seeks made in
// Spotify's own window are never signalled).

import { Bridge, onEvent } from "../core/bridge";
import { pillDefinition } from "../core/pills";
import { SPOTIFY_ID, Spotify, isAd, type SpotifyState } from "../core/spotify";
import { YTMUSIC_ID, YtMusic } from "../core/ytmusic";
import { State } from "../core/state";
import { t } from "../i18n/i18n";

export interface SpotifyHost {
  /** Compact island from hidden, no peek sound. */
  revealSilently(): void;
}

export function registerSpotifyHandlers(island: SpotifyHost) {
  void onEvent<SpotifyState>("spotify", (s) => applySpotify(island, s));
  void onEvent<{ artUrl: string; dataUrl: string }>("spotify-artwork", (art) => {
    Spotify.artwork = art;
    State.notify();
  });
  // YouTube Music, from a browser tab through Ciel for Chrome (ytmusic.rs).
  void onEvent<SpotifyState>("ytmusic", (s) => applyYtMusic(island, s));
  void Bridge.ytMusicState().then((s) => {
    if (s) applyYtMusic(island, s);
  });
  State.subscribe(() => {
    syncPillName();
    refreshWhenShown();
  });
}

/** A report from Rust (or the answer to a refresh). */
export function applySpotify(island: SpotifyHost, next: SpotifyState) {
  const wasPlaying = State.spotifyPlaying;
  Spotify.state = next;
  syncPillName();
  // Only on not playing → playing (SpotifyController.setPlaying).
  if (!wasPlaying && State.spotifyPlaying && !State.paused && State.mode === "hidden") {
    island.revealSilently();
  }
  State.notify();
}

/** YouTube Music's player: as Spotify's, music starting shows the compact island silently. */
export function applyYtMusic(island: SpotifyHost, next: SpotifyState) {
  const wasPlaying = State.spotifyPlaying;
  YtMusic.state = next;
  syncPillName();
  if (!wasPlaying && State.spotifyPlaying && !State.paused && State.mode === "hidden") {
    island.revealSilently();
  }
  State.notify();
}

/** SpotifyController.syncTaskName: each music pill wears its track's title, else its own name. */
export function syncPillName() {
  const players: [string, SpotifyState][] = [[SPOTIFY_ID, Spotify.state], [YTMUSIC_ID, YtMusic.state]];
  for (const [id, s] of players) {
    const task = State.tasks.find((x) => x.id === id);
    if (!task) continue;
    const title = isAd(s.track) ? t("Advertisement") : (s.track?.title ?? "");
    const name = title || (pillDefinition(id)?.name ?? task.name);
    if (task.name !== name) task.name = name;
  }
}

let shown = false;

function refreshWhenShown() {
  const now = State.mode === "expanded" && State.view === "overview" && State.focusTask?.id === SPOTIFY_ID;
  if (now && !shown) void Bridge.spotifyRefresh();
  shown = now;
}
