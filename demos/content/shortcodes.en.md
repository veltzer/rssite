+++
title = "Shortcodes"
description = "Templates called from markdown: {{ name(args) }} and {% name(args) %}...{% end %}."
weight = 11
+++

A shortcode is a template in `templates/shortcodes/`, called from markdown
in zola's syntax. This site has three; their sources are in
`demos/templates/shortcodes/`.

## Inline: `{{ kbd(keys="...") }}`

To reopen a closed tab press {{ kbd(keys="Ctrl+Shift+T") }}, and to copy
press {{ kbd(keys="Ctrl+C") }}. The call sits in the middle of a sentence and
its output stays there.

## With a body: `{% details(summary="...") %}...{% end %}`

The text between the call and `{% end %}` reaches the template as `body`;
this one renders it as markdown inside a `<details>`.

{% details(summary="Click to see how the arguments work") %}
Arguments are `key=value` pairs, and every value is a **TOML value**:

- strings, `"double"` or `'single'` quoted
- numbers and `true` / `false`
- arrays, `[1, 2, 3]`

A template also gets `nth` (1 for the first call of that shortcode on the
page), `page`, `lang` and everything else a page template has.
{% end %}

{% details(summary="This one starts open", open=true) %}
`open=true` is just another argument; the template decides what it means.
{% end %}

## An embed: `{{ youtube(id="...") }}`

A call alone on its line is a block, not wrapped in a paragraph:

{{ youtube(id="aqz-KE-bpKQ", title="Big Buck Bunny") }}

## Showing a call without making one

Inside code, a call is text: `{{ kbd(keys="Ctrl+C") }}` above is in
backticks. A call to a shortcode that does not exist fails the build and
names the file, the line, and the template it looked for.
