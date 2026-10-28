+++
title = "Callouts"
description = "GitHub's [!NOTE] blockquotes."
weight = 2
+++

A blockquote whose first line is `[!KIND]` becomes
`<blockquote class="markdown-alert-kind">`. rssite adds no label, because the
word depends on the language; `sass/style.scss` supplies the label and the
colour here.

> [!NOTE]
> Useful information the reader should notice even when skimming.

> [!TIP]
> A way to do the thing better or more easily.

> [!IMPORTANT]
> What the reader needs to know to succeed.

> [!WARNING]
> Something that needs attention right away.

> [!CAUTION]
> The risks or bad outcomes of an action.

A callout holds any markdown:

> [!TIP]
> Run `rssite serve` and edit this file: the page **reloads by itself**.
>
> - lists work
> - so do [links](@/code.md)

> An ordinary blockquote, without a kind, stays as it was.
