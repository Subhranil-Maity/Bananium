import type { ReactNode } from "react";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

/** A labelled form field with an optional right-aligned extra (e.g. a switch). */
export function Field({ label, children, aside }: { label: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <div className="space-y-1.5">
      <div className="flex h-5 items-center justify-between">
        <Label className="text-xs font-medium text-muted-foreground">{label}</Label>
        {aside}
      </div>
      {children}
    </div>
  );
}

/** Free-text group input with the existing groups as one-click chips. */
export function GroupPicker({
  value,
  onChange,
  groups,
}: {
  value: string;
  onChange: (group: string) => void;
  groups: string[];
}) {
  return (
    <>
      <Input placeholder="None" value={value} onChange={(e) => onChange(e.target.value)} />
      {groups.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {groups.map((g) => (
            <button
              key={g}
              type="button"
              className={cn(
                "rounded border px-1.5 py-0.5 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground",
                g === value && "border-primary/60 text-foreground",
              )}
              onClick={() => onChange(g === value ? "" : g)}
            >
              {g}
            </button>
          ))}
        </div>
      )}
    </>
  );
}

/**
 * `base` turned into a valid instance name (letters, digits, '-', '_') and
 * made unique against `taken` (lower-cased names) with a numeric suffix.
 * Mirrors the backend's own fallback naming for modpacks.
 */
export function uniqueInstanceName(base: string, taken: Set<string>): string {
  const clean =
    base
      .replace(/[^A-Za-z0-9_-]+/g, "-")
      .split("-")
      .filter(Boolean)
      .join("-") || "instance";
  if (!taken.has(clean.toLowerCase())) return clean;
  for (let n = 2; ; n++) if (!taken.has(`${clean}-${n}`.toLowerCase())) return `${clean}-${n}`;
}
