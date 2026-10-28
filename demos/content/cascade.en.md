+++
title = "Settings for a whole section"
description = "A section's [cascade] sets front matter for every page below it."
weight = 10
+++

The [course section](@/course/_index.md) of this site has this front matter
in `content/course/_index.en.md`:

```toml
+++
title = "A small course"
sort_by = "weight"
[cascade]
math = true
smart_punctuation = true
[cascade.extra]
banner = "Course: A small course"
+++
```

So every lesson in it has math and smart punctuation on, and carries the
banner (this site's `templates/page.html` shows `page.extra.banner` above
the title), without any lesson saying so.

- [Lesson 1](@/course/lesson-1.md) takes all of it.
- [Lesson 2](@/course/lesson-2.md) sets `smart_punctuation = false` and its
  own `[extra] banner` in its front matter: a page's own settings win.

A cascade can set `math`, `smart_punctuation` and `extra`. A subsection's
cascade overrides its parent's key by key, and a cascade is per language.
