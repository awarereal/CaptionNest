// Interface themes (Settings > Appearance). The colours live in styles/tokens.css under
// [data-theme="…"]; Rust (src-tauri/src/theme.rs) matches the native window to them.
// Captions in the video are styled separately (captionStyle.ts) and never change with
// the theme.

export type ThemeId = "system" | "tungsten" | "daylight" | "midnight" | "contrast";
export type ConcreteTheme = Exclude<ThemeId, "system">;

export interface ThemeInfo {
  id: ThemeId;
  name: string;
  description: string;
}

export const DEFAULT_THEME: ConcreteTheme = "tungsten";

export const THEMES: ThemeInfo[] = [
  { id: "system", name: "Match this computer", description: "Light or dark, following your computer's setting" },
  { id: "tungsten", name: "Tungsten", description: "Warm dark with gold. The original" },
  { id: "midnight", name: "Midnight", description: "Deep blue, easy on the eyes at night" },
  { id: "daylight", name: "Daylight", description: "Light, for bright rooms" },
  { id: "contrast", name: "High contrast", description: "Black, white and yellow; strongest edges" },
];

const lightQuery = () => window.matchMedia("(prefers-color-scheme: light)");

export function isThemeId(id: unknown): id is ThemeId {
  return THEMES.some((t) => t.id === id);
}

/** The theme actually shown: "system" becomes Daylight or Tungsten. */
export function resolveTheme(id: ThemeId): ConcreteTheme {
  if (id !== "system") return id;
  return lightQuery().matches ? "daylight" : "tungsten";
}

let chosen: ThemeId = DEFAULT_THEME;
let watching = false;

/** Shows a theme from the settings (unknown ids, e.g. from a newer version, show the default). */
export function applyTheme(id: string | null | undefined) {
  chosen = isThemeId(id) ? id : DEFAULT_THEME;
  document.documentElement.dataset.theme = resolveTheme(chosen);
  if (!watching) {
    watching = true;
    // "Match this computer" follows the computer when it switches between light and dark.
    lightQuery().addEventListener("change", () => {
      if (chosen === "system") document.documentElement.dataset.theme = resolveTheme("system");
    });
  }
}
