// Black, white, or as the system: the Ciel window's colours and the island's.
// Each page's stylesheet does the rest from `data-theme` on its root.

export type Theme = "black" | "white" | "system";

let query: MediaQueryList | null = null;

export function applyTheme(theme: Theme) {
  query ??= window.matchMedia("(prefers-color-scheme: light)");
  const set = () => {
    const white = theme === "white" || (theme === "system" && query!.matches);
    document.documentElement.dataset.theme = white ? "white" : "black";
    // settings.html paints the page black until this runs.
    document.documentElement.style.removeProperty("background");
  };
  query.onchange = theme === "system" ? set : null;
  set();
}

/** True while this page is drawn on white. */
export const onWhite = () => document.documentElement.dataset.theme === "white";
