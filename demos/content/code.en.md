+++
title = "Code blocks"
description = "Highlighting, line numbers and highlighted lines."
weight = 3
+++

Code is highlighted at build time; `highlight.css` holds the colours of
`highlight_theme`. Options follow the language after a comma, in zola's
syntax.

## Plain

```rust
fn main() {
    println!("Hello, world!");
}
```

## Line numbers

` ```python,linenos `

```python,linenos
def fib(n):
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a
```

## Highlighted lines

` ```rust,hl_lines=2 4-5 `: line 2, and lines 4 to 5.

```rust,hl_lines=2 4-5
fn area(r: f64) -> f64 {
    let pi = std::f64::consts::PI;
    // the formula
    let squared = r * r;
    pi * squared
}
```

## Both, starting at line 40

` ```c,linenos,linenostart=40,hl_lines=3 `: `hl_lines` counts lines of the
block, whatever number the first one shows.

```c,linenos,linenostart=40,hl_lines=3
int main(void)
{
    /* a comment that runs
       over two lines keeps its colour */
    return 0;
}
```

Select the code and copy it: the line numbers are drawn by CSS and do not
come along.

## A title

` ```rust,title=src/main.rs ` puts a caption above the block, usually a file
name. The block becomes a `<figure class="code-block">` with a
`<figcaption>`; `sass/style.scss` styles it. Quotes are optional:
`title="my file.rs"`.

```rust,title=src/main.rs,linenos
fn main() {
    println!("Hello from a titled block");
}
```

## A language rssite does not know

` ```nosuchlang,linenos ` is escaped and numbered, just not coloured.

```nosuchlang,linenos
a < b && c > d
```
