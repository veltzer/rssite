//! `rssite init`: a small working site to start from, with every template
//! the generator looks for and a translation file, so the first build
//! succeeds and every later change is a diff against something that works.

use anyhow::{Result, bail};
use std::path::Path;

pub const FILES: &[(&str, &str)] = &[
    (
        "rssite.toml",
        r#"base_url = "https://example.org"
title = "My site"
description = "A site built with rssite."
author = "Me"

# Every language is explicit and addressed the same way: /en/, /he/, ...
# Content files are named <name>.<code>.md.
[[languages]]
code = "en"
name = "English"
direction = "ltr"

[[taxonomies]]
name = "tags"

[markdown]
smart_punctuation = false

[feeds]
limit = 20
"#,
    ),
    (
        "i18n/en.toml",
        r#"# Strings for templates: {{ trans(key="nav_blog") }}.
nav_home = "Home"
nav_blog = "Blog"
nav_tags = "Tags"
latest_posts = "Latest posts"
tagged_with = "Tagged"
newer = "Newer"
older = "Older"

# Names the `date` filter uses for %B, %b, %A and %a in this language.
[dates]
months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
"#,
    ),
    (
        "templates/base.html",
        r#"<!DOCTYPE html>
<html lang="{{ lang }}" dir="{{ language.direction }}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{% block title %}{{ language.title }}{% endblock %}</title>
<link rel="canonical" href="{{ current_url }}">
{%- for alternate in translations %}
<link rel="alternate" hreflang="{{ alternate.lang }}" href="{{ alternate.permalink }}">
{%- endfor %}
<link rel="stylesheet" href="{{ get_url(path="style.css") }}">
<link rel="stylesheet" href="{{ get_url(path="highlight.css") }}">
{%- if language.generate_feeds %}
<link rel="alternate" type="application/atom+xml" href="{{ get_url(path=language.url_prefix ~ config.feeds.filename) }}">
{%- endif %}
</head>
<body>
<header>
  <a href="{{ language.url_prefix }}">{{ language.title }}</a>
  <nav>
    <a href="{{ get_url(path="@/blog/_index.md") }}">{{ trans(key="nav_blog") }}</a>
    <a href="{{ get_url(path=language.url_prefix ~ "tags/") }}">{{ trans(key="nav_tags") }}</a>
  </nav>
  <nav class="languages">
    {#- This document in each language, falling back to that language's
        home when it has no translation. `alternates` maps language code
        to permalink. -#}
    {%- for entry in languages %}
    <a href="{{ alternates | get(key=entry.code, default=entry.url_prefix) }}" hreflang="{{ entry.code }}"{% if entry.code == lang %} aria-current="true"{% endif %}>{{ entry.name }}</a>
    {%- endfor %}
  </nav>
</header>
<main>
{% block content %}{% endblock %}
</main>
</body>
</html>
"#,
    ),
    (
        "templates/index.html",
        r#"{% extends "base.html" %}
{% block content %}
<h1>{{ section.title }}</h1>
{{ section.content | safe }}
{%- set blog = get_section(path="blog/_index.md") %}
<h2>{{ trans(key="latest_posts") }}</h2>
<ul>
{%- for post in blog.pages %}
{%- if loop.index > 10 %}{% break %}{% endif %}
  <li><a href="{{ post.permalink }}">{{ post.title }}</a> <time datetime="{{ post.date }}">{{ post.date | date(format="%-d %B %Y") }}</time></li>
{%- endfor %}
</ul>
{% endblock %}
"#,
    ),
    (
        "templates/section.html",
        r#"{% extends "base.html" %}
{% block title %}{{ section.title }} &middot; {{ language.title }}{% endblock %}
{% block content %}
<h1>{{ section.title }}</h1>
{{ section.content | safe }}
{%- set posts = section.pages %}
{%- if paginator %}{% set posts = paginator.pages %}{% endif %}
<ul>
{%- for post in posts %}
  <li><a href="{{ post.permalink }}">{{ post.title }}</a> <time datetime="{{ post.date }}">{{ post.date | date(format="%-d %b %Y") }}</time></li>
{%- endfor %}
</ul>
{%- if paginator %}
<nav>
  {%- if paginator.previous %}<a href="{{ paginator.previous }}">{{ trans(key="newer") }}</a>{% endif %}
  <span>{{ paginator.current_index }} / {{ paginator.number_pagers }}</span>
  {%- if paginator.next %}<a href="{{ paginator.next }}">{{ trans(key="older") }}</a>{% endif %}
</nav>
{%- endif %}
{% endblock %}
"#,
    ),
    (
        "templates/page.html",
        r#"{% extends "base.html" %}
{% block title %}{{ page.title }} &middot; {{ language.title }}{% endblock %}
{% block content %}
<article>
<h1>{{ page.title }}</h1>
<p>
  <time datetime="{{ page.date }}">{{ page.date | date(format="%-d %B %Y") }}</time>
  {%- for tag in page.taxonomies.tags %} <a href="{{ tag.permalink }}">{{ tag.name }}</a>{% endfor %}
</p>
{{ page.content | safe }}
<nav>
  {%- if page.previous %}<a href="{{ page.previous.permalink }}">{{ page.previous.title }}</a>{% endif %}
  {%- if page.next %}<a href="{{ page.next.permalink }}">{{ page.next.title }}</a>{% endif %}
</nav>
</article>
{% endblock %}
"#,
    ),
    (
        "templates/taxonomy_list.html",
        r#"{% extends "base.html" %}
{% block title %}{{ trans(key="nav_tags") }} &middot; {{ language.title }}{% endblock %}
{% block content %}
<h1>{{ trans(key="nav_tags") }}</h1>
<ul>
{%- for term in terms %}
  <li><a href="{{ term.permalink }}">{{ term.name }}</a> ({{ term.page_count }})</li>
{%- endfor %}
</ul>
{% endblock %}
"#,
    ),
    (
        "templates/taxonomy_single.html",
        r#"{% extends "base.html" %}
{% block title %}{{ term.name }} &middot; {{ language.title }}{% endblock %}
{% block content %}
<h1>{{ trans(key="tagged_with") }} {{ term.name }}</h1>
<ul>
{%- for post in term.pages %}
  <li><a href="{{ post.permalink }}">{{ post.title }}</a></li>
{%- endfor %}
</ul>
{% endblock %}
"#,
    ),
    (
        "content/_index.en.md",
        r#"+++
title = "My site"
+++

Welcome. This is the root section in English; its body is rendered by `templates/index.html`.
"#,
    ),
    (
        "content/blog/_index.en.md",
        r#"+++
title = "Blog"
sort_by = "date"
paginate_by = 10
generate_feeds = true
+++
"#,
    ),
    (
        "content/blog/hello.en.md",
        r#"+++
title = "Hello, world"
date = 2026-01-01
description = "The first post."

[taxonomies]
tags = ["meta"]
+++

This is the first post. Link to another page with `[text](@/blog/hello.md)`;
a link to a page that does not exist fails the build.

<!-- more -->

Everything after the marker is left out of the summary.

```rust
fn main() {
    println!("hello");
}
```
"#,
    ),
    (
        "sass/style.scss",
        r#"$width: 42rem;

body {
  max-width: $width;
  margin: 2rem auto;
  padding: 0 1rem;
  font-family: system-ui, sans-serif;
  line-height: 1.6;
}

nav a + a {
  margin-inline-start: 1rem;
}

pre {
  overflow-x: auto;
  padding: 1rem;
}
"#,
    ),
];

pub fn init(dir: &Path) -> Result<usize> {
    if dir.join(crate::config::CONFIG_FILE).exists() {
        bail!(
            "{} already exists in {}",
            crate::config::CONFIG_FILE,
            dir.display()
        );
    }
    // Refuse before writing anything, so a refusal leaves no half-site.
    for (name, _) in FILES {
        let path = dir.join(name);
        if path.exists() {
            bail!(
                "{} already exists; init only writes into an empty site",
                path.display()
            );
        }
    }
    let mut written = 0;
    for (name, content) in FILES {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        written += 1;
    }
    Ok(written)
}
