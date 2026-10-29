# Configuration

Everything lives in `rssite.toml` at the site root. Every key is known to the generator; an unknown key is an error, never an ignored line.

```toml
base_url = "https://example.org"   # required; a trailing slash is dropped
title = "My site"
description = "What the site is."
author = "Me"

# Directories, all relative to the site root. These are the defaults.
output_dir = "_site"
content_dir = "content"
templates_dir = "templates"
static_dir = "static"
sass_dir = "sass"
i18n_dir = "i18n"

# A theme: themes/<name>/ holds templates, sass, static and i18n
# directories behind the site's own. Optional.
# theme = "fleet"
themes_dir = "themes"

# The language used for the root redirect and the global 404.html.
# Defaults to the first language. It is a fallback for hosts that cannot
# negotiate, not a "default language": every language is written and
# addressed the same way.
fallback_language = "en"

[[languages]]
code = "en"
name = "English"
direction = "ltr"          # or "rtl"; templates get it as language.direction
title = "My site"          # optional, defaults to the top-level title
description = "..."        # optional
generate_feeds = true      # default true: /en/atom.xml
url_prefix = "/en/"        # default "/<code>/"
locale = "en_US"           # optional; templates get it as language.locale
date_format = "%-d %b %Y"  # the date filter's default; "%Y-%m-%d" if unset
date_format_long = "%-d %B %Y"  # with date(long=true); date_format if unset

[[languages]]
code = "he"
name = "עברית"
direction = "rtl"

[[taxonomies]]
name = "tags"
paginate_by = 20           # optional
paginate_path = "page"     # default
feed = true                # a feed per term; default false
translations = "i18n/tags.toml"   # optional, see below
# infer_translations = true       # or pair terms through the pages, see below

[slugify]
paths = "safe"             # "safe" keeps Unicode letters; "ascii" transliterates
taxonomies = "safe"
anchors = "safe"

[markdown]
smart_punctuation = false  # a page or section can override this
highlight = true           # build-time syntax highlighting, CSS in highlight.css
highlight_theme = "InspiredGitHub"
math = false               # $…$ and $$…$$ as math; a page or section can override this
external_links_target_blank = false  # links leaving the site open in a new tab
external_links_no_follow = false     # rel="nofollow" on them
external_links_no_referrer = false   # rel="noreferrer" on them
diagram_languages = ["mermaid"]      # fenced blocks left for a client-side renderer
heading_anchors = "none"             # a # link on headings: "none", "start" or "end"
emoji = false                        # :rocket: as an emoji

[images]
enabled = false            # markdown images of local files as <picture>s, see Content
widths = [480, 960, 1440]  # widths offered below the image's own
formats = ["webp"]         # also offered: "webp", "avif", or both, in preference order
quality = 80               # 1-100, for JPEG, WebP and AVIF
sizes = "100vw"            # the sizes attribute: how wide the image is shown
lazy = true                # loading="lazy"

[feeds]
limit = 20                 # newest entries per feed
filename = "atom.xml"
root = false               # also write the fallback language's feed at /atom.xml

[sitemap]
enabled = true

[search]
enabled = false            # a JSON index per language at <prefix>search.json
filename = "search.json"
content = false            # include each page's text

[related]
limit = 5                  # page.related: pages sharing the most terms; 0 for none
taxonomies = []            # which taxonomies count; all of them by default

# Retired URLs, each a redirect page. Optional, repeatable.
[[redirects]]
from = "/2019/05/01/old-title/"
to = "@/blog/post.md"              # or a site path "/en/blog/", or a URL

[extra]
# Anything; templates read it as config.extra.

# Directories published into the output as they are. Optional, repeatable.
[[publish]]
from = "shared/shared-themes"      # relative to the site root
to = "shared-themes"               # relative to the output root
files = ["themes.css", "fonts.css", "theme-switcher.js", "fonts/*"]
```

## Themes

A theme is a directory, `themes/<name>/`, with the same four directories a site has: `templates`, `sass`, `static` and `i18n`. With `theme = "<name>"` set, the site's own directories sit in front of the theme's, file by file:

- A template is looked up in `templates/` first, then in the theme's `templates/`. A name that exists in both is the site's. The manifest lists the file that was used, `templates/base.html` or `themes/fleet/templates/base.html`, so an override is visible, never silent.
- A sass file is compiled from whichever root has it; a partial (`_tokens.scss`) is found by `@use` in the site's `sass/` first, then the theme's, so the site can use a theme's partials and override them.
- A static file is published from the site's `static/` if present there, otherwise from the theme's.
- A translation file is layered: the theme's `i18n/<code>.toml` first, then the site's, key by key, so the site overrides single strings. The `[dates]` table is taken from the last file that has one, so a site file of strings alone keeps the theme's month names. A theme file for a language the site does not declare is an error.

A theme named in the configuration that does not exist is an error. A theme never changes which languages a site has, which taxonomies, or where the output goes; those stay in `rssite.toml`.

## Published directories

Some of what a site serves is not its own: a submodule's stylesheets, a shared font directory. A `[[publish]]` entry copies files from a directory under the site root into a directory under the output root, as they are. `files` is a list of globs relative to `from` (`*` within a segment, `**` across segments, `?` one character); the default is everything. Each copied file is its own output in the manifest, with itself as its only source, so a build system rebuilds exactly the changed file. An entry whose `from` is not a directory, or that matches no file, is an error: the usual cause is a submodule that is not checked out.

## Languages

Every language is explicit. There is no unmarked default: with more than one language configured, every content file carries a language suffix (`post.en.md`), and every language is served under its own prefix. With exactly one language the suffix may be omitted.

The language's `direction` reaches templates so the document can carry `dir="rtl"` without hand-written conditions.

## Translation files

`i18n/<code>.toml` holds the strings `trans()` returns and the names the `date` filter uses:

```toml
nav_blog = "בלוג"
nav_tags = "תגיות"

[dates]
months = ["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר"]
months_short = [...]   # optional, 12 entries
weekdays = [...]       # optional, 7 entries, Sunday first
weekdays_short = [...] # optional
```

A missing key is an error that names the file. A missing file is an empty table with English date names, so a single-language site needs no `i18n/` directory.

## Taxonomy term translations

Terms are per language: a Hebrew post carries Hebrew tags, and `/he/tags/` and `/en/tags/` are separate term sets. To pair a term with its translation, point the taxonomy's `translations` at a file of rows:

```toml
[[terms]]
en = "religion"
he = "דת"

[[terms]]
en = "linux"
he = "לינוקס"
```

A paired term's page gets `translations` and `alternates` like a page does, so the language switcher and the `hreflang` links reach the same tag in the other language. A row naming a term that no page uses yet is fine; a row naming a language that is not configured is an error.

When a page and its translation always list the same terms in the same order, the table can be left to rssite: `infer_translations = true` pairs the Nth term of a page with the Nth term of each of its translations. Pairs chain across pages, so a term used on many posts gets one row. Two lists of different length, or a term paired with two different terms of one language (some page lists them in another order), is an error naming the pages. A taxonomy takes `translations` or `infer_translations`, not both. Inferred pairings read every language's pages, so the term pages list every content file as a source.

## Redirects

A `[[redirects]]` entry writes a redirect page at `from`, a site path written like a page alias: `/old/` and `old` are `old/index.html`, `/keys/public.asc` is that file. `to` is one of:

- `@/blog/post.md`: a content file, in the fallback language unless the name carries a suffix (`@/blog/post.he.md`), with an optional `#fragment`. The file becomes a source of the redirect, and one that does not exist fails the build.
- `/en/blog/`: a site path, which must be something the build writes. A redirect to a page that was since renamed fails the build instead of sending readers to a 404.
- `https://...`: an absolute URL, taken as it is.

A redirect written over a file the build already produces is an error, like any two outputs at one path. For a page's own old URLs, `aliases` in its front matter does the same job; `[[redirects]]` is for what is not a page: a section that moved, a file that is gone, an address from an older site. A section's old pager URLs are `pager_aliases` (see [Content](content.md)).

## Feeds, search and robots.txt

`feeds.root = true` writes the fallback language's feed a second time at the site root, for readers who subscribed before the site had languages. The copy is byte for byte the language's feed, self link included, so a reader that follows the self link moves to the language's URL.

`search.enabled = true` writes `<language prefix><filename>` for every language: `{"lang": "en", "pages": [...]}`, newest first, each page with `title`, `permalink`, `path`, `date`, `section`, `description` (the meta description, see [Templates](templates.md)) and `taxonomies` (a map of taxonomy name to term names). `content = true` adds each page's text, which makes the index about as large as the site's text.

Without a `robots.txt` in `static/`, rssite writes one allowing everything and naming the sitemap; with the sitemap disabled it names none.

## Slugs

The default slugifier lowercases and keeps every alphanumeric character of every script, turning everything else into `-`. A Hebrew tag therefore gets a Hebrew URL. `ascii` transliterates first, for sites that want Latin-only paths. The mode is set separately for paths, taxonomies and anchors.
