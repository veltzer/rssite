# rssite — desired feature set

A working list of everything we want a static site generator to do. Scope is deliberately broad; not every item lands in v1. Items are grouped by concern. The cross-cutting constraint from the README still applies: **every feature must be compatible with plan/build separation** — if it produces files, those files must be enumerable up front.

## 1. Content sources

- **Markdown (CommonMark + extensions)** via `pulldown-cmark`:
  - Tables, strikethrough, task lists, footnotes, autolinks.
  - Math: inline `$…$` and block `$$…$$`, rendered via KaTeX (client-side) or pre-rendered to MathML.
  - Mermaid / PlantUML diagrams as fenced code blocks, rendered at build time to SVG.
  - Admonitions / callouts (`> [!NOTE]` GFM style and `!!! note` mkdocs style).
  - Definition lists.
- **Front matter**: TOML (preferred) and YAML, parsed into a typed schema. Unknown keys are an error by default, warning in loose mode.
- **Raw HTML** passthrough pages (`.html` in content tree).
- **reStructuredText** (optional, behind a feature flag) — not a priority, but the plugin shape should allow it.
- **Jupyter notebooks** (`.ipynb`) — code, markdown, and outputs rendered as a page. Plugin territory.
- **AsciiDoc** — plugin territory.
- **Data files** (`.json`, `.yaml`, `.toml`, `.csv`) accessible from templates for data-driven pages.

## 2. Templating

- **Tera** as the default engine (Rust-native, Jinja-like).
  - Template inheritance, includes, macros.
  - Custom filters and functions registered by plugins.
- **Layout selection** via front matter (`layout: post`) with sensible defaults (`default`, `page`, `post`, `index`).
- **Partials** directory, auto-registered.
- **Shortcodes / custom directives** inside markdown (`{{< figure src=… >}}` style) that expand during rendering — must be enumerable during plan.
- **Template-only pages**: a `.tera` file in the content tree becomes an output page driven by data, not markdown.

## 3. Styling and assets

- **SCSS/SASS** compilation via `grass` (or `rsass`):
  - Partials (`_vars.scss`) tracked as sources in the manifest.
  - Source maps in dev mode.
- **CSS** passthrough with optional minification.
- **Asset pipeline**:
  - Image optimization: resize, convert to WebP/AVIF, generate responsive `srcset`.
  - SVG minification.
  - Fingerprinted filenames (`style.a1b2c3.css`) for cache-busting — manifest reflects the hashed names so downstream tools can consume them.
  - Copy-through for arbitrary static files.
- **JavaScript bundling** — out of scope; delegate to esbuild/rollup via rsconstruct. rssite just references the output.

## 4. Navigation and taxonomy

- **Tags**: front-matter-driven, produce per-tag index pages. Enumerable at plan time by scanning front matter.
- **Categories** (single-valued taxonomy, distinct from multi-valued tags).
- **Custom taxonomies** defined in config (`series`, `authors`, etc.).
- **Automatic archive pages**: by year, year/month, optionally year/month/day.
- **Breadcrumbs** derived from URL structure.
- **Table of contents** per page, auto-generated from headings, configurable depth.
- **Cross-references / wikilinks** (`[[page-slug]]`) that resolve at build time; broken links are hard errors.
- **Menu / nav definition** in config — ordered, nested, with external link support.
- **Pagination** for index and archive pages (N per page).

## 5. Feeds and indexes

- **RSS 2.0** feed.
- **Atom 1.0** feed.
- **JSON Feed 1.1**.
- **Per-tag / per-category feeds**.
- **Sitemap** (`sitemap.xml`) with `lastmod`, `changefreq`, `priority`.
- **robots.txt** generation.
- **Search index** (JSON) for client-side search via lunr/pagefind-style lookup. Pagefind integration is attractive — it already has a precomputed-output story that fits the manifest model.

## 6. URLs and permalinks

- **Configurable permalink schemes** per content type:
  - `/posts/:year/:month/:slug/`
  - `/:category/:slug.html`
  - `/:slug/` (pretty URLs, `index.html` underneath).
- **Slug customization** via front matter, with a default slugifier (Unicode-aware).
- **Aliases / redirects**: front matter `aliases: [/old-path/]` generates redirect pages or a redirect map for the server.
- **Trailing-slash policy** configurable and consistent.

## 7. Multi-language / i18n

- **Multiple locales** in the same project.
- **Language-switcher data** exposed to templates.
- **Per-language front matter** (`title_en`, `title_fr`) OR parallel files (`post.en.md`, `post.fr.md`) — pick one, probably parallel files.
- **Translated permalinks**.
- **Date and number formatting** per locale.

## 8. Drafts, scheduling, versioning

- **Drafts**: pages with `draft: true` excluded from production builds; included with `--drafts`.
- **Future-dated posts**: excluded unless `--future` is passed. Plan must be aware: if the manifest depends on wall-clock time, that's a determinism violation — so `--future` becomes part of the plan inputs.
- **Content versioning / history** (optional): render `published` vs `updated` dates, show diffs. Not v1.

## 9. Code and syntax highlighting

- **Server-side highlighting** at build time via `syntect` or `tree-sitter-highlight` — no client-side JS needed.
- **Line numbers**, **highlighted line ranges** (`{hl_lines=[1,3-5]}`).
- **Copy-to-clipboard** button (client-side JS, opt-in).
- **Language auto-detection** as a fallback.
- **Custom themes**, including dark/light pair matching the site theme.

## 10. SEO and metadata

- **OpenGraph tags** per page.
- **Twitter Card tags**.
- **JSON-LD** structured data for articles, breadcrumbs, site.
- **Canonical URLs**.
- **`<meta>` description** auto-derived from content if not set.
- **Social preview image** generation — optional, plugin territory.

## 11. Accessibility and output quality

- **Built-in link checker** (internal links must resolve; external optional, network-gated).
- **Image `alt` required** (configurable severity).
- **Heading-order check** (no jumping from `h2` to `h4`).
- **Validate generated HTML** optionally via `html5ever` parse.

## 12. Performance

- **Incremental builds** — already the point of the design: only rebuild outputs whose `sources` changed.
- **Parallel rendering** of independent pages.
- **Content-addressed template cache** (compiled templates reused across runs).
- **Lazy image loading** attribute injected automatically.
- **Preload/prefetch hints** for critical assets.

## 13. Dev experience

- **`rssite serve`**: local dev server with hot reload. Watches sources, re-runs plan+build for affected outputs only.
- **`rssite new post "Title"`**: scaffold a new content file from a template.
- **`rssite check`**: run all validators (link checker, manifest consistency, schema) without building.
- **`rssite clean`**: remove `_site/` and internal caches.
- **Helpful errors**: Tera-style error messages with file/line/column pointing back to the source, not the template.
- **Verbose / quiet modes**; structured JSON logs behind `--log-format=json`.

## 14. Plugins

Design already locked per README §Plugins. Features we want plugins to be able to add:

- New content formats (ipynb, asciidoc, org-mode).
- New output formats (EPUB export, PDF export of the whole site).
- Custom shortcodes and template functions.
- Image filters / transforms.
- Pre-publish hooks (linters, spell check, compliance).
- Alternative highlighters, math engines, diagram renderers.

All plugins obey the two-phase contract: `contribute_outputs` (plan) + `build_outputs` (build), with framework-enforced consistency.

## 15. Configuration

- **`rssite.toml`** at project root.
- **Per-directory overrides** (`_config.toml` inside a content subdirectory) — optional, only if clearly useful.
- **Environment-specific profiles** (`[profile.prod]`, `[profile.dev]`) selectable via `--profile`.
- **Schema validation** of config with good error messages.
- **Variables / site globals** exposed to templates as `site.*`.

## 16. Deployment integration

Not rssite's job, but we want to play well with:

- **Netlify / Vercel / Cloudflare Pages** — just produce `_site/`, done.
- **GitHub Pages** — same.
- **rsync / SFTP / S3** — downstream tool's problem.
- **Atomic deploys**: rssite's strict manifest makes this easy — tools know the exact file set.

## 17. Analytics and comments

Not generated, but **easy to embed**:

- Partial templates for common providers (Plausible, Umami, Fathom, GoatCounter).
- Comments via GitHub Discussions / giscus / utterances — a partial template, nothing more.

## 18. Security

- **No arbitrary code execution from content** — markdown and front matter are data, never evaluated.
- **Template sandboxing** — Tera is already sandboxed; keep it that way.
- **Escape by default** in templates.
- **Subresource Integrity** hashes for external scripts/styles, optional.
- **Content Security Policy** header hints in generated `<meta>` tags, configurable.

## 19. Observability

- **Build report**: per-page time, per-plugin time, total output size, cache hit rate.
- **Deterministic builds verifiable**: a `--verify-determinism` flag that runs plan twice and diffs.
- **Manifest diff tool**: `rssite plan --diff <previous-manifest>` shows what changed.

## 20. What we explicitly do NOT want

- Dynamic server-side rendering.
- Runtime template evaluation.
- Database-backed content.
- Admin UI / CMS (content is files in git).
- Theme marketplaces — themes are just template directories, copy them.
- Automatic cloud sync / publish — that's the build system's job.
- Bundled JS framework integration (React/Vue/Svelte islands). Could be a plugin someday; not core.

## Prioritization sketch (not a commitment)

**v0.1 — minimum viable:**
Markdown + front matter, Tera templates, SCSS, tags, RSS, sitemap, permalinks, plan/build contract, `serve` with reload.

**v0.2:**
Syntax highlighting, asset fingerprinting, image optimization, link checker, pagination, archives.

**v0.3:**
i18n, search index (pagefind), OpenGraph/JSON-LD, aliases/redirects, config profiles.

**v0.4+:**
Plugin API stabilized, diagrams, math, notebook support, custom taxonomies, social-image generation.
