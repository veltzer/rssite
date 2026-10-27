# Content

## Files and languages

Everything under `content/` is a page, a section index, or an asset.

```text
content/
  _index.en.md          root section, English
  _index.he.md          root section, Hebrew
  about.en.md           a page at /en/about/
  blog/
    _index.en.md        the blog section, English
    _index.he.md        the blog section, Hebrew
    post.en.md          /en/blog/post/
    post.he.md          /he/blog/post/, paired with post.en.md
    diagram.png         an asset, copied next to the section in every language
```

The file name is `<name>.<lang>.<ext>`. With several languages configured the suffix is required; a file without one is an error, as is a suffix that is not a configured language. A directory holding pages in some language needs a `_index.<lang>.md` in that language; the root section is created for every language when the file is absent.

Three body kinds are supported, chosen by extension:

- `.md`: markdown, rendered to HTML. Front matter is required.
- `.html`: used as is. Front matter is optional. This is how a hand-written application page becomes a page of the site without being pretended into markdown.
- `.tera`: the body is a Tera template rendered with the page's context, so it can call `trans()`, loop over data, or read `page.title`. Front matter is optional.

## Front matter

TOML between `+++` lines. Unknown keys are errors.

```toml
+++
title = "The title"            # required for markdown pages
date = 2026-04-28              # TOML date, or a string: "2026-04-28", RFC 3339
updated = 2026-05-01
description = "For meta tags and feeds."
slug = "custom-slug"           # default: the file name, slugified
template = "post.html"         # default: the section's page_template, else page.html
draft = false
weight = 10                    # for sections that sort by weight
aliases = ["/old/url/", "/legacy/file.html"]
translation_key = "shared"     # see Translations
body_from = "en"               # see Translations
smart_punctuation = true       # overrides the config for this page

[taxonomies]
tags = ["religion", "ethics"]  # every taxonomy must be declared in rssite.toml

[extra]
anything = "goes here"         # templates read page.extra
+++
```

A section index (`_index.<lang>.md`) takes `title`, `description`, `template`, `page_template`, `sort_by` (`date`, `weight`, `title`, `none`), `paginate_by`, `paginate_path`, `generate_feeds`, `year_archives`, `archive_template`, `pager_aliases`, `require_translations`, `draft`, `weight`, `aliases`, `updated`, `body_from`, `smart_punctuation` and `extra`. A section that sorts by date and contains a page without one is an error, not a silently dropped page.

## Translations

A page and its translations share a base name: `post.en.md` and `post.he.md` are one post in two languages. When the Hebrew file should have its own name (and therefore its own URL), give both files the same `translation_key`:

```toml
# blog/fourth.he.md
title = "פוסט רביעי"
slug = "רביעי"

# blog/renamed.en.md
title = "Fourth post"
translation_key = "fourth"     # the default key of fourth.he.md is its stem
```

A page can also reuse the body of its translation. An application page that exists once, in English, gets a Hebrew section that keeps the Hebrew navigation around the English body:

```toml
# apps/_index.he.html
+++
title = "יישום"
template = "app.html"
body_from = "en"
+++
```

`body_from` with a non-empty body, with a language the page has no translation in, or pointing at a page that itself uses `body_from`, is an error.

Templates see the translations as `page.translations` (a list ordered as the languages are configured) and as `alternates` (a map from language code to permalink).

`require_translations = true` on a section index makes a missing translation an error: every page of the section must exist in every language the section itself exists in, and the error names the page and the language it lacks.

## Links between pages

A markdown link to `@/blog/post.md` resolves to that post's permalink in the current language. `@/blog/post.he.md` names the language explicitly. `@/blog/_index.md` links to a section. A fragment (`@/blog/post.md#heading`) is kept. A link whose target does not exist fails the build and names the file.

Headings get ids from their text (`## Heading one` becomes `heading-one`, a repeat becomes `heading-one-1`), and the page's table of contents is available to templates as `page.toc`.

`<!-- more -->` in a body ends the summary; `page.summary` is the HTML before it, and feeds use it instead of the full content when present.

## Sorting and pagination

`sort_by = "date"` sorts newest first. `paginate_by = 10` writes the first ten posts at the section's URL and the rest at `page/2/`, `page/3/` and so on. There is no `page/1/`: the first pager is the section itself.

`pager_aliases = "/page/{n}/"` writes a redirect per pager, `{n}` being its number: `/page/1/` to the section, `/page/2/` to its second pager, and so on for as many pagers as the section has. It is for URLs from before the section moved, which a hand-written list would stop covering as the section grows. It needs `paginate_by`.

`year_archives = true` writes one listing per year of the section's dated pages at `<section>/<year>/`, rendered with `archive_template` (default `archive.html`), and gives the section a `years` list for templates to link from. A year's page in one language has the same year in the other languages as its translations, where those have pages that year, so the alternates and the language switcher work there as everywhere.

Within a section's order, every page has `previous` and `next`.

## Drafts

`draft = true` on a page or a section leaves it out of the build. A draft section takes its pages and assets with it. `--drafts` includes them all.
