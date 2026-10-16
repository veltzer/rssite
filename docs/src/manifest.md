# The manifest

`rssite plan` prints every file a build would write, each with the files whose content can change it:

```json
{
  "version": 1,
  "outputs": [
    {
      "path": "_site/en/blog/post/index.html",
      "sources": [
        "content/blog/post.en.md",
        "content/blog/post.he.md",
        "i18n/en.toml",
        "rssite.toml",
        "templates/base.html",
        "templates/page.html"
      ]
    }
  ]
}
```

Paths are relative to the site root, sorted, and unique: two outputs claiming one path is an error that names both. The plan is a pure function of the configuration and the source tree. It reads no clock, no environment and no network, and the build iterates the very same list, so what `plan` prints is what `build` writes, no more and no less.

## What a file depends on

- A page depends on its own file, its translations, the file it takes its body from, every content file it links to, its section index, its template with everything that template extends, includes or imports, the literal `load_data` paths in those, the configuration, and the language's translation files (the theme's and the site's, when both exist). A template or a static file that comes from the theme is listed under its theme path, so the manifest says which root was used.
- When a page's template reads other pages (`get_page`, `get_section`, `get_taxonomy`, `section.pages`, `page.previous`, `page.next` and the like), the whole content tree is added, because any file can then change the output.
- A section listing, a year archive, a taxonomy page and a feed depend on every content file of their language.
- The sitemap depends on every content file and the configuration.
- A compiled stylesheet depends on every file under `sass/`, since any of them can be imported.
- A static file, a published file (`[[publish]]`) and a content asset depend on themselves.

## Using it from a build system

[rsconstruct](https://github.com/veltzer/rsconstruct) consumes this format directly:

```toml
[processor.mass_generator.generic.site]
command         = "rssite"
args            = ["build"]
predict_command = "rssite"
predict_args    = ["plan"]
output_dirs     = ["_site"]
```

Each manifest entry becomes a product with its own cache entry, so an unchanged site runs nothing, an edited post rebuilds only the files that list it, and other processors can write into `_site/` beside rssite's files. That last point is why a build never deletes the output directory: `rssite clean` removes exactly the planned files and nothing else.

## Determinism

`rssite check` and `rssite build --verify-determinism` render the whole site twice and fail if any byte differs. Every list a template sees has a defined order, the clock is unreachable from templates, and the output of two builds of the same sources is identical, so a build change can be verified by diffing output.
