// Build-time pixel textures for the terrain layers. Everything is seeded, so
// every build renders the same blocks.

export const BLOCK = 16;

export function rng(seed: number) {
  let s = seed >>> 0;
  return (n: number) => {
    s = (Math.imul(s, 1103515245) + 12345) >>> 0;
    return (s >>> 8) % n;
  };
}

function toUrl(svg: string): string {
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

/**
 * A tile of faint light and dark specks, drawn over a layer's base colour so
 * flat fills read as dirt, stone, or deepslate.
 */
export function noiseTile({
  seed,
  size = 8,
  cells = 24,
  light = 'rgba(255,255,255,0.05)',
  dark = 'rgba(0,0,0,0.09)',
}: {
  seed: number;
  size?: number;
  cells?: number;
  light?: string;
  dark?: string;
}): string {
  const r = rng(seed);
  const px = size;
  const w = cells * px;
  const rects: string[] = [];
  for (let y = 0; y < cells; y++) {
    for (let x = 0; x < cells; x++) {
      const v = r(10);
      if (v < 2) rects.push(`<rect x="${x * px}" y="${y * px}" width="${px}" height="${px}" fill="${dark}"/>`);
      else if (v > 7) rects.push(`<rect x="${x * px}" y="${y * px}" width="${px}" height="${px}" fill="${light}"/>`);
    }
  }
  return toUrl(`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${w}" shape-rendering="crispEdges">${rects.join('')}</svg>`);
}

/**
 * The stepped boundary between two layers: columns of `to` blocks rising
 * 1–3 blocks into a `from` background. `cap` draws a lip on top of each
 * column (the darker green under a grass top).
 */
export function edgeTile({
  seed,
  to,
  shades = [],
  top,
  cap,
  cols = 24,
  maxBlocks = 3,
}: {
  seed: number;
  to: string;
  shades?: string[];
  /** Colours for each column's top block (grass over dirt). */
  top?: string[];
  cap?: string;
  cols?: number;
  maxBlocks?: number;
}): { image: string; width: number; height: number } {
  const r = rng(seed);
  const palette = [to, ...shades];
  const rects: string[] = [];
  for (let i = 0; i < cols; i++) {
    const h = 1 + r(maxBlocks);
    const x = i * BLOCK;
    for (let b = 0; b < h; b++) {
      const y = (maxBlocks - 1 - b) * BLOCK;
      const fill = top && b === h - 1 ? top[r(top.length)] : palette[r(palette.length)];
      rects.push(`<rect x="${x}" y="${y}" width="${BLOCK}" height="${BLOCK}" fill="${fill}"/>`);
    }
    if (cap) {
      const topY = (maxBlocks - h) * BLOCK;
      rects.push(`<rect x="${x}" y="${topY + BLOCK - 4}" width="${BLOCK}" height="4" fill="${cap}"/>`);
    }
  }
  const width = cols * BLOCK;
  const height = maxBlocks * BLOCK;
  return {
    image: toUrl(`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" shape-rendering="crispEdges">${rects.join('')}</svg>`),
    width,
    height,
  };
}

/** The terrain layers, top to bottom. */
export const LAYERS = {
  sky: { base: '#1a2140', shades: [] as string[] },
  dirt: { base: '#3a281b', shades: ['#402c1e', '#342417'] },
  stone: { base: '#302f2d', shades: ['#363532', '#2a2927'] },
  // The app's own background colour.
  deepslate: { base: '#1c1b19', shades: ['#21201d', '#181715'] },
  bedrock: { base: '#100f0e', shades: ['#1c1b19', '#080807'] },
} as const;

export type Layer = keyof typeof LAYERS;

// Grass by moonlight.
export const GRASS = ['#2f5a24', '#356229', '#3b6a2d'];
export const GRASS_CAP = '#244619';

const NOISE: Partial<Record<Layer, Parameters<typeof noiseTile>[0]>> = {
  dirt: { seed: 11, light: 'rgba(255,230,200,0.03)', dark: 'rgba(0,0,0,0.14)' },
  stone: { seed: 23, light: 'rgba(255,255,255,0.03)', dark: 'rgba(0,0,0,0.12)' },
  deepslate: { seed: 37, light: 'rgba(255,255,255,0.018)', dark: 'rgba(0,0,0,0.14)' },
  bedrock: { seed: 41, light: 'rgba(255,255,255,0.04)', dark: 'rgba(0,0,0,0.35)', size: 16, cells: 16 },
};

/** Inline style for a layer's background: base colour plus its speck texture. */
export function layerStyle(layer: Layer): string {
  const noise = NOISE[layer];
  const bg = `background-color: ${LAYERS[layer].base};`;
  return noise ? `${bg} background-image: ${noiseTile(noise)};` : bg;
}

/** Ores that glow. `hi` is the lit face of each nugget. */
export const ORES = {
  gold: { color: '#f5d547', hi: '#fff3a8' },
  diamond: { color: '#4fe3d9', hi: '#c4fffa' },
  redstone: { color: '#ff3b2f', hi: '#ffa59c' },
  emerald: { color: '#33d46a', hi: '#b0ffc8' },
  lapis: { color: '#4169ff', hi: '#a9baff' },
} as const;

export type Ore = keyof typeof ORES;
