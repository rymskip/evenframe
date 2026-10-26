# Evenframe documentation

SvelteKit 3 and Svelte 5, prerendered with adapter-static and served by Cloudflare Workers Static Assets. Deno manages dependencies and tasks; Biome formats and lints TypeScript, Svelte, CSS, and JSON. The UI uses shadcn-svelte components and yellow/gray theme tokens.

## Development

From `website/`, using Deno 2.9 or newer:

```sh
deno install --frozen --allow-scripts=npm:workerd
deno task dev
```

The repository's Pixi environment also supplies Deno. Dependencies and tasks share `package.json`; `deno.lock` pins resolution. No editor-specific configuration is required.

| Task | Purpose |
| --- | --- |
| `deno task format` | Apply Biome formatting, import organization, and safe fixes |
| `deno task lint` | Check formatting and lint, treating warnings as failures |
| `deno task check` | Sync SvelteKit and check types, components, and accessibility |
| `deno task build` | Prerender into `build/` |
| `deno task preview` | Run the Vite dev server |
| `deno task serve` | Serve the build locally using Wrangler |
| `deno task test` | Browser tests against the production build through Wrangler |
| `deno task deploy:check` | Validate a Workers deployment without publishing |
| `deno task verify` | Run lint, checks, build, browser tests, and deployment validation |
| `deno task deploy` | Build and publish to Cloudflare Workers |

Before the first browser test, install Chromium:

```sh
deno run -A npm:@playwright/test install chromium
```

Tests start their own `deno task serve` on port 4173, so they run against the production build. Run `deno task build` first. `deno task serve` alone defaults to Wrangler's port 8787. `PLAYWRIGHT_BASE_URL` can point tests at an already-running server, in which case the test runner does not start another.

## Authoring

Articles live in `src/lib/content/` as Markdown. Add a registration to `src/lib/docs.ts` with its slug, title, description, and group. That registry drives navigation, search, page metadata, static route entries, and previous/next links. Use kebab-case filenames and start article sections at `##`; the page supplies its title.

Markdown is rendered and sanitized on the server during prerendering. Shiki highlights fenced code blocks. Heading ids and the table of contents are generated together using GitHub's slugging rules. Full-text search matches article bodies as well as titles and descriptions and requires no external service.

Use supported fenced languages: `rust`, `toml`, `sh`/`bash`, `typescript`, `json`, or plain text. Add languages to `src/lib/server/markdown.ts` before using other fences. Author internal article links as relative paths, such as `../installation/`, so they work at a configured base path.

Theme tokens live in `src/routes/layout.css`. shadcn-svelte components live under `src/lib/components/ui/`; `components.json` configures their registry and imports. Component updates should be regenerated from that configuration and formatted with Biome. Biome's full Svelte support is experimental; `svelte-check` additionally validates the compiler and accessibility rules.

## Cloudflare Workers

`wrangler.json` declares an assets-only Worker named `evenframe`. It serves `build/` and returns the static `404.html` with a 404 status for missing assets. There is no Pages project or server bundle.

```sh
deno task build
deno task serve
deno task deploy:check
deno task deploy
```

Authenticate Wrangler before deployment, or supply `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` through the deployment environment. Keep credentials out of committed files. Configure the desired custom domain in Cloudflare or through Wrangler routes.

For Cloudflare Workers Builds, set the root directory to `website`, install with `deno install --frozen --allow-scripts=npm:workerd`, build with `deno task build`, and deploy with `deno run -A npm:wrangler deploy`. The build environment must provide Deno.

`BASE_PATH` optionally configures a subdirectory deployment. It must begin with `/` and have no trailing slash. The default root deployment is appropriate for Workers custom domains. The standalone static 404 links to the origin root.

Set `SITE_URL` to the public HTTP(S) origin at build time to emit canonical and Open Graph URLs, for example `SITE_URL=https://docs.example.com deno task build`. Without it, the site emits titles and descriptions without assuming a deployment hostname.
