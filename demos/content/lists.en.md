+++
title = "Lists and tables"
description = "Definition lists, task lists, tables, footnotes, strikethrough."
weight = 4
+++

## Definition lists

Ownership
: Every value in Rust has exactly one owner.

Borrowing
: A reference lends a value without taking it.
: A second definition of the same term.

## Task lists

- [x] Write the demo pages
- [x] Build them
- [ ] Read every one

## Tables

| Feature        | Default | Switch                        |
|----------------|:-------:|-------------------------------|
| Callouts       | on      | always                        |
| Math           | off     | `math = true`                 |
| External links | plain   | `external_links_*` in config  |

## Footnotes

rssite renders footnotes[^fn] at the end of the page.

[^fn]: Like this one.

## Strikethrough

~~Zola~~ rssite builds this site.
