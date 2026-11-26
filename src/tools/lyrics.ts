// The words of the song playing, timed (src-tauri/src/lyrics.rs asks
// lrclib.net once the user turned lyrics on). The music card shows the line
// being sung under the title; the folded island, with nothing more urgent,
// sings it too while Ciel dances.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import { lineAt, parseLrc, type LyricLine } from "../core/lyrics";
import { spotifyPosition, type SpotifyState } from "../core/spotify";
import { Tools } from "./store";

/** The song a player is on, as the words are kept: "" with none. */
function songOf(s: SpotifyState): string {
  const track = s.track;
  return track && track.title ? `${track.title}\u001f${track.artist}` : "";
}

/** The last few songs' words, each asked once: the card and the dock may follow two players at once. */
class LyricsStore {
  private found = new Map<string, LyricLine[]>();

  /** Asks for the track's words once, when lyrics are on. */
  want(s: SpotifyState) {
    const song = songOf(s);
    if (!Tools.prefs.lyrics || !song || this.found.has(song)) return;
    this.found.set(song, []);
    while (this.found.size > 8) this.found.delete(this.found.keys().next().value!);
    const track = s.track!;
    void Bridge.lyricsGet(track.title, track.artist, track.album, track.duration).then((found) => {
      const lines = found?.synced ? parseLrc(found.synced) : [];
      if (!lines.length) return;
      this.found.set(song, lines);
      State.notify();
    });
  }

  /** The track's timed words; none with lyrics off or not found. */
  timed(s: SpotifyState): LyricLine[] {
    const song = songOf(s);
    return Tools.prefs.lyrics && song ? this.found.get(song) ?? [] : [];
  }

  /** The line being sung now, "" between lines, null with nothing to show. */
  current(s: SpotifyState, nowMs = Date.now()): string | null {
    const lines = this.timed(s);
    if (!lines.length) return null;
    const i = lineAt(lines, spotifyPosition(s, nowMs));
    return i < 0 ? "" : lines[i].text;
  }
}

export const Lyrics = new LyricsStore();
