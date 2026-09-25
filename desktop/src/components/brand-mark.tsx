import { cn } from "@/lib/utils";

/** The Bananium mark: a pixel-art banana on a rounded tile. */
export function BrandMark({ className }: { className?: string }) {
  // 10×10 pixel banana; each string row is one pixel row ("#" body, "o" shade, "s" stem).
  const rows = [
    ".......ss.",
    "........s.",
    ".......##.",
    "......##o.",
    ".....###o.",
    "....###o..",
    "..####oo..",
    ".####oo...",
    "..oooo....",
    "..........",
  ];
  const colors: Record<string, string> = { "#": "#f7d64a", o: "#d9a91c", s: "#6b4a1b" };
  return (
    <svg viewBox="0 0 10 10" shapeRendering="crispEdges" className={cn("size-6", className)} aria-label="Bananium">
      {rows.flatMap((row, y) =>
        [...row].map((c, x) =>
          colors[c] ? <rect key={`${x},${y}`} x={x} y={y} width={1} height={1} fill={colors[c]} /> : null,
        ),
      )}
    </svg>
  );
}
