+++
title = "Math"
description = "TeX between dollar signs, drawn by KaTeX."
weight = 1
math = true
+++

Math is off by default and turned on here by `math = true` in this page's
front matter (or for the whole site by `[markdown] math = true`). rssite
writes each formula into a `<span class="math …">`; the KaTeX script in
`templates/base.html` draws it.

## Inline

Euler's identity, $e^{i\pi} + 1 = 0$, sits in the middle of a sentence. So
does the golden ratio, $\varphi = \frac{1 + \sqrt{5}}{2}$.

## Display

$$\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}$$

$$\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}$$

$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\begin{pmatrix} x \\ y \end{pmatrix} =
\begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}
$$

A formula may run over several lines, but it is still markdown: a line that
is only `=` or `-` would turn the lines above it into a heading, so keep the
sign on the line before.

## Why it is opt-in

On a page without math, "it cost $5, then $10" stays text. With math on, a
dollar sign followed by a non-space opens a formula, so prose about money
would need `\$`. That is why each page decides for itself.
