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
| --- | --- |
| `plan` | Print the manifest of every file a build would write, with the sources of each |
| `build` | Build the site, writing only the files whose content changed |
| `check` | Render every page twice without writing anything: templates, links, translations, determinism |
| `query` | Print the parsed site (pages, sections, taxonomies) as JSON |
| `clean` | Remove every file the plan lists from the output directory; with `--unknown`, list the files there that the plan does not list |
| `serve` | Build, serve the output directory on 127.0.0.1, rebuild when a source changes and reload open pages |
| `init` | Write a minimal working site |
| `version` | Print version and build details |

Two flags apply to every command: `-C <dir>` runs against a site root other than the current directory, and `--drafts` includes pages and sections marked `draft = true`. `build` takes `--plan` to print the manifest as well, and `--verify-determinism` to render everything twice and fail if any file differs. `serve` takes `--port` (default 8000), `--no-watch` to serve without rebuilding, `--no-reload` to keep open pages from reloading after a rebuild, and `--keep-base-url` to leave the site's absolute links pointing at `base_url`. The reload works by adding one `<script>` to each HTML response, never to the files: what is on disk is what would be deployed.

The build writes `base_url` into every permalink, so a served page would otherwise send you to the live site on the first click. `serve` therefore replaces `base_url` with its own address (the one in your browser's address bar) in every text response: HTML, CSS, JavaScript, JSON, XML, SVG and plain text. Like the reload script, this is done to the response and never to the files. A `base_url` with a path (`https://example.org/site`) maps to the server's root, which is where the output directory is served from.

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

A build also removes the files the previous build wrote that it no longer does: a deleted page does not stay behind to be deployed. It knows them from a record it keeps per output directory in `$XDG_CACHE_HOME/rssite/outputs` (`~/.cache/rssite/outputs`), outside the site; without that record (a first build, a fresh checkout, a cleared cache) it removes nothing, and it never touches a file it did not write.

Two things reach the files that record does not know about:

- `rssite clean --unknown` lists every file in the output directory that the plan does not list, one path per line, and removes nothing. Add `--execute` to remove them. Use it once after an upgrade or a lost cache, after reading the list: a file another tool wrote there is on it too, unless `[prune] keep` names it.
- `[prune] enabled = true` in `rssite.toml` makes the output directory rssite's own: every build removes whatever is there that it did not plan and `keep` does not name, with no record involved. `keep` is a list of globs relative to the output directory for what other tools write into it. Leave this off when you cannot list those files: a file missing from `keep` is deleted on the next build.

Both refuse to run when the output directory holds the site root or one of its source directories, since every source would then be an unknown file.
