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
```

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
