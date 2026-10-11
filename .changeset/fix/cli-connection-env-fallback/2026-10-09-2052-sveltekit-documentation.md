# feat(website): add SvelteKit documentation on Cloudflare Workers

**Date**: 2026-10-09 20:52
**Commit**: bca1d70
**Area**: Website, documentation

## Problem

Evenframe had a README but no documentation website with navigation, search,
mobile layouts, or a deployment configuration.

## Cause

The repository had no web documentation project. Build-script instructions also
assumed ArkType and Effect generators were enabled by the generation infrastructure.

## Fix

Add a SvelteKit 3 website with 26 Markdown articles based on the Rust interfaces,
configuration, CLI, and existing documentation. Use Deno with a single
package.json manifest and a frozen lockfile, Biome, Tailwind, and shadcn-svelte.

Provide yellow/gray light and dark themes, full-text search, responsive
navigation, heading links, tables of contents, code copying, and dual-theme
Shiki highlighting. Sanitize rendered Markdown and bundle the sanitizer's
CommonJS/ES-module boundary for Deno prerendering. Wrap long inline keys on
mobile while keeping fenced examples scrollable.

Prerender the site for Cloudflare Workers Static Assets, with wrangler.json,
local preview and deployment tasks, a real static 404, and optional canonical
URLs. Correct the README's generator-feature instructions and document site
development and deployment.

Use a circular syncing-arrow logo and a plain `evenframe` wordmark. The header
and favicon share one SVG asset, which preserves the icon's license notice.

## Verified

- `deno install --frozen --allow-scripts=npm:workerd`: passed.
- `deno task format`: passed.
- `deno task verify`: passed. Biome passed; svelte-check reported zero errors
  and zero warnings; the static build completed; all five browser tests passed;
  Wrangler's deployment dry run passed.
- Browser tests exercised navigation through all 26 articles, full-text search
  and empty results, keyboard entry, theme persistence and rendered code colors,
  clipboard copying, mobile navigation and heading links, inline wrapping,
  nested page refreshes, and a Workers-served 404.
- A browser walkthrough exercised the homepage, documentation navigation,
  search, theme switching, mobile navigation, heading links, and code copying.
- After the branding update, format, check, build, and lint passed again. A
  browser preview confirmed the wordmark, loaded syncing icon, shared favicon,
  and home-link navigation; the rendered logo was inspected in a screenshot.
