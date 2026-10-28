+++
title = "Code on right-to-left pages"
description = "Code blocks and inline code stay left to right in Hebrew."
weight = 8
+++

A language declares its direction in `rssite.toml` (`direction = "rtl"`). On
its pages, rssite gives every code block `<pre dir="ltr">` and every inline
code span `<code dir="ltr">`. Without that, the bidi algorithm reorders code
inside right-to-left text: punctuation moves to the wrong end, and
`a < b` can read as `b > a`.

Nothing needs to be written for it. Open
[the Hebrew version of this page](@/code-direction.he.md) (or use the
language switch at the top) and compare.
