import type { InstanceSummary } from "@/bindings/InstanceSummary";
import type { LibraryGroupBy, LibrarySort } from "@/stores/prefs";

/** "Fabric 0.16.9" or "Vanilla". */
export function loaderLabel(i: InstanceSummary, withVersion = false): string {
  if (i.loader !== "fabric") return "Vanilla";
  return withVersion && i.loader_version ? `Fabric ${i.loader_version}` : "Fabric";
}

/**
 * Orders Minecraft ids numerically ("1.21.10" after "1.21.9"); anything
 * non-numeric (snapshots) falls back to plain string order after releases.
 */
export function compareMcVersions(a: string, b: string): number {
  const pa = a.split(/[.-]/).map(Number);
  const pb = b.split(/[.-]/).map(Number);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (Number.isNaN(x) || Number.isNaN(y)) return a.localeCompare(b);
    if (x !== y) return x - y;
  }
  return 0;
}

const byName = (a: InstanceSummary, b: InstanceSummary) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });

export function sortInstances(list: InstanceSummary[], sort: LibrarySort): InstanceSummary[] {
  const sorted = [...list];
  switch (sort) {
    case "name":
      return sorted.sort(byName);
    case "version":
      return sorted.sort((a, b) => compareMcVersions(b.mc_version, a.mc_version) || byName(a, b));
    case "playtime":
      return sorted.sort((a, b) => b.playtime_secs - a.playtime_secs || byName(a, b));
    case "played":
      return sorted.sort((a, b) => (b.last_played_unix ?? 0) - (a.last_played_unix ?? 0) || byName(a, b));
  }
}

export interface InstanceSection {
  key: string;
  label: string;
  instances: InstanceSummary[];
}

/** Split an (already sorted) list into library sections. */
export function groupInstances(list: InstanceSummary[], groupBy: LibraryGroupBy): InstanceSection[] {
  if (groupBy === "none") return [{ key: "all", label: "All instances", instances: list }];
  const keyOf = (i: InstanceSummary): string => {
    switch (groupBy) {
      case "group":
        return i.group ?? "";
      case "loader":
        return loaderLabel(i);
      case "version":
        return i.mc_version;
    }
  };
  const sections = new Map<string, InstanceSummary[]>();
  for (const i of list) {
    const k = keyOf(i);
    sections.set(k, [...(sections.get(k) ?? []), i]);
  }
  const keys = [...sections.keys()].sort((a, b) => {
    // Ungrouped always last; versions newest first; the rest alphabetical.
    if (a === "") return 1;
    if (b === "") return -1;
    return groupBy === "version" ? compareMcVersions(b, a) : a.localeCompare(b);
  });
  return keys.map((k) => ({
    key: `${groupBy}:${k}`,
    label: k === "" ? "Ungrouped" : groupBy === "version" ? `Minecraft ${k}` : k,
    instances: sections.get(k)!,
  }));
}

/** Every distinct user-defined group, alphabetically. */
export function allGroups(list: InstanceSummary[] | undefined): string[] {
  return [...new Set((list ?? []).flatMap((i) => (i.group ? [i.group] : [])))].sort();
}
