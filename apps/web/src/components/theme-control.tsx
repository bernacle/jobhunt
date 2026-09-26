"use client";

import { useState } from "react";

import { THEME_COOKIE, type ThemeChoice } from "@/lib/theme";

const CHOICES: { value: ThemeChoice; label: string }[] = [
  { value: "system", label: "System" },
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
];

/** Pins the theme on <html> at once, and remembers it for the server. */
function applyTheme(value: ThemeChoice) {
  const root = document.documentElement;
  if (value === "system") {
    root.removeAttribute("data-theme");
    document.cookie = `${THEME_COOKIE}=; path=/; max-age=0; samesite=lax`;
  } else {
    root.setAttribute("data-theme", value);
    document.cookie = `${THEME_COOKIE}=${value}; path=/; max-age=31536000; samesite=lax`;
  }
}

/** Follow the system, or pin dark or light. Applies at once. */
export function ThemeControl({ initial }: { initial: ThemeChoice }) {
  const [theme, setTheme] = useState(initial);
  const choose = (value: ThemeChoice) => {
    setTheme(value);
    applyTheme(value);
  };
  return (
    <fieldset>
      <legend className="sr-only">Theme</legend>
      <div className="inline-flex gap-0.5 rounded-md border border-line p-0.5">
        {CHOICES.map((c) => (
          <label
            key={c.value}
            className={`flex h-[26px] cursor-pointer items-center rounded-[4px] px-2.5 text-[12px] font-medium transition-colors duration-[120ms] has-[:focus-visible]:outline has-[:focus-visible]:outline-[1.5px] has-[:focus-visible]:outline-offset-2 has-[:focus-visible]:outline-[var(--nr-focus)] max-sm:h-10 max-sm:px-4 ${
              theme === c.value ? "bg-selected text-fg" : "text-fg-secondary hover:text-fg"
            }`}
          >
            <input type="radio" name="theme" value={c.value} checked={theme === c.value} onChange={() => choose(c.value)} className="sr-only" />
            {c.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}
