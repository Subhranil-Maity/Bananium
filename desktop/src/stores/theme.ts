import { create } from "zustand";

/** Dark is the default; light is the optional alternative. */
export type Theme = "dark" | "light";

const STORAGE_KEY = "bananium.theme";

function load(): Theme {
  try {
    // "banana" is what older builds stored for the light theme.
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored === "light" || stored === "banana" ? "light" : "dark";
  } catch {
    return "dark";
  }
}

function applyToDocument(theme: Theme) {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

interface ThemeState {
  theme: Theme;
  setTheme: (theme: Theme) => void;
}

export const useTheme = create<ThemeState>((set) => {
  const initial = load();
  applyToDocument(initial);
  return {
    theme: initial,
    setTheme: (theme) => {
      applyToDocument(theme);
      try {
        localStorage.setItem(STORAGE_KEY, theme);
      } catch {
        // Storage can be unavailable; the theme still applies for this session.
      }
      set({ theme });
    },
  };
});
