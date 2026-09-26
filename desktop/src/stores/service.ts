import { create } from "zustand";

/** The latest "Modrinth is being retried" notice for a request outside any task (search, project pages). */
export interface ServiceNotice {
  reason: string;
  attempt: number;
  maxAttempts: number;
  at: number;
}

interface ServiceState {
  notice: ServiceNotice | null;
  report: (notice: Omit<ServiceNotice, "at">) => void;
  clear: () => void;
}

/**
 * How long a retry notice stays relevant: one full attempt (Modrinth's 30s
 * request timeout) plus backoff. Older notices belong to a request that has
 * long since succeeded or failed, so they expire on their own.
 */
export const NOTICE_TTL_MS = 35_000;

export const useService = create<ServiceState>((set, get) => ({
  notice: null,
  report: (notice) => {
    const at = Date.now();
    set({ notice: { ...notice, at } });
    setTimeout(() => {
      if (get().notice?.at === at) set({ notice: null });
    }, NOTICE_TTL_MS);
  },
  clear: () => set({ notice: null }),
}));
