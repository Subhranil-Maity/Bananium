import { useMemo } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import type { InstanceSummary } from "@/bindings/InstanceSummary";
import { cn } from "@/lib/utils";

/** Block-ish palettes (dark → light) the placeholder texture is drawn from. */
const PALETTES = [
  ["#3b5e2b", "#4f7a37", "#5d8f41", "#6fa34c"], // grass
  ["#5a5a5a", "#6e6e6e", "#7f7f7f", "#939393"], // stone
  ["#1d6f6a", "#2a9d8f", "#4cc3b5", "#8be0d6"], // diamond
  ["#8a6d1c", "#c29b27", "#e8c547", "#f7e27a"], // gold
  ["#5e1f1f", "#7a2a2a", "#943838", "#ad4b4b"], // netherrack
  ["#2f5d62", "#3f7f7a", "#5ba39a", "#7cc2b5"], // prismarine
  ["#4b2e6b", "#6a449a", "#8a63c2", "#b08ae0"], // amethyst
  ["#5c3f22", "#7a5530", "#94693d", "#b0824f"], // oak planks
  ["#2a2f45", "#3a4262", "#4d5a86", "#6878aa"], // lapis
  ["#6b3a1e", "#9a5528", "#c46f33", "#e08d4a"], // copper
];

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619);
  return h >>> 0;
}

/** 64 fill colours for an 8×8 texture, derived only from `seed`. */
function texture(seed: string): string[] {
  let state = hash(seed) || 1;
  const palette = PALETTES[hash(seed) % PALETTES.length];
  const cells: string[] = [];
  for (let i = 0; i < 64; i++) {
    // xorshift32: cheap, deterministic, good enough for texture noise.
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    const r = (state >>> 0) / 4294967296;
    cells.push(palette[r < 0.2 ? 0 : r < 0.55 ? 1 : r < 0.85 ? 2 : 3]);
  }
  return cells;
}

/** An 8×8 pixel texture seeded by the instance name: stable and recognisable. */
function PlaceholderTexture({ seed }: { seed: string }) {
  const cells = useMemo(() => texture(seed), [seed]);
  return (
    <svg viewBox="0 0 8 8" shapeRendering="crispEdges" className="size-full" aria-hidden>
      {cells.map((fill, i) => (
        <rect key={i} x={i % 8} y={Math.floor(i / 8)} width={1} height={1} fill={fill} />
      ))}
    </svg>
  );
}

/**
 * An instance's custom icon, or a pixel-block placeholder. A green dot marks
 * a running game.
 */
export function InstanceIcon({
  instance,
  className,
  showRunning = true,
}: {
  instance: Pick<InstanceSummary, "name" | "icon_path" | "running">;
  className?: string;
  showRunning?: boolean;
}) {
  return (
    <div className={cn("relative size-10 shrink-0", className)}>
      <div className="size-full overflow-hidden rounded-md bg-muted ring-1 ring-border ring-inset">
        {instance.icon_path ? (
          <img
            src={convertFileSrc(instance.icon_path)}
            alt=""
            draggable={false}
            className="pixelated size-full object-cover"
          />
        ) : (
          <PlaceholderTexture seed={instance.name} />
        )}
      </div>
      {showRunning && instance.running && (
        <span className="absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full bg-success ring-2 ring-background" />
      )}
    </div>
  );
}
