+++
title = "Links and headings"
description = "External link options, links to other pages, heading anchors."
weight = 5

[extra]
show_toc = true
+++

## External links

This site sets `external_links_target_blank` and `external_links_no_follow`,
so [the Rust website](https://www.rust-lang.org/) opens in a new tab with
`rel="noopener nofollow"`. A bare URL does too: <https://crates.io/>.

Links that stay on the site are left alone: [the home page](/), and
[this site's absolute URL](http://127.0.0.1:8000/en/).

## Links to other pages

`[math](@/math.md)` gives [math](@/math.md): the content file's permalink,
checked at build time. A link to a file that does not exist fails the build.
With a fragment, `@/code.md#line-numbers` gives
[the line numbers section](@/code.md#line-numbers).

## Heading anchors

Every heading gets an id from its text; the contents box above lists them.

### A heading with a chosen id {#chosen}

`### A heading with a chosen id {#chosen}` sets the id to `chosen`:
[jump to it](#chosen).
