# Plan

Features other static site generators have and rssite lacks, in the order
we mean to take them. Written on 2026-10-28 from a scan of `src/`; an item
counts as missing when nothing in the source implements it.
[Features](docs/src/features.md) is the original design wish list; this file
is the open part of it, ranked. When an item lands, remove it here and
document it in the book.

Already built, so not listed: multiple languages and translations (taxonomy
terms included), redirects, Atom feeds (per language, per term, at the root),
sitemap, `robots.txt`, search index, pagination, archives, custom taxonomies,
drafts, table of contents, heading anchors, summaries, reading time, related
posts, previous and next, `load_data`, server-side highlighting, SCSS, themes,
git and build info, internal link checking, `check`, `serve`, and the
determinism check.

Done since this list was written, documented in
[Content](docs/src/content.md#markdown) and
[Getting started](docs/src/getting-started.md): callouts, definition lists,
opt-in math, `rel` and `target` on external links, line numbers and
highlighted lines in code blocks, live reload in `serve`, mermaid diagram
blocks, heading anchor links, code block titles, left-to-right code on
right-to-left pages, emoji shortcodes, a section's `[cascade]` of front
matter to everything below it, and shortcodes (templates called from
markdown). The [demo site](demos/) shows each of them.

## Bigger features

1. **Image processing**: resize, WebP and AVIF, `srcset`, automatic `width`
   and `height` (no layout shift) and `loading="lazy"`. Zola's
   `resize_image`, Hugo's image processing. It fits plan/build separation:
   every output size is a known file.
2. **Cache-busting asset URLs**: `style.css?h=abc123` (zola's
   `cachebust=true`) or fingerprinted file names (Hugo's `fingerprint`).
   Today a changed stylesheet can be served stale from browser caches.
3. **Minification** of HTML, CSS and inline JS: `minify_html` in zola,
   `--minify` in Hugo. On a site of about 1,200 pages it saves real bytes.
4. **Scheduled posts**: a future-dated post stays out unless `--future` is
   given, as in Hugo and Jekyll. To keep builds reproducible the cutoff has to
   be a plan input, for example `SOURCE_DATE_EPOCH`, which `src/build_info.rs`
   already reads.
5. **`rssite new <section>/<slug>`**: scaffold a post, like `hugo new`. On a
   bilingual site it writes both `.en.md` and `.he.md` with matching front
   matter.
6. **Base URL override**: `--base-url` (zola) or environments (Hugo), for
   preview deploys and local builds that should not point at the production
   host.

## Nice to have

1. **External link checking**, as in `zola check`, behind a flag because it
   needs the network. Internal links are already checked.
2. **Pagefind integration**: the search index format exists, nothing
   consumes it. Pagefind also covers Hebrew, which zola never could.
3. **Pages from data**: one page per row of a data file (Hugo content
   adapters, 11ty pagination over data). The media tracker or the syllabi on
   veltzer.github.io could become real pages instead of imported HTML.
4. **Content checks**: images without `alt`, headings that skip a level
   (`h2` to `h4`). Both are in the features list; `check` is their home.
5. **JSON Feed** and **Sass source maps**: little value for veltzer.org,
   common elsewhere.

## Seen elsewhere since

Ideas from a second look at other generators, after the cheap ones of that
look were done.

1. **Including real files**: mdBook's `{{#include file.rs:10:20}}`, MkDocs'
   snippets. Code in a post comes from a source file, with a line range, and
   cannot go stale; the file is a source of the page in the manifest.
2. **Last-modified date per page from git**: Hugo's `enableGitInfo`. Each
   page's `lastmod` from its file's history, for the sitemap's `<lastmod>`
   and an "updated" line, without writing `updated =` by hand.
3. **Render hooks**: Hugo's `render-link.html`, `render-image.html` and the
   rest. A template takes over how a link, image, heading or code block is
   written: images in `<figure>` with `loading="lazy"`, and options like the
   external link ones without new code each time.
4. **More output formats**: `llms.txt` and a raw markdown copy of each page
   for AI crawlers, as Docusaurus and Mintlify now write.
5. **Social preview images**: an Open Graph image per post with its title on
   a template (Astro, Hugo with a plugin). Hebrew text shaping is the work.
6. **Privacy-friendly embeds**: video players that load on click, as Hugo's
   privacy settings and "lite" YouTube embeds do. Shortcodes exist now, so
   this is a matter of shipping good ones; the demo's `youtube` already uses
   the no-cookie player.

## Where to start

Image processing is the largest gap left against zola and Hugo for a blog
this size. Privacy-friendly embeds can now be written as shortcodes.
