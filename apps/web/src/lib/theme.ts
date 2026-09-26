/**
 * Appearance: follow the system (the default), or pin dark or light. The
 * choice is a plain cookie so the server renders the right theme on the
 * first paint; it holds nothing private.
 */
export const THEME_COOKIE = "narrow_theme";

export type ThemeChoice = "system" | "dark" | "light";

export function parseTheme(value: string | undefined): ThemeChoice {
  return value === "dark" || value === "light" ? value : "system";
}
