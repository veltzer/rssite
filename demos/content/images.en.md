+++
title = "Images"
description = "Markdown images as <picture>s: smaller widths, AVIF and WebP, width and height, lazy loading."
weight = 12
+++

This site sets `[images] enabled = true`. A markdown image of a local JPEG,
PNG or WebP file is written as a `<picture>`; view this page's source to see
it. The photos are the inside of a Fatar keyboard, from veltzer.org.

![The keyboard opened, its key bed exposed](/images/keyboard-open.jpg "800 x 598, a JPEG")

What rssite wrote for it, from the configuration in `demos/rssite.toml`
(`widths = [320, 640]`, `formats = ["avif", "webp"]`):

- **Smaller widths.** 320 and 640 pixels, the configured widths below the
  photo's own 800. The photo is wider than every width, so its largest
  version is 640 pixels and the 800-pixel original is never sent; it is
  still published, so an old link to it works.
- **AVIF and WebP.** Each width again in both formats, offered in
  `<source>` elements in the order listed. A browser takes the first type
  it knows and falls back to the JPEG.
- **`srcset` and `sizes`.** The browser picks the width that fits how wide
  the image is shown on its screen.
- **`width` and `height`.** The page keeps the image's space before it
  arrives, so the text below does not jump.
- **`loading="lazy"`.** It is fetched when it nears the viewport.

Each variant is a file of its own beside the original
(`keyboard-open-320w.avif`, `keyboard-open-640w.webp`, ...) and an output
of the plan, with the photo as its source: `rssite plan` lists them, and
changing the photo rebuilds them and this page, nothing else.

## A narrower photo

![The keyboard's empty case](/images/keyboard-case.jpg)

This one is 598 pixels wide. It is narrower than 640 but wider than 320,
so it gets a 320-pixel version, and the original stays its largest JPEG.
The AVIF and WebP sets go up to 598 too.

## What is left alone

- Images on other hosts, like the badge below.
- SVG files, which have no pixels to resize.
- GIFs get `width`, `height` and lazy loading but no variants: resizing
  would keep only the first frame of an animation.

![A badge from another host](https://img.shields.io/badge/images-on%20another%20host-blue)

## In templates

`get_image_metadata(path="static/images/keyboard-open.jpg")` returns the
image's `width` and `height`, for an `og:image` or a hand-written `<img>`.
The file becomes a source of every output whose template calls it with a
literal path.
