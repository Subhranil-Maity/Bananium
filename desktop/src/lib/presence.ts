// Discord Rich Presence from the webview's side: telling the backend which
// page the user is on. Everything else (connecting, composing the activity,
// rate limits) happens in `bananium-api`.
import { useEffect } from "react";

import type { LauncherView } from "@/bindings/LauncherView";
import { dispatch } from "@/lib/api";

/** Query key for the Discord connection status (kept fresh by events). */
export const PRESENCE_STATUS_KEY = ["presence-status"] as const;

// Pages nest (a project sheet opens over Browse), so views form a stack:
// the innermost mounted one is what's reported, and unmounting it falls back
// to the page underneath.
const stack: { id: number; view: LauncherView }[] = [];
let nextId = 1;
let timer: ReturnType<typeof setTimeout> | undefined;
let lastSent = "";

function flush() {
  clearTimeout(timer);
  // Debounced: route changes come in bursts (redirects, sheets opening).
  timer = setTimeout(() => {
    const view = stack.at(-1)?.view ?? { view: "other" };
    const key = JSON.stringify(view);
    if (key === lastSent) return;
    lastSent = key;
    dispatch({ command: "presence_set_view", view }).catch(() => {
      lastSent = "";
    });
  }, 400);
}

/**
 * Report `view` while the calling component is mounted (`null` reports
 * nothing). Re-reports when the view's contents change.
 */
export function usePresenceView(view: LauncherView | null) {
  const key = view ? JSON.stringify(view) : null;
  useEffect(() => {
    if (!key) return;
    const entry = { id: nextId++, view: JSON.parse(key) as LauncherView };
    stack.push(entry);
    flush();
    return () => {
      const i = stack.findIndex((e) => e.id === entry.id);
      if (i >= 0) stack.splice(i, 1);
      flush();
    };
  }, [key]);
}
