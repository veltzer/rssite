# Templates

Templates are [Tera](https://keats.github.io/tera/docs/) files under `templates/`. Autoescaping is on for `.html`, `.htm` and `.xml`; it escapes `&`, `<`, `>`, `"` and `'` and leaves `/` alone, so a permalink prints as a URL.

## Which template renders what

| Output | Template |
| --- | --- |
| Root section of a language | `index.html` |
| Any other section | `section.html`, or the section's `template` |
| Year archive (`/en/blog/2026/`) | `archive.html`, or the section's `archive_template` |
| Page | `page.html`, or the section's `page_template`, or the page's `template` |
| Taxonomy list (`/en/tags/`) | `taxonomy_list.html` |
| One term (`/en/tags/x/`) | `taxonomy_single.html` |
| Feeds | `atom.xml` (built in; a file of that name replaces it) |
| Sitemap | `sitemap.xml` (built in) |
| 404 pages | `404.html` (built in) |
| Site root `/` | `root.html` if present, otherwise a redirect to the fallback language |

A missing `index.html`, `section.html`, `archive.html`, `page.html`, `taxonomy_list.html` or `taxonomy_single.html` is an error when something needs it.

With a theme configured (see [Configuration](configuration.md)), every name is looked up in the site's `templates/` first and in the theme's second; `extends`, `include` and `import` resolve the same way, so a site template can extend a theme layout and a theme layout can include a site partial. The manifest names the file each template came from.

## Context

Every template gets:

- `config`: the whole configuration, `config.extra` included.
- `lang`: the language code being rendered.
- `language`: `{code, name, direction, url_prefix, permalink, title, author, description, generate_feeds, locale, date_format, date_format_long, is_fallback}`. `locale` is null unless the language sets it. `title`, `author` and `description` are the language's own, or the top-level ones when it sets none; feeds (a section's and a term's too) and a page's JSON-LD name the site and the author with them.
- `languages`: the list of all languages, in configured order.
- `current_path` and `current_url`.
- `translations`: this document in every language it exists in, as a list of `{lang, permalink, path, title, current}` in configured language order; `current` is true for the language being rendered.
- `alternates`: the same as a map from language code to permalink. A language switcher is one line: `{{ alternates | get(key=entry.code, default=entry.permalink) }}`.
- `x_default`: the fallback language's version of this document, for `<link rel="alternate" hreflang="x-default">`; null when there is none.
- `meta_description`: what the document says about itself in `<meta name="description">` and `og:description`. A page's is its `description`, else the start of its summary or text (160 characters at most, cut at a word); a section's is its `description`; anything else, and anything without one, gets the language's description, else the site's.
- `git`: the commit the site root is checked out at, `{commit, short, date, year, dirty}`, or null outside git. `date` is the committer date, so `git.year` is a copyright year that does not depend on the build clock. A template that reads `git` lists `.git/HEAD`, the branch's ref and `.git/index` as sources, so a new commit rebuilds the pages that show it. `dirty` is the exception to the rule that every output is a function of its listed sources: an unstaged edit changes it without changing them.
- `build`: when the site was built, `{date, time, year}` in UTC (`date` is `YYYY-MM-DD`, `time` is RFC 3339 to the second). It is read once per build, so every page of a build names the same moment, and `SOURCE_DATE_EPOCH` replaces the clock when set. It is not a function of the sources: a page whose sources did not change keeps the date of the build that last wrote it.
- `rssite`: the generator, `{name, version, url}`, for a "made by" line in a footer.

Pages get `page`, `section` (the parent, without its page list, plus `description` and `extra`) and `json_ld`, the page's schema.org JSON-LD, ready for `<script type="application/ld+json">{{ json_ld | safe }}</script>`.

The JSON-LD is a `BlogPosting` when the page is dated and a `WebPage` otherwise, with its author, dates, terms as `keywords`, its `image` when the page has one (`page.image`, see [Content](content.md)) and its translations as `workTranslation`. With `author_id` set in `rssite.toml` the author carries that `@id`, so a `Person` block of the same `@id` written by a template and the post's author are one entity.

Sections get `section` with `pages`, `subsections` and `years` (the year archives, `{year, count, permalink, path}` newest first, empty unless `year_archives` is set), and `paginator` when paginated. A year archive gets `section` (light, with `description` and `extra`), `year`, `pages` (that year's, in section order) and `years`. Term pages get `taxonomy`, `term` and `paginator`; taxonomy lists get `taxonomy` and `terms`. Feeds get `pages`, `feed_url`, `feed_alternate`, `feed_title`, `last_updated` and, for scoped feeds, `section` or `term`. The sitemap gets `entries`.

A page object:

```text
title, description, date ("2026-04-28" or null), datetime (RFC 3339), year, month, day,
updated, updated_datetime, slug, path, permalink, content, summary, toc, word_count,
reading_time, taxonomies (map of name to a list of {name, slug, permalink, path}), extra,
lang, translations, meta_description, related, previous, next, draft, weight,
relative_path, components, body_kind, section ({title, permalink, path, lang})
```

`page.related` is up to `related.limit` pages of the same language (`{title, permalink, path, lang, date}`) sharing the most terms with this one, in the taxonomies `related.taxonomies` names (all by default): most shared terms first, then newest. A template that reads it depends on every content file, as one that reads `previous` does.

A section object carries `page_count` and every term `page_count`, so a tag cloud is `terms | sort(attribute="page_count") | reverse`.

`page.taxonomies.<name>` exists for every configured taxonomy, empty when the page has no terms, so `{% for tag in page.taxonomies.tags %}` is always safe.

A paginator:

```text
paginate_by, number_pagers, current_index, first, last, previous (URL or null),
next (URL or null), base_url, pages (this pager's slice), total_pages
```

## Functions

- `trans(key, lang=?)`: a string from `i18n/<lang>.toml`. A missing key is an error naming the file.
- `get_url(path, lang=?, check=true, cachebust=?)`: `@/blog/post.md` gives that content's permalink in `lang`; any other path is a file of the build (`style.css`, `images/x.png`, `en/tags/`) and resolves against `base_url`. A path the build does not produce is an error; pass `check=false` for a file another tool writes into the output tree. A static, published or Sass file gets `?h=<hash of its bytes>` appended, see [Cache-busting](#cache-busting).
- `get_page(path, lang=?)` and `get_section(path, lang=?)`: the page or section object for a content path such as `blog/post.md` or `blog/_index.md`.
- `get_taxonomy(name, lang=?)`: `{name, lang, path, permalink, terms}`.
- `get_taxonomy_url(name, term, lang=?)`: a term's permalink; an unknown term is an error.
- `get_image_metadata(path)`: `{width, height}` of a JPEG, PNG, WebP or GIF file under the site root (`static/images/a.jpg`), after its EXIF orientation. A literal path is recorded as a source of the pages that use it.
- `load_data(path, format=?, required=true)`: a TOML or JSON file under the site root as a value, any other file as a string. With `required=false` a missing file is `null`. The path is recorded as a source of the pages that use it.

`now()`, `get_random()` and `get_env()` exist in Tera and are errors here: their results would differ between two builds of the same sources.

## Cache-busting

Hosts let browsers keep a stylesheet or a script for a while (GitHub Pages: ten minutes), so after a deploy a reader can get the new HTML with yesterday's `style.css`. Naming every asset by the commit fixes that but throws away every reader's cache on every deploy, changed file or not. rssite names a file by its content instead:

```html
<link rel="stylesheet" href="{{ get_url(path='style.css') }}">
<!-- https://example.org/style.css?h=9f2c4e0b7a1d3c55 -->
```

The hash is 64-bit FNV-1a of the bytes the build writes: the file itself for a static or `[[publish]]`ed file, the compiled CSS for a Sass entry point. It moves when those bytes do and only then, and it is the same on every machine and every release.

It is on by default. `cachebust = false` in `rssite.toml` turns it off for the site; `cachebust=false` or `cachebust=true` on a call overrides that for the one URL.

What gets a hash, and what does not:

- **Static, published and Sass files.** Pages, feeds, the sitemap, the search index, content assets and image variants are left as they are: a rendered file's bytes are not known until it is rendered, and a feed reader or search engine wants a stable address. Asking for one with `cachebust=true` is an error.
- **Literal paths in templates.** A URL whose hash changes with a file makes the page depend on that file, so the plan lists the file's sources (for a stylesheet, every Sass file) among the page's own, and a build system rebuilds the pages when the file changes. The plan can only see a path written as one string: `get_url(path='style.css')`, not `get_url(path='data/' ~ lang ~ '.json')`. A computed path, and any call inside a content body or a shortcode (whose HTML is reused in listings with other sources), is left without a hash unless the output already depends on that file; with `cachebust=true` it is an error instead, so a URL is never hashed by a file nothing tracks.

A consequence to expect: editing the stylesheet rewrites every page that links it, because each one now names the new hash.

## Filters

- `date(format=?, long=false, lang=?)`: formats a date or RFC 3339 string with strftime, taking `%B`, `%b`, `%A` and `%a` from the language's `[dates]` names. Without `format` it uses the language's `date_format`, or `date_format_long` with `long=true`, so `{{ page.date | date }}` is right in every language. An invalid format is an error.
- `slugify(mode="safe")`: the site's slugifier.
- `markdown(inline=false)`: render a string as markdown. `inline=true` unwraps a single paragraph; a string that renders to anything else (several paragraphs, a heading) is returned as it is.

All of Tera's own filters (`length`, `sort`, `group_by`, `concat`, `slice`, `json_encode`, `get`, `default` and the rest) are available; Tera's macros work through `{% import "macros.html" as m %}`.

## Sharing a layout with other tools

The context above is plain JSON. `rssite query` prints the same page and section objects, so another tool can render the same base template with the same data, and a hand-written `.html` page can extend the site's layout by being a content file with `template = "app.html"`.
