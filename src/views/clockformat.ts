// 12 or 24 hours (Settings → General → Clock), for the flip clocks on screen
// (flipclock.ts) and the one drawn into the lock screen (flipclock-picture.ts).

/** "auto" follows the interface's language. */
export type ClockFormat = "auto" | "12h" | "24h";

/** 12 or 24 hours: as chosen, or as the language writes the hours. */
export function twelveHours(format: ClockFormat, lang: string | undefined): boolean {
  if (format !== "auto") return format === "12h";
  const cycle = new Intl.DateTimeFormat(lang, { hour: "numeric" }).resolvedOptions().hourCycle;
  return cycle === "h12" || cycle === "h11";
}

/** The hours (unpadded on 12 hours), the minutes, and AM / PM in the language on 12 hours. */
export function clockFace(now: Date, twelve: boolean, lang: string | undefined) {
  let h = now.getHours();
  if (twelve) h = h % 12 || 12;
  return {
    hours: twelve ? String(h) : String(h).padStart(2, "0"),
    minutes: String(now.getMinutes()).padStart(2, "0"),
    period: twelve
      ? new Intl.DateTimeFormat(lang, { hour: "numeric", hour12: true }).formatToParts(now).find((p) => p.type === "dayPeriod")?.value ?? ""
      : "",
  };
}
