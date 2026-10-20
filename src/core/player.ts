// The music pills the island shows and plays from: Spotify (spotify.rs) and
// YouTube Music (ytmusic.rs, through Ciel for Chrome). They share the card and
// the pill (views/spotify.ts), and both speak Spotify's shape; this is what
// differs between them — where the state lives, where the buttons go, what
// the cover and the idle card offer.

import { Bridge, type SpotifyAction } from "./bridge";
import { SPOTIFY_GREEN, SPOTIFY_ID, Spotify, currentArtwork, type SpotifyState } from "./spotify";
import { YTMUSIC_ID, YTMUSIC_RED, YTMUSIC_URL, YtMusic, ytCover, ytIdle } from "./ytmusic";
import { t } from "../i18n/i18n";

/** The card when nothing plays: a dot, a line, a button. */
export interface IdleCard {
  ok: boolean;
  status: string;
  action: string;
  act(): void;
}

export interface PlayerSource {
  /** Its pill's id. */
  readonly id: string;
  readonly name: string;
  readonly color: string;
  /** The page's copy, changed at once by a button before the player confirms it. */
  state: SpotifyState;
  cover(): string | null;
  control(action: SpotifyAction, value?: number): void;
  /** The cover's tooltip: what a press on it does, "" when nothing. */
  coverTitle(): string;
  openFromCover(): void;
  idle(): IdleCard;
}

export const SPOTIFY_PLAYER: PlayerSource = {
  id: SPOTIFY_ID,
  name: "Spotify",
  color: SPOTIFY_GREEN,
  get state() {
    return Spotify.state;
  },
  set state(s: SpotifyState) {
    Spotify.state = s;
  },
  cover: () => currentArtwork(Spotify.state),
  control: (action, value) => void Bridge.spotifyControl(action, value),
  coverTitle() {
    const s = Spotify.state;
    // Only Spotify can be brought forward from here.
    if (!s.spotify || !s.track) return "";
    return s.track.album ? t("{0} — open Spotify", { 0: s.track.album }) : t("Open Spotify");
  },
  openFromCover() {
    if (Spotify.state.spotify) void Bridge.spotifyOpen();
  },
  idle() {
    const installed = Spotify.state.installed || Spotify.state.running;
    return {
      ok: installed,
      status: installed ? t("Not playing") : t("Spotify not installed"),
      action: installed ? t("Open Spotify") : t("Get Spotify"),
      act: () => void Bridge.spotifyOpen(),
    };
  },
};

export const YTMUSIC_PLAYER: PlayerSource = {
  id: YTMUSIC_ID,
  name: "YouTube Music",
  color: YTMUSIC_RED,
  get state() {
    return YtMusic.state;
  },
  set state(s: SpotifyState) {
    YtMusic.state = s;
  },
  cover: () => ytCover(YtMusic.state),
  control(action, value) {
    // A tab takes what the media keys do, and a seek; shuffle, repeat and
    // volume stay YouTube Music's own (the card doesn't offer them).
    if (action === "playPause" || action === "next" || action === "previous" || action === "seek") {
      void Bridge.ytMusicControl(action, value);
    }
  },
  coverTitle: () => t("Open YouTube Music"),
  openFromCover: () => void Bridge.ytMusicControl("focus"),
  idle() {
    switch (ytIdle(YtMusic.state)) {
      case "notPlaying":
        return { ok: true, status: t("Not playing"), action: t("Open YouTube Music"), act: () => void Bridge.ytMusicControl("focus") };
      case "notOpen":
        return { ok: true, status: t("Not open"), action: t("Open YouTube Music"), act: () => void Bridge.openUrl(YTMUSIC_URL) };
      case "noExtension":
        // The bridge is set up in Settings → Integrations → Notifications.
        return { ok: false, status: t("Needs Ciel for Chrome"), action: t("Set it up"), act: () => void Bridge.openSettingsWindow() };
    }
  },
};

const PLAYERS = [SPOTIFY_PLAYER, YTMUSIC_PLAYER];

/** The player behind a pill, if it is a music pill. */
export function playerFor(id: string): PlayerSource | null {
  return PLAYERS.find((p) => p.id === id) ?? null;
}
