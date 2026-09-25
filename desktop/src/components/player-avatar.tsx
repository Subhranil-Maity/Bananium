import { cn } from "@/lib/utils";

// 8×8 default-skin faces. Letters index into each palette.
const STEVE = ["HHHHHHHH", "HHHHHHHH", "HSSSSSSH", "SSSSSSSS", "SWPSSPWS", "SSSNNSSS", "SSMMMMSS", "SSMSSMSS"];
const ALEX = ["HHHHHHHH", "HHHHHHHH", "HSSSSSHH", "SSSSSSSH", "SWPSSPWS", "SSSSSSSS", "SSSMMSSS", "SSSSSSSS"];
const STEVE_COLORS: Record<string, string> = {
  H: "#2f1f0f",
  S: "#b8896b",
  W: "#ffffff",
  P: "#4e3d8a",
  N: "#8f5e3e",
  M: "#6b3f2a",
};
const ALEX_COLORS: Record<string, string> = {
  H: "#d7792c",
  S: "#f1c9a3",
  W: "#ffffff",
  P: "#3f7b3a",
  N: "#e0ae88",
  M: "#c9876a",
};

/**
 * Java's `UUID.hashCode()` parity picks Steve or Alex, the rule legacy
 * clients used for players without a skin. Falls back to the name when no
 * UUID is known.
 */
function isAlex(uuid: string | undefined, name: string): boolean {
  const hex = uuid?.replace(/-/g, "");
  if (hex && hex.length === 32) {
    const hilo = BigInt(`0x${hex.slice(0, 16)}`) ^ BigInt(`0x${hex.slice(16)}`);
    const hash = Number((hilo >> 32n) & 0xffffffffn) ^ Number(hilo & 0xffffffffn);
    return (hash & 1) !== 0;
  }
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) | 0;
  return (h & 1) !== 0;
}

/** A pixel-art default-skin head for an offline account. */
export function PlayerAvatar({ name, uuid, className }: { name: string; uuid?: string; className?: string }) {
  const alex = isAlex(uuid, name);
  const face = alex ? ALEX : STEVE;
  const colors = alex ? ALEX_COLORS : STEVE_COLORS;
  return (
    <svg
      viewBox="0 0 8 8"
      shapeRendering="crispEdges"
      className={cn("size-7 shrink-0 rounded-[3px]", className)}
      aria-label={name}
    >
      {face.flatMap((row, y) =>
        [...row].map((c, x) => <rect key={`${x},${y}`} x={x} y={y} width={1} height={1} fill={colors[c]} />),
      )}
    </svg>
  );
}
