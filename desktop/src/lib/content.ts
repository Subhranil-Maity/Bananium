import type { ContentKind } from "@/bindings/ContentKind";

export const KINDS: { kind: ContentKind; label: string; singular: string }[] = [
  { kind: "mod", label: "Mods", singular: "mod" },
  { kind: "resource_pack", label: "Resource packs", singular: "resource pack" },
  { kind: "shader", label: "Shaders", singular: "shader pack" },
];

/** What the Browse page can show: installable content, or modpacks (which become new instances). */
export type BrowseKind = ContentKind | "modpack";

export const BROWSE_KINDS: { kind: BrowseKind; label: string }[] = [
  ...KINDS,
  { kind: "modpack", label: "Modpacks" },
];

export function kindLabel(kind: ContentKind): string {
  return KINDS.find((k) => k.kind === kind)!.label;
}

/** File extension the backend accepts for each kind (mirrors `ContentKind::accepts`). */
export function kindExtension(kind: ContentKind): string {
  return kind === "mod" ? "jar" : "zip";
}

/** Compact download counts: 1234567 -> "1.2M". */
export function formatCount(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)}B`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
  return String(n);
}

export function contentKey(instance: string) {
  return ["content", instance] as const;
}
