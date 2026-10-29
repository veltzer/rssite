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
math = true                    # overrides the config for this page, see Markdown

[taxonomies]
tags = ["religion", "ethics"]  # every taxonomy must be declared in rssite.toml

[extra]
anything = "goes here"         # templates read page.extra
+++
```

A section index (`_index.<lang>.md`) takes `title`, `description`, `template`, `page_template`, `sort_by` (`date`, `weight`, `title`, `none`), `paginate_by`, `paginate_path`, `generate_feeds`, `year_archives`, `archive_template`, `pager_aliases`, `require_translations`, `draft`, `weight`, `aliases`, `updated`, `body_from`, `smart_punctuation`, `math`, `extra` and `cascade` (below). A section that sorts by date and contains a page without one is an error, not a silently dropped page.

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

A section index can set front matter for everything below it with a `[cascade]` table: every page in the section and in its subsections, at any depth, and the subsections themselves.

```toml
+++
title = "Courses"
[cascade]
math = true                    # every lesson below has math on
smart_punctuation = true
[cascade.extra]
banner = "Part of the courses"
+++
```

A cascade can set `math`, `smart_punctuation` and `extra`, the settings that mean the same on every page below; anything else is an error naming the key. (`template` is not one of them: a section's `page_template` chooses its own pages' template.) The nearest section wins: a subsection's cascade overrides its parent's key by key, `extra` merging key by key too, and a page's own front matter overrides every cascade. The cascade does not apply to the section that declares it, whose own front matter already says what it wants. Cascades are per language, like everything in a section index: `_index.en.md`'s reaches the English pages only. A page lists the section files whose cascade it took as sources in the manifest, so editing a cascade rebuilds exactly the pages under it.

`require_translations = true` on a section index makes a missing translation an error: every page of the section must exist in every language the section itself exists in, and the error names the page and the language it lacks.

## Links between pages

A markdown link to `@/blog/post.md` resolves to that post's permalink in the current language. `@/blog/post.he.md` names the language explicitly. `@/blog/_index.md` links to a section. A fragment (`@/blog/post.md#heading`) is kept. A link whose target does not exist fails the build and names the file.

Headings get ids from their text (`## Heading one` becomes `heading-one`, a repeat becomes `heading-one-1`), and the page's table of contents is available to templates as `page.toc`.

`<!-- more -->` in a body ends the summary; `page.summary` is the HTML before it, and feeds use it instead of the full content when present.

## Markdown

CommonMark, plus tables, footnotes, strikethrough, task lists, heading attributes (`## Title {#id}`) and the following.

GitHub callouts: a blockquote whose first line is `[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]` or `[!CAUTION]` renders as `<blockquote class="markdown-alert-note">` (and so on). rssite writes no title into it, because the word differs per language: the site's stylesheet gives each class its look and, if it wants one, its label.

```markdown
> [!WARNING]
> This deletes the branch.
```

Definition lists: a term on one line, its definition on the next after `: `.

```markdown
Ownership
: Every value has exactly one owner.
```

That makes any line starting with `:` right after a line of text a definition, so a paragraph wrapped to put a colon (or an emoji shortcode) first on a line needs rewrapping.

Math, when `[markdown] math` or the page's `math` is on: `$…$` becomes `<span class="math math-inline">` and `$$…$$` becomes `<span class="math math-display">`, the TeX inside escaped and otherwise untouched. A renderer on the page (KaTeX's auto-render, MathJax) turns them into formulas. It is off by default because with it on, two dollar amounts in one sentence are a formula.

Code blocks take options after the language, separated by commas, in zola's syntax:

````markdown
```rust,linenos,linenostart=10,hl_lines=1 3-4
fn main() {}
```
````

`title=src/main.rs` (quotes optional, for a name with spaces) puts a caption above the block: it becomes `<figure class="code-block"><figcaption>src/main.rs</figcaption><pre>…</pre></figure>`, styled by the site.

`linenos` numbers the lines (from `linenostart`, default 1), and `hl_lines` marks lines of the block, counted from 1 whatever `linenostart` says. Each line then becomes `<span class="line">`, with `hl` added on a marked line and `data-line="N"` when numbered; `highlight.css` styles them, and a site without highlighting styles the same classes itself. The numbers are drawn by CSS, so copying the code does not copy them. A malformed `hl_lines`, or one naming a line past the end of the block, is an error. Other options, such as mdBook's `ignore`, are left alone, and a block without options renders exactly as before.

Diagrams: a fenced block whose language is in `[markdown] diagram_languages` (`["mermaid"]` by default) is neither highlighted nor numbered. Its source, escaped, goes into `<pre class="mermaid">` for a script on the page (mermaid.js) to draw.

On a page whose language has `direction = "rtl"`, code blocks get `<pre dir="ltr">` (a titled one, `<figure dir="ltr">`) and inline code gets `<code dir="ltr">`, so the bidi algorithm does not reorder punctuation and operators inside them. Nothing needs to be written for it.

`[markdown] heading_anchors = "start"` or `"end"` adds a link to each heading, to itself: `<a class="heading-anchor" href="#id" aria-hidden="true" tabindex="-1">#</a>`, before or after the heading's text. Start and end rather than left and right, so a right-to-left page puts it on the correct side. It is not part of the heading's text in `page.toc`. The default, `"none"`, adds nothing.

`[markdown] emoji = true` turns GitHub's shortcodes into emoji: `:rocket:` becomes 🚀. A name that is not a shortcode, and a time like `10:30:00`, stays as written; code is never touched.

Links that leave the site, absolute `http` or `https` URLs outside `base_url`, can get `target="_blank"` (with `rel="noopener"`), `rel="nofollow"` and `rel="noreferrer"`, each switched on in `[markdown]`. Links in raw HTML are left as they are.

## Shortcodes

A shortcode is a template in `templates/shortcodes/` (or a theme's) called from markdown, in zola's syntax. `{{ name(key=value, ...) }}` renders `templates/shortcodes/name.html` in place; `{% name(key=value) %}...{% end %}` does the same with the text between as `body`.

```markdown
Press {{ kbd(keys="Ctrl+C") }} to copy.

{{ youtube(id="aqz-KE-bpKQ", title="Big Buck Bunny") }}

{% details(summary="More") %}
Anything *markdown* here.
{% end %}
```

Arguments are `key=value` pairs separated by commas, and each value is a TOML value: a string in double or single quotes, a number, `true` or `false`, an array. The template sees each argument as a variable, plus `nth` (1 for the first call of that shortcode in the body, 2 for the next), `body` for a call with one (the raw text, which the template can pass through `markdown`), `page` or `section`, and everything else a page template has: `lang`, `config`, the functions. An argument named like one of those replaces it.

A call alone on its line is a block: its output is not wrapped in a paragraph. A call in the middle of a line stays in its sentence. Calls inside code blocks and inline code are text, so a page can show the syntax; so is a `{{` or `{%` that is not followed by a name and an argument list. Calling a shortcode that has no template, arguments that are not TOML values, and a body call without its `{% end %}` each fail the build with the file, the line and what was expected. Calls do not nest.

A page lists the templates of the shortcodes it calls (and what they include) as its sources, so editing a shortcode rebuilds the pages that call it. Shortcode output is part of the page's content and summary, and so of its feeds; it is not part of the text the page's meta description and search index entry are made from. The `markdown` template filter leaves calls as text.

[The demo site](https://github.com/veltzer/rssite/tree/master/demos) has three: `kbd`, `details` and `youtube`.

## Images

With `[images] enabled = true`, a markdown image of a local JPEG, PNG, WebP or GIF file (a static file, a published one or a content asset, by an absolute, relative or `base_url` URL) is written as a `<picture>`:

```html
<picture>
  <source type="image/webp" srcset="/img/a-480w.webp 480w, /img/a-960w.webp 960w" sizes="100vw" />
  <img src="/img/a-960w.jpg" srcset="/img/a-480w.jpg 480w, /img/a-960w.jpg 960w" sizes="100vw"
       width="960" height="640" alt="..." loading="lazy" decoding="async" />
</picture>
```

- **Widths.** Every configured width smaller than the image's own, and the image itself when it is no wider than the largest width. An image wider than every width is never sent whole; its largest variant takes its place in `src`.
- **Formats.** Each of `formats` (`"webp"`, `"avif"`) at the same widths, as `<source>` elements in that order. A browser takes the first type it supports and falls back to the original's format. AVIF is the smallest and the slowest to encode.
- **`width` and `height`** of the `src` image, after its EXIF orientation, so the browser reserves the space before the image arrives. Give images `max-width: 100%; height: auto` in CSS.
- **`loading="lazy"`** unless `lazy = false`, and `decoding="async"`.

GIFs get `width`, `height` and lazy loading but no variants, since resizing an animation would keep its first frame. SVG files, images on other hosts, `data:` URLs and URLs with a query or fragment are left as written, and so are images in raw HTML.

Each variant is its own output, named after the original with its width (`a.jpg` gives `a-480w.jpg`, `a-480w.webp`), in the same directory, with the image and `rssite.toml` as its sources. The page lists the images it shows as sources too, since its `width` and `height` come from them. The original is still published as it is. A variant that would land on an existing file fails the build with both named.

Encoding is the slow part of a build with images, AVIF most of all, so encoded variants are kept in `$XDG_CACHE_HOME/rssite/images` (`~/.cache/rssite/images`), keyed by the image's bytes, the variant and rssite's version. Deleting the directory is always safe.

Images in templates are not processed; `get_image_metadata` gives a template an image's size.

## Sorting and pagination

`sort_by = "date"` sorts newest first. `paginate_by = 10` writes the first ten posts at the section's URL and the rest at `page/2/`, `page/3/` and so on. There is no `page/1/`: the first pager is the section itself.

`pager_aliases = "/page/{n}/"` writes a redirect per pager, `{n}` being its number: `/page/1/` to the section, `/page/2/` to its second pager, and so on for as many pagers as the section has. It is for URLs from before the section moved, which a hand-written list would stop covering as the section grows. It needs `paginate_by`.

`year_archives = true` writes one listing per year of the section's dated pages at `<section>/<year>/`, rendered with `archive_template` (default `archive.html`), and gives the section a `years` list for templates to link from. A year's page in one language has the same year in the other languages as its translations, where those have pages that year, so the alternates and the language switcher work there as everywhere.

Within a section's order, every page has `previous` and `next`.

## Drafts

`draft = true` on a page or a section leaves it out of the build. A draft section takes its pages and assets with it. `--drafts` includes them all.
