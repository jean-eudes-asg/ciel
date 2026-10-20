// The YouTube Music pill on the page: what src-tauri/src/ytmusic.rs reports —
// the player in a music.youtube.com tab, read there by Ciel for Chrome — in
// Spotify's shape, so the same card and pill show it (views/spotify.ts).

import { IDLE_SPOTIFY, type SpotifyState } from "./spotify";

export const YTMUSIC_ID = "integration_youtube_music";
/** YouTube's red. */
export const YTMUSIC_RED = "#FF0033";
export const YTMUSIC_URL = "https://music.youtube.com/";

/** The page's copy of the player. `running`: a tab plays or has played; `installed`: the extension is there. */
export const YtMusic = {
  state: { ...IDLE_SPOTIFY } as SpotifyState,
};

/** The cover arrives with the track, made small by the extension. */
export function ytCover(s: SpotifyState): string | null {
  const art = s.track?.artUrl;
  return art && art.startsWith("data:image/") ? art : null;
}

/** What the idle card says: playing nothing, no tab, or no extension to read it. */
export function ytIdle(s: SpotifyState): "notPlaying" | "notOpen" | "noExtension" {
  if (s.running) return "notPlaying";
  return s.installed ? "notOpen" : "noExtension";
}
