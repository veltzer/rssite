# Lessons from zola

`veltzer.github.io` (the site at veltzer.org) is built with zola 0.23.3. It is a bilingual blog, English and Hebrew, with about 400 posts in each language, plus six single-page JavaScript applications (media tracker, chess viewer, calendar, slides, syllabi, animations), a tag taxonomy, feeds, and a sitemap. It is built by rsconstruct, which needs to know every file a tool writes before the tool runs.

That site is the proving ground for rssite. Every item below is a problem zola caused there, the workaround that had to be built around it, and what rssite must do instead. The workarounds are real: they live in that repo's `config.toml`, `templates/`, `scripts/build_site.py`, `scripts/gen_stats.py`, `scripts/import_teaching.py`, `doc/IMPROVEMENTS.md` and `doc/SEO.md`, and in rsconstruct's `src/processor/mass_generator/zola.rs`. Nothing here is hypothetical.

The list is grouped by theme. The last section condenses it into requirements.

## Multi-linguality

### The default language is marked by the absence of a suffix

Zola identifies its default language by a filename *without* a language marker: `foo.md` is the default language, `foo.he.md` is a translation. There is no way to write `foo.en.md` while `en` is the default. Zola parses the `.en` as part of the slug and silently builds `/blog/foo-en/` instead, with the translation pairing severed and exit code 0. Declaring `[languages.en]` does not help: with a title it is a hard error ("specified twice"), and bare it changes nothing.

The site wanted both languages treated the same, every file explicitly suffixed. The only way to get that was to point `default_language` at a language that has no content at all (`cs`, and there is not one word of Czech on the site). Every real language is then a non-default one.

The phantom language then has to be serviced everywhere:

- A complete `[translations]` table at the top level, because `trans()` errors out when a key is missing from the default language, even though nothing should ever reach it. The same table is therefore written three times in `config.toml`: once for `cs`, once for `en`, once for `he`.
- A `content/_index.md` for the phantom root section, because zola insists the default language has one.
- A top-level `generate_feeds` or `taxonomies` would produce an empty `/atom.xml` and an empty `/tags/` for the phantom language, both of which also reached the sitemap. They had to be moved into each real language's block.
- `build_search_index` at the top level would try to build an index for `cs`, so it has to be off there and set per language.
- `base.html` maps `cs` to `en` by hand because the site-wide `404.html` is rendered once, in the default language, and every nav link in it would otherwise point at `/cs/`, which does not exist.
- Every template defaults `lang` to `en`, never to `config.default_language`.

rssite: every language is explicit and symmetrical. There is no unmarked default and no language that exists only to make the others work.

### The default language lives at the root, the others under a prefix

Zola serves the default language at `/` and every other language under `/xx/`. The site wanted `/en/` and `/he/` side by side. Before the phantom-language trick, that meant a post-processing step (`relocate_english()`) that swept every root directory into `/en/` after the build, and a sitemap fixer that rewrote the unprefixed entries. Every URL that ever shipped at the root then had to be kept alive with redirect stubs, and Google Search Console reported the leftovers for months.

rssite: URL layout per language is configuration, with a symmetric layout as the default.

### Translations are paired by filename and nothing else

`foo.en.md` and `foo.he.md` are one post in two languages because they share a base name. That is also where the URL slug comes from, so a Hebrew post cannot have a Hebrew URL: renaming the file severs the pairing and kills the language switcher, and a `slug =` key in the front matter does the same. The site settled for English slugs under `/he/` and wrote a decision record explaining why.

rssite: pairing and URL are separate facts. A translation can declare its own slug.

### The order of `page.translations` is not deterministic

Three consecutive builds of identical input emitted the two `hreflang` alternates as "he en", "he en", then "en he". Two templates iterated the list directly, so about 100 files differed between builds, and a build change could not be validated by diffing output. The fix was to loop a fixed language list from the config and look each language up, and a pytest in the site repo now guards the template source against anyone reintroducing a direct loop.

rssite: every list exposed to templates has a defined order.

### Taxonomy terms have no translation link

Zola pairs pages and sections across languages but not taxonomy terms. A Hebrew tag page has no idea which English tag page is its counterpart, so its language switcher and `hreflang` alternates could only point at the other language's home page. The site derives a `tag_translations.toml` from the posts by position: tag number N in the English list is the translation of tag number N in the Hebrew list. A script enforces that invariant at build time, and `base.html` reads the table with `load_data` and resolves it with `get_taxonomy_url`.

rssite: taxonomy terms can be declared as translations of each other.

### Slugify transliterates non-Latin scripts

Zola's default slugifier turns a Hebrew tag into unreadable Latin: archaeology became `rky-vlvgyh`. The escape is `[slugify] taxonomies = "safe"`, which only strips characters illegal in a path. It can be set per kind (paths, taxonomies, anchors), which is useful, but the default is wrong for any site with a non-Latin language.

rssite: the slugifier is Unicode-aware by default and transliterates only when asked.

### The `date` filter ignores `locale`, and macros do not work

Every Hebrew page printed its dates in English. `date(format=..., locale="he_IL")` is accepted and silently ignored (`fr_FR` too, verified in a scratch project). The next obvious fix, a Tera macro, failed: zola 0.23.3 rejects `{% macro %}` in a template as "Unknown tag". What works is an `{% include %}` that reads variables the caller `set`s, plus a twelve-entry Hebrew month array kept in `config.toml`, assembled by hand from `%-d`, `%m` and `%Y`.

rssite: per-locale date formatting, and template helpers that can take parameters.

### UI strings live in the config file

Every nav label, every heading, every pagination word and the home page intro are `[languages.xx.translations]` entries in `config.toml`, three copies each. The config file is 300 lines and most of it is prose in two languages.

rssite: translation tables are files per language, outside the config.

### The search index only works for a fixed list of languages

Zola ships stemmers for a fixed set of languages and errors out on any other, so Hebrew search was never possible. English search was on by default and generated a 4.1 MB `search_index.en.js` plus `elasticlunr.min.js` on every build, deployed both, and no page ever loaded them.

rssite: a search index is opt-in, and the engine is pluggable (pagefind has a precomputed-output story that fits the manifest model).

### Right-to-left gets no help

Zola knows a page is Hebrew and does nothing with it. The site sets `dir="rtl"` on `<html>` by hand, wraps the footer in `dir="ltr"` because the bidi algorithm scattered "© 2026 Mark Veltzer · RSS", wraps a phone number and "C++" in `<bdi>` on the training page, and wraps every embedded English application in `<div dir="ltr" lang="en">` so its flex rows do not mirror.

rssite: a language declares its direction, and the generator emits `lang` and `dir` on the document. Bidi inside authored prose remains the author's job.

### Unpaired posts fail silently

A post with no translation just has an empty `page.translations`, so its language switcher disappears without a word. The site enforces "every post exists in both languages" with an external script that fails the build.

rssite: an optional check that every page has every configured language, with a clear error naming the missing file.

### Translated stubs need an escape hatch

The six applications exist once, in English (`content/<app>/_index.en.md`, up to 390 KB each). Their Hebrew sections carry only a translated title, because duplicating a generated 390 KB body would drift the moment the generator ran. The template detects the empty body and pulls the English content with `get_section(path="<app>/_index.en.md", lang="en")`. The `lang="en"` is load-bearing: `get_section` defaults to the current page's language and would return the stub being rendered.

rssite: a translation can say "same body as language X" in its front matter.

### Function signatures disagree about the suffix

`get_url(path="@/blog/_index.md", lang="he")` adds the language suffix itself, so the path stays bare. `get_section(path="blog/_index.he.md", lang="he")` wants the real filename and reports the unsuffixed one as not found. Two functions, two conventions, learned by trial.

rssite: one convention for every function that takes a content path.

### One 404 page for every language

GitHub Pages serves one `404.html` for every unmatched URL, so the page cannot know the reader's language. The site shows both languages on it. That is a hosting limit, not zola's, but zola renders the page in the default language, which on this site is the phantom one.

rssite: the 404 page is rendered per language, and the site can pick which one is the global fallback.

## Templates: the Tera dialect and sharing with hand-written pages

### The Tera subset is narrower than the Tera documentation

Each of these was discovered by a failing build or a wrong page, not by reading a list:

- `{% macro %}` is rejected as an unknown tag (see dates above).
- No `concat` filter, so there is no way to append to an array. `base.html` builds the per-language URL list as a `|`-separated string and reads it back with `split` and `nth`, with a leading `|` so the 1-based loop index lands on the right position.
- No `slice` filter, so the feed cap is a loop counter with `{% break %}`.
- No `group_by` over a derived key, so counting posts per year means looping the whole section once per year. The site computes the archive stats in a Python script, writes them into the blog section's front matter, commits the result, and runs the script in check mode in CI.
- A map cannot be iterated in a defined order, so those stats are written as arrays of tables rather than a year-keyed table.
- The `default` filter fills only a *missing* value. Zola sets `page.description` to the empty string when the front matter omits it, so `page.description | default(...)` shipped an empty meta description on every post. The fix is `if page.description`.
- `page.updated` is null rather than missing, so `default` does not help there either.
- `set_global` is silently shadowed by a plain `set` of the same name in the same scope; the write lands on a different variable and the value comes out empty.
- `feed_title` does not exist even though the built-in feed template looks as if it uses one.

rssite: whatever template engine is chosen, the supported subset is documented, and the obvious operations on lists and maps (append, slice, group, ordered iteration) exist.

### Directory and file names are hardcoded

`templates/`, `content/`, `static/`, `sass/` and `public/` are fixed. Renaming `templates/` to `zola.templates/`, to say which generator owns it, gives "Either a `templates/` folder or a theme is required"; there is no config key and no flag. An `output_dir` key in `config.toml` is silently ignored, so a bare `zola build` writes to `public/` whatever the config says, and the site's notes record stale copies of that tree producing wrong page counts and false "missing post" reports.

rssite: every directory is configurable, and an unknown config key is an error.

### Hand-written HTML pages have to pretend to be markdown

Zola renders a page only from a markdown file through a template. A raw `.html` file in `content/` is not a page, and a file in `static/` is copied verbatim with no template at all. The six applications are hand-written HTML and JavaScript, so each one is pasted wholesale as the body of a `_index.en.md`: the syllabi "markdown" file is 390 KB of inline `<style>`, markup and `const DATA`. Three of them are generated by a script from sibling repositories. The markdown linter then reads the inline CSS as headings and those files have to be excluded from it, and the rest of `content/` needs its own lint dialect because the H1 comes from the front matter, not the body.

rssite: a page can be a template-only file or raw HTML, driven by front matter and data, with no markdown in between.

### One base template cannot serve both zola pages and the standalone sites

The sibling repositories (teaching-slides, teaching-syllabi, teaching-animations) each build a self-contained `index.html` with the same shared-themes header and theme switcher the main site uses. The main site's `base.html` cannot render them: it is Tera with zola-only context (`config`, `section`, `trans`, `get_url`, `load_data`, `page_lang`). And their chrome cannot be used inside zola, because the page would then have two headers and two theme switchers.

The result is `import_teaching.py`: strip the embedded header, strip the theme select, find and remove the inlined copy of `theme-switcher.js` by brace counting (its top-level `const` collided with the shared copy and the browser rejected the second script), prefix every CSS selector with a wrapper id using a hand-written CSS splitter that understands `@media`, `@supports`, `@container`, `@layer` and quoted semicolons in `@import url(...)`, rewrite asset paths to the sibling site's own origin, and commit the result. It runs by hand, because CI has no access to the siblings.

rssite: the base layout is a plain template that any page can extend, including a hand-written HTML page that supplies its own body, and the context a template sees is a documented data structure that another tool could also produce.

### Overriding a built-in template means copying all of it

Zola's built-in `atom.xml` emits every page of a section with the full rendered body. At 165 posts that was a 1 MB file, and with per-tag feeds there were 144 of them: 10.9 MB of feed output that every subscriber re-downloaded on every poll. The built-in `sitemap.xml` lists the `/page/1/` stub of every paginated listing, which zola builds as a redirect to the listing itself; that was 112 of 629 sitemap entries pointing at a redirect, which Search Console reports as an error. Both templates are now overridden with copies of the built-in that add one rule each, and the site's notes record that if zola's defaults change the copies will not follow.

rssite: feeds and sitemaps have the sensible options (entry cap, exclude redirects) as configuration, and the built-in templates are small enough that a copy is not a liability.

### Stylesheets cannot import a plain CSS file

The design tokens come from a git submodule as `themes.css`. A sass `@import` of a `.css` file is left as a runtime `@import`, whose relative path then resolves against `/style.css` and 404s. Zola also does not follow symlinks out of the project. So the file is copied into the output by a separate build step and linked from `base.html` directly.

rssite: assets outside the content tree can be declared as sources and copied with their dependencies tracked.

### `load_data` is the escape hatch for everything else

Build provenance (the commit hash on the About page), the `sameAs` identity list for JSON-LD, the tag translation table, and the archive statistics all reach the templates through `load_data` of files that external scripts generate, or through generated blocks in committed content files guarded by begin/end markers and check-mode scripts. Zola has no plugin system, so anything it cannot compute has to be computed outside, written to disk, and loaded back.

rssite: a plan-aware plugin API, so this kind of computation runs inside the build and its outputs are part of the manifest.

### Templates cannot be linted as HTML

The HTML linters (tidy, htmlhint) run over `static/` only; `templates/` is excluded because a Tera file is not HTML until rendered. Problems in the chrome are found by rendering the site and linting `_site/`.

rssite: a `check` mode that renders templates against fixture data so they can be validated before a full build.

## Build-system transparency

This is the reason rssite exists, so it gets its own section.

### Zola cannot say what it will write

Zola has no plan mode. To build the site under rsconstruct, with one cached product per output file, rsconstruct's zola processor reimplements the part of zola that decides where files go: 1,685 lines in `zola.rs` reproducing content paths, section walking, draft handling, hidden sections, transparent sections, pagination, taxonomies with term merging by slug, feeds, aliases, colocated assets, Sass partials, highlight stylesheets, the static tree, `404.html`, `sitemap.xml` and `robots.txt`, all against one pinned zola version. It refuses any other version by checking `zola --version` on every plan. The alternative, a patch adding `zola plan` to zola itself (`contrib/zola/zola-plan-v0.23.3.patch`), exists and has not been upstreamed.

Things the planner cannot model and therefore refuses by name: themes, `ignored_content`, `ignored_static`, a `[languages]` block for the default language, `resize_image` anywhere (processed images are only known by rendering), and sites above 30,000 pages (zola then splits the sitemap).

rssite: `rssite plan` is the first phase of `rssite build`, and both come from the same code. This is the whole README.

### `zola build` deletes its output directory first

Building into `_site/` directly would delete the files every other processor put there (the redirect stubs, the shared theme files, the companies data, the root feed), files rsconstruct then believes are up to date. So zola builds into a private staging directory and rsconstruct moves the planned files into place, unlinking each one first because a cached restore may have left a read-only hardlink.

rssite: never delete the output directory. Write the planned files, and optionally remove files that were in the previous manifest and are not in this one.

### Every rendered file depends on everything

A template can read any page or section (`get_page`, `get_section`, sibling links, taxonomy listings, the paginator), so a rendered file's content can change when any other file changes. The planner lists the whole content tree, every template, the config, and every `load_data` file as the sources of every rendered page. Only static files and compiled Sass have precise sources. Zola itself rebuilds everything on every run regardless.

rssite: the manifest records real dependencies. A page that reads only its own markdown and its layout depends on only those. Cross-page reads are explicit in the template and widen the dependency set of that page only.

### Output is not reproducible

The translations ordering above, plus `now()` in the footer year. The site's notes record that until the ordering was fixed, nothing about the build could be verified by diffing two runs, and that "verified inert" had meant reading the code rather than diffing output, which is not verification.

rssite: no timestamps, no non-deterministic iteration, and a `--verify-determinism` flag that builds twice and diffs.

### Silent failures

The list of things zola accepts and ignores, or gets wrong while exiting 0, is the most expensive category:

- `foo.en.md` under an English default builds `/blog/foo-en/`.
- `output_dir` in `config.toml` is ignored.
- `locale` on the `date` filter is ignored.
- `default` on an empty-string description does nothing.
- An alias for a page is written to the site root even when the page is not at the root, so it serves `/blog/x/` instead of rescuing `/2026/01/01/old-slug/`; and aliases exist only for pages, not sections, paginator URLs or plain files, so the site generates every redirect stub itself.
- An unpaired translation just loses its switcher.
- A Hebrew tag quietly spelled in English creates a new term under `/he/tags/` without an error.

rssite: unknown keys are errors, ignored arguments do not exist, and anything that changes a URL is either exactly what was asked for or a hard error.

### Version churn without a migration path

Zola 0.23 renamed `highlight_code` to a `[markdown.highlighting]` table and swapped the highlighter from syntect to giallo, so every old theme name (`base16-*`, "Solarized (light)") stopped resolving. An unpinned upgrade fails the build on `config.toml` alone. `cargo install zola` fetches an unrelated yanked crate, not zola. Distro packages track other versions. The site pins a tarball URL in rsconstruct's tool registry.

rssite: config schema is versioned, old keys produce an error naming the new one, and the tool is installable from crates.io under its own name.

### The dev server is not the deployed site

`zola serve` serves from an in-memory build, runs none of the steps around zola, injects a livereload script, and handles trailing slashes slightly differently from a static host. The site has its own `serve.py` that runs the full build and serves `_site/` with a dumb static server, because that is the only faithful preview.

rssite: `serve` serves the real output directory produced by the real build, with reload as the only addition.

### Front matter is not queryable

The redirect generator needs the number of English blog pages before zola runs, so it parses `paginate_by` out of the blog index's front matter with a regular expression and counts `*.en.md` files itself. Every external script that needs to know something about the content re-parses the content.

rssite: `rssite plan` and a `query` subcommand expose the parsed site model as JSON, so scripts never re-implement the parser.

## Smaller things worth not repeating

- The built-in 404 page without a template is a 68-byte document with no stylesheet and no navigation.
- `smart_punctuation` is global, so a site with `--` in its titles has to choose between all or nothing.
- `generate_feeds` and `taxonomies` are per language but `feed_filenames` is global.
- `get_url(path="atom.xml")` without `lang` resolves to the phantom root feed. The real feeds need the argument.
- Pagination always writes a `/page/1/` redirect stub, and there is no option to skip it.
- Terms whose slugs collide are silently merged into one term.
- A draft index file stops the content walk below that directory, which is surprising when a section is hidden by accident.
- Spellchecking in two languages, link checking, HTML validation and schema validation of data files all happen outside zola. That is fine, but the generator should make it easy: a manifest with sources is exactly what a link checker needs.

## What this means for rssite

Condensed into requirements, in the order they bit:

1. Languages are symmetrical. Every content file names its language, every language gets the same URL shape, and nothing is special about any of them.
2. Translation pairing is explicit and separate from the slug. Pages, sections and taxonomy terms can all be paired.
3. Every list a template sees has a defined order. Builds are byte-reproducible by construction and checked by a flag.
4. Locale-aware dates, per-language translation files, RTL direction from the language declaration, Unicode slugs by default.
5. A page is markdown, a template-only file, or raw HTML. Hand-written applications are first-class, not markdown with a 390 KB HTML body.
6. The base layout is a plain template with a documented context, so the same chrome can be rendered by rssite and by other tools.
7. The template subset is documented and has the obvious list and map operations.
8. `plan` is the first phase of `build`. The manifest carries real per-file dependencies. The output directory is never deleted.
9. Unknown config keys, unknown filter arguments and ambiguous filenames are errors. Nothing is silently ignored.
10. Config schema is versioned with named migrations. The tool is installable under its own name.
11. Feeds, sitemaps and pagination have the sensible knobs (entry caps, exclude redirects, no `/page/1/` stub) as configuration.
12. The dev server serves the real build output.
13. A plan-aware plugin API, so archive statistics, redirect maps and generated data pages run inside the build instead of as committed output with check-mode scripts.
