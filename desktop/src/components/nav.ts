import { Compass, Image, Layers, LayoutGrid, Settings, Users } from "lucide-react";

/** Top-level pages, in rail order. `shortcut` is shown in the palette (Ctrl+1…). */
export const NAV = [
  { to: "/", label: "Library", icon: LayoutGrid, end: true, shortcut: "Ctrl+1" },
  { to: "/browse", label: "Browse Modrinth", icon: Compass, end: false, shortcut: "Ctrl+2" },
  { to: "/presets", label: "Presets", icon: Layers, end: false, shortcut: "Ctrl+3" },
  { to: "/screenshots", label: "Screenshots", icon: Image, end: false, shortcut: "Ctrl+4" },
  { to: "/accounts", label: "Accounts", icon: Users, end: false, shortcut: "Ctrl+5" },
  { to: "/settings", label: "Settings", icon: Settings, end: false, shortcut: "Ctrl+," },
] as const;
