import { create } from "zustand";

export type InstanceDialogKind = "rename" | "clone" | "delete" | "group" | "dry-run";

interface InstanceDialogsState {
  /** The one instance dialog currently open, if any. */
  open: { kind: InstanceDialogKind; slug: string } | null;
  show: (kind: InstanceDialogKind, slug: string) => void;
  close: () => void;
}

/**
 * Instance dialogs are hosted once in the app shell, so any menu (context
 * menu, "…" dropdown, command palette) can open them without mounting its
 * own copy.
 */
export const useInstanceDialogs = create<InstanceDialogsState>((set) => ({
  open: null,
  show: (kind, slug) => set({ open: { kind, slug } }),
  close: () => set({ open: null }),
}));
