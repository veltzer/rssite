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

[slugify]
paths = "safe"             # "safe" keeps Unicode letters; "ascii" transliterates
taxonomies = "safe"
anchors = "safe"

[markdown]
smart_punctuation = false  # a page or section can override this
highlight = true           # build-time syntax highlighting, CSS in highlight.css
highlight_theme = "InspiredGitHub"

[feeds]
limit = 20                 # newest entries per feed
filename = "atom.xml"

[sitemap]
enabled = true

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

## Slugs

The default slugifier lowercases and keeps every alphanumeric character of every script, turning everything else into `-`. A Hebrew tag therefore gets a Hebrew URL. `ascii` transliterates first, for sites that want Latin-only paths. The mode is set separately for paths, taxonomies and anchors.
