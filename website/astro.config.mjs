// @ts-check
import { defineConfig } from 'astro/config';
import sitemap from '@astrojs/sitemap';

// Served as a GitHub Pages project site, so every URL lives under /Bananium.
export default defineConfig({
  site: 'https://subhranil-maity.github.io',
  base: '/Bananium',
  trailingSlash: 'ignore',
  integrations: [sitemap()],
  markdown: {
    shikiConfig: { theme: 'github-dark-dimmed' },
  },
  vite: {
    // Screenshots are imported straight from the repo's docs/images.
    server: { fs: { allow: ['..'] } },
  },
});
