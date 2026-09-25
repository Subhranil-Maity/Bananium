import { create } from "zustand";

/** "banana" is the default light theme with a yellow accent; "dark" is its dark twin. */
export type Theme = "banana" | "dark";

const STORAGE_KEY = "bananium.theme";

function load(): Theme {
  try {
    return localStorage.getItem(STORAGE_KEY) === "dark" ? "dark" : "banana";
  } catch {
    return "banana";
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
