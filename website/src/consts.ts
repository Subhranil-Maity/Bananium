// Mirrors the constants in desktop/src/routes/about.tsx.
export const SITE_TITLE = 'Bananium';
export const SITE_TAGLINE = 'The Banana Launcher';
export const SITE_DESCRIPTION =
  'A fast, lightweight Minecraft launcher. Instances, Fabric, Modrinth mods, shaders, resource packs, and modpacks, with a Rust core that stays out of your RAM\'s way.';
export const VERSION = '0.1.1';

export const REPO_URL = 'https://github.com/Subhranil-Maity/Bananium';
export const AUTHOR_NAME = 'Subhranil Maity';
export const AUTHOR_HANDLE = 'Subhranil-Maity';
export const AUTHOR_URL = `https://github.com/${AUTHOR_HANDLE}`;
export const BLOG_URL = 'https://subhranil-maity.github.io/';

/** Prefix an internal path with the site base (`/Bananium`). */
export function url(path = ''): string {
  const base = import.meta.env.BASE_URL.replace(/\/$/, '');
  return `${base}/${path.replace(/^\//, '')}`;
}
