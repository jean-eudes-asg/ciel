// The audio of an answer (Kitten's WAV, ElevenLabs' or a voice server's MP3),
// as the pill may play it: the app's policy lets media come from blob: URLs
// only (tauri.conf.json, media-src), never from data: ones.

/** A blob: URL for base64 `audio` of type `mime`; revoke it once played. */
export function audioUrl(audio: string, mime: string): string {
  const bytes = Uint8Array.from(atob(audio), (c) => c.charCodeAt(0));
  return URL.createObjectURL(new Blob([bytes], { type: mime }));
}
