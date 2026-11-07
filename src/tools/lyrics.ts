// The words of the song playing, timed (src-tauri/src/lyrics.rs asks
// lrclib.net once the user turned lyrics on). The music card shows the line
// being sung under the title; the folded island, with nothing more urgent,
// sings it too while Ciel dances.

import { Bridge } from "../core/bridge";
import { State } from "../core/state";
import { lineAt, parseLrc, type LyricLine } from "../core/lyrics";
import { spotifyPosition, type SpotifyState } from "../core/spotify";
import { Tools } from "./store";

class LyricsStore {
  private key = "";
  lines: LyricLine[] = [];
  /** Words without times: nothing to follow, but the card can say there are some. */
  plain = false;
  instrumental = false;

  /** Asks for the track's words once, when lyrics are on. */
  want(s: SpotifyState) {
    const track = s.track;
    if (!Tools.prefs.lyrics || !track || !track.title) return;
    const key = `${track.title}\u001f${track.artist}`;
    if (key === this.key) return;
    this.key = key;
    this.lines = [];
    this.plain = false;
    this.instrumental = false;
    void Bridge.lyricsGet(track.title, track.artist, track.album, track.duration).then((found) => {
      if (this.key !== key) return;
      this.lines = found?.synced ? parseLrc(found.synced) : [];
      this.plain = !!found?.plain && !this.lines.length;
      this.instrumental = !!found?.instrumental;
      State.notify();
    });
  }

  /** The line being sung now, "" between lines, null with nothing to show. */
  current(s: SpotifyState, nowMs = Date.now()): string | null {
    if (!Tools.prefs.lyrics || !s.track || !this.lines.length) return null;
    if (this.key !== `${s.track.title}\u001f${s.track.artist}`) return null;
    const i = lineAt(this.lines, spotifyPosition(s, nowMs));
    if (i < 0) return "";
    return this.lines[i].text;
  }
}

export const Lyrics = new LyricsStore();
