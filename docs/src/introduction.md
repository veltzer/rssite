# rssite

Static site generator with a pre-declared output manifest.

rssite renders markdown, HTML and template pages through Tera templates into a static site, in as many languages as the site has, and can list every file it will write before writing any of them. That manifest is what lets a build system cache each page on its own and let other tools write into the same output directory.

It was designed against a concrete bilingual site that had been built with zola; [Lessons from zola](zola-problems.md) is the list of problems it exists to not have. The rest of this book is the user guide: [Getting started](getting-started.md), [Configuration](configuration.md), [Content](content.md), [Templates](templates.md) and [The manifest](manifest.md). [Features](features.md) is the longer wish list, of which the chapters before it describe what is implemented.
