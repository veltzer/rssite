+++
title = "Emoji"
description = "GitHub's :shortcodes:, turned on for the site."
weight = 9
+++

This site sets `[markdown] emoji = true`, so GitHub's shortcodes become
emoji, like this :tada:

One trap: a line that starts with a colon right after a line of text is a
definition list's definition, so do not begin a line with a shortcode.

| Written         | Shown           |
|-----------------|-----------------|
| `:rocket:`      | :rocket:        |
| `:white_check_mark:` | :white_check_mark: |
| `:warning:`     | :warning:       |
| `:+1:`          | :+1:            |
| `:israel:`      | :israel:        |

A name it does not know, :no_such_emoji:, stays as written, and so does a
time like 10:30:00. Code is never touched: `:rocket:` above is in backticks.
