import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** Merge Tailwind class lists, letting later classes override earlier ones. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Human-readable byte count (`1536` -> `"1.5 KB"`); mirrors the CLI's `format_bytes`. */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

/** Total playtime: `"41h 12m"`, `"12m"`, or `"—"` for never. */
export function formatPlaytime(secs: number): string {
  if (secs < 60) return secs > 0 ? "<1m" : "—";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
}

const RELATIVE = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
const STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["year", 365 * 86400],
  ["month", 30 * 86400],
  ["week", 7 * 86400],
  ["day", 86400],
  ["hour", 3600],
  ["minute", 60],
];

/** `"3 days ago"`, `"just now"`, or `"Never"` for a missing timestamp. */
export function formatRelative(unix: number | null | undefined): string {
  if (!unix) return "Never";
  const diff = unix - Date.now() / 1000;
  for (const [unit, secs] of STEPS) {
    if (Math.abs(diff) >= secs) return RELATIVE.format(Math.round(diff / secs), unit);
  }
  return "just now";
}
