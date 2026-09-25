import { create } from "zustand";

/** How the library lays out instances. */
export type LibraryView = "grid" | "list";
export type LibraryGroupBy = "none" | "group" | "loader" | "version";
export type LibrarySort = "played" | "name" | "version" | "playtime";

interface Prefs {
  view: LibraryView;
  groupBy: LibraryGroupBy;
  sort: LibrarySort;
  /** Library sections the user folded away, by section key. */
  collapsed: string[];
}

const STORAGE_KEY = "bananium.prefs";
const DEFAULTS: Prefs = { view: "grid", groupBy: "group", sort: "played", collapsed: [] };

function load(): Prefs {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Prefs>) } : DEFAULTS;
  } catch {
    return DEFAULTS;
  }
}

interface PrefsState extends Prefs {
  set: (patch: Partial<Prefs>) => void;
  toggleCollapsed: (key: string) => void;
}

/** Per-machine UI preferences; cosmetic, so losing them is harmless. */
export const usePrefs = create<PrefsState>((set, get) => {
  const persist = (patch: Partial<Prefs>) => {
    set(patch);
    const { view, groupBy, sort, collapsed } = get();
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ view, groupBy, sort, collapsed }));
    } catch {
      // Storage unavailable: preferences just last for this session.
    }
  };
  return {
    ...load(),
    set: persist,
    toggleCollapsed: (key) => {
      const { collapsed } = get();
      persist({ collapsed: collapsed.includes(key) ? collapsed.filter((k) => k !== key) : [...collapsed, key] });
    },
  };
});
