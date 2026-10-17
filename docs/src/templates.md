# Templates

Templates are [Tera](https://keats.github.io/tera/docs/) files under `templates/`. Autoescaping is on for `.html`, `.htm` and `.xml`; it escapes `&`, `<`, `>`, `"` and `'` and leaves `/` alone, so a permalink prints as a URL.

## Which template renders what

| Output | Template |
|---|---|
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
- `language`: `{code, name, direction, url_prefix, title, description, generate_feeds}`.
- `languages`: the list of all languages, in configured order.
- `current_path` and `current_url`.
- `translations`: this document in every language it exists in, as a list of `{lang, permalink, path, title}` in configured language order.
- `alternates`: the same as a map from language code to permalink. A language switcher is one line: `{{ alternates | get(key=entry.code, default=entry.url_prefix) }}`.
- `rssite`: the generator, `{name, version, url}`, for a "made by" line in a footer.

Pages get `page` and `section` (the parent, without its page list, plus `description` and `extra`). Sections get `section` with `pages`, `subsections` and `years` (the year archives, `{year, count, permalink, path}` newest first, empty unless `year_archives` is set), and `paginator` when paginated. A year archive gets `section` (light, with `description` and `extra`), `year`, `pages` (that year's, in section order) and `years`. Term pages get `taxonomy`, `term` and `paginator`; taxonomy lists get `taxonomy` and `terms`. Feeds get `pages`, `feed_url`, `feed_alternate`, `feed_title`, `last_updated` and, for scoped feeds, `section` or `term`. The sitemap gets `entries`.

A page object:

```text
title, description, date ("2026-04-28" or null), datetime (RFC 3339), year, month, day,
updated, updated_datetime, slug, path, permalink, content, summary, toc, word_count,
reading_time, taxonomies (map of name to a list of {name, slug, permalink, path}), extra,
lang, translations, previous, next, draft, weight, relative_path, components, body_kind,
section ({title, permalink, path, lang})
```

`page.taxonomies.<name>` exists for every configured taxonomy, empty when the page has no terms, so `{% for tag in page.taxonomies.tags %}` is always safe.

A paginator:

```text
paginate_by, number_pagers, current_index, first, last, previous (URL or null),
next (URL or null), base_url, pages (this pager's slice), total_pages
```

## Functions

- `trans(key, lang=?)`: a string from `i18n/<lang>.toml`. A missing key is an error naming the file.
- `get_url(path, lang=?, check=true)`: `@/blog/post.md` gives that content's permalink in `lang`; any other path is a file of the build (`style.css`, `images/x.png`, `en/tags/`) and resolves against `base_url`. A path the build does not produce is an error; pass `check=false` for a file another tool writes into the output tree.
- `get_page(path, lang=?)` and `get_section(path, lang=?)`: the page or section object for a content path such as `blog/post.md` or `blog/_index.md`.
- `get_taxonomy(name, lang=?)`: `{name, lang, path, permalink, terms}`.
- `get_taxonomy_url(name, term, lang=?)`: a term's permalink; an unknown term is an error.
- `load_data(path, format=?, required=true)`: a TOML or JSON file under the site root as a value, any other file as a string. With `required=false` a missing file is `null`. The path is recorded as a source of the pages that use it.

`now()`, `get_random()` and `get_env()` exist in Tera and are errors here: their results would differ between two builds of the same sources.

## Filters

- `date(format="%Y-%m-%d", lang=?)`: formats a date or RFC 3339 string with strftime, taking `%B`, `%b`, `%A` and `%a` from the language's `[dates]` names. An invalid format is an error.
- `slugify(mode="safe")`: the site's slugifier.
- `markdown(inline=false)`: render a string as markdown. `inline=true` unwraps a single paragraph; a string that renders to anything else (several paragraphs, a heading) is returned as it is.

All of Tera's own filters (`length`, `sort`, `group_by`, `concat`, `slice`, `json_encode`, `get`, `default` and the rest) are available; Tera's macros work through `{% import "macros.html" as m %}`.

## Sharing a layout with other tools

The context above is plain JSON. `rssite query` prints the same page and section objects, so another tool can render the same base template with the same data, and a hand-written `.html` page can extend the site's layout by being a content file with `template = "app.html"`.
