// Long answers, said a piece at a time (pill/main.ts): Chromium's speech
// synthesis stops a long utterance partway, the online voices sooner, so the
// pill says one stretch after another — whole sentences, joined while they
// stay short, and a very long sentence cut at a comma or a space.

const MAX = 220;

/** `text` in pieces of at most `max` characters, cut where a voice pauses anyway. */
export function speechChunks(text: string, max = MAX): string[] {
  const sentences = text.replace(/\s+/g, " ").trim().split(/(?<=[.!?…]["»”')\]]*)\s+/).filter(Boolean);
  const pieces: string[] = [];
  for (let s of sentences) {
    while (s.length > max) {
      const comma = Math.max(s.lastIndexOf(", ", max), s.lastIndexOf("; ", max));
      const space = s.lastIndexOf(" ", max);
      const at = comma > max / 3 ? comma + 1 : space > 0 ? space : max;
      pieces.push(s.slice(0, at).trim());
      s = s.slice(at).trim();
    }
    if (s) pieces.push(s);
  }
  // Short sentences go together: fewer breaks between them.
  const out: string[] = [];
  for (const p of pieces) {
    const last = out.length - 1;
    if (last >= 0 && out[last].length + 1 + p.length <= max) out[last] = `${out[last]} ${p}`;
    else out.push(p);
  }
  return out;
}
