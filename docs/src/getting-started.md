# Getting started

rssite is a static site generator written in Rust. It reads markdown (or HTML, or a template) under `content/`, renders it through Tera templates, and writes a tree of files. What sets it apart is that it can list every file it will write, with the inputs of each, before it writes any of them. That list is the manifest; see [The manifest](manifest.md).

## Install

```bash
cargo install rssite
```

## A first site

```bash
mkdir mysite && cd mysite
rssite init
rssite build
```

To see what rssite's markdown can do before writing any, the repository's `demos/` directory is a site with one page per feature: `rssite -C demos serve`, then open <http://127.0.0.1:8000/>.

`init` writes a working single-language site: `rssite.toml`, one translation file, the six templates the generator looks for, a root section, a blog section with one post, and a stylesheet. `build` renders it into `_site/`.

```text
rssite.toml           configuration
i18n/en.toml          strings and date names for English
templates/            Tera templates
content/_index.en.md  the root section
content/blog/         a section with one post
sass/style.scss       compiled to style.css
static/               copied as is (when present)
_site/                the output
```

## Commands

Running `rssite` with no command prints this list.

| Command | What it does |
|---|---|
| `plan` | Print the manifest of every file a build would write, with the sources of each |
| `build` | Build the site, writing only the files whose content changed |
| `check` | Render every page twice without writing anything: templates, links, translations, determinism |
| `query` | Print the parsed site (pages, sections, taxonomies) as JSON |
| `clean` | Remove every file the plan lists from the output directory |
| `serve` | Build, serve the output directory on 127.0.0.1, rebuild when a source changes and reload open pages |
| `init` | Write a minimal working site |
| `version` | Print version and build details |

Two flags apply to every command: `-C <dir>` runs against a site root other than the current directory, and `--drafts` includes pages and sections marked `draft = true`. `build` takes `--plan` to print the manifest as well, and `--verify-determinism` to render everything twice and fail if any file differs. `serve` takes `--port` (default 8000), `--no-watch` to serve without rebuilding, and `--no-reload` to keep open pages from reloading after a rebuild. The reload works by adding one `<script>` to each HTML response, never to the files: what is on disk is what would be deployed.

## What a build writes

For a site with languages `en` and `he`:

```text
index.html                 root: templates/root.html, or a redirect to the fallback language
404.html                   the fallback language's 404 page
en/index.html              root section
en/404.html
en/atom.xml                language feed
en/blog/index.html         section
en/blog/page/2/index.html  further pagers; there is never a /page/1/
en/blog/post/index.html    page
en/blog/atom.xml           section feed, when the section asks for one
en/tags/index.html         taxonomy list
en/tags/term/index.html    term, with its own pagers and feed
he/...                     the same shape for every other language
sitemap.xml                every page in every language, with hreflang alternates
robots.txt                 generated unless static/robots.txt exists
style.css                  every non-partial file under sass/
highlight.css              the syntax highlighting theme
<static files>             copied verbatim
```

Nothing deletes the output directory. A build writes the planned files and leaves everything else alone, so other tools can write into the same tree. `rssite clean` removes exactly the planned files.
