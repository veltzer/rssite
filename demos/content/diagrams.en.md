+++
title = "Diagrams"
description = "Mermaid blocks, drawn in the browser."
weight = 7
+++

A fenced block in one of `[markdown] diagram_languages` (`["mermaid"]` by
default) is not highlighted: rssite writes its source into
`<pre class="mermaid">`, and the mermaid.js script in `templates/base.html`
draws it.

## A flowchart

```mermaid
graph LR
  A[content/*.md] --> P[rssite plan]
  P --> M[(manifest)]
  M --> B[rssite build]
  B --> S[_site/]
```

## A sequence diagram

```mermaid
sequenceDiagram
  participant E as Editor
  participant S as rssite serve
  participant B as Browser
  E->>S: save a page
  S->>S: rebuild
  B->>S: poll the build generation
  S-->>B: it moved
  B->>B: reload
```

## Another renderer

`diagram_languages = ["mermaid", "dot"]` would leave ` ```dot ` blocks as
`<pre class="dot">` too, for a Graphviz script to draw.
