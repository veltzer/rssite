//! Markdown to HTML: CommonMark with tables, footnotes, strikethrough and
//! task lists; heading ids and a table of contents; build-time syntax
//! highlighting; `@/` links to other content resolved to permalinks, with a
//! broken link a hard error.

use crate::slug::{SlugMode, slugify};
use anyhow::{Result, bail};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use syntect::html::{ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style};
use syntect::parsing::SyntaxSet;

pub const SUMMARY_MARKER: &str = "<!-- more -->";

#[derive(Clone, Debug, Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    pub title: String,
    pub children: Vec<Heading>,
}

#[derive(Clone, Debug, Default)]
pub struct Rendered {
    pub html: String,
    pub summary: Option<String>,
    pub toc: Vec<Heading>,
    pub word_count: usize,
    /// Content paths (`@/...`) this body links to, for dependency tracking.
    pub links: Vec<String>,
}

pub struct RenderOptions {
    pub smart_punctuation: bool,
    pub highlight: bool,
    pub anchor_mode: SlugMode,
}

/// Resolves an `@/...` content path to a permalink; `None` when no such
/// content exists.
pub type LinkResolver<'a> = dyn Fn(&str) -> Option<String> + 'a;

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The stylesheet for `highlight_theme`, written to `highlight.css`.
pub fn highlight_css(theme: &str) -> Result<String> {
    let themes = syntect::highlighting::ThemeSet::load_defaults();
    let Some(theme) = themes.themes.get(theme) else {
        let mut names: Vec<&String> = themes.themes.keys().collect();
        names.sort();
        bail!(
            "markdown.highlight_theme {theme:?} is not a known theme; known: {}",
            names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    };
    Ok(css_for_theme_with_class_style(theme, ClassStyle::Spaced)?)
}

fn highlight(code: &str, lang: &str) -> Result<String> {
    let set = syntax_set();
    let syntax = if lang.is_empty() {
        None
    } else {
        set.find_syntax_by_token(lang)
    };
    let Some(syntax) = syntax else {
        return Ok(format!(
            "<pre><code{}>{}</code></pre>\n",
            if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", crate::util::escape_html(lang))
            },
            crate::util::escape_html(code)
        ));
    };
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, set, ClassStyle::Spaced);
    for line in syntect::util::LinesWithEndings::from(code) {
        generator.parse_html_for_line_which_includes_newline(line)?;
    }
    Ok(format!(
        "<pre class=\"highlight\"><code class=\"language-{}\">{}</code></pre>\n",
        crate::util::escape_html(lang),
        generator.finalize()
    ))
}

/// Render `markdown`. Links of the form `@/section/page.md` (optionally with
/// a language suffix and a `#fragment`) are resolved through `resolve`.
pub fn render(markdown: &str, options: &RenderOptions, resolve: &LinkResolver) -> Result<Rendered> {
    let mut parser_options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES;
    if options.smart_punctuation {
        parser_options |= Options::ENABLE_SMART_PUNCTUATION;
    }
    let parser = Parser::new_ext(markdown, parser_options);

    let mut events: Vec<Event> = Vec::new();
    let mut links = Vec::new();
    let mut word_count = 0usize;
    let mut headings: Vec<(u8, String, String)> = Vec::new();
    let mut used_ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut heading: Option<(u8, Option<String>, String, usize)> = None;
    let mut code: Option<(String, String)> = None;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split([' ', ',']).next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::Text(text) if code.is_some() => {
                if let Some((_, buffer)) = code.as_mut() {
                    buffer.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let (lang, buffer) = code.take().expect("code block was opened");
                word_count += buffer.split_whitespace().count();
                let html = if options.highlight {
                    highlight(&buffer, &lang)?
                } else {
                    highlight_plain(&buffer, &lang)
                };
                events.push(Event::Html(CowStr::from(html)));
            }
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => {
                heading = Some((
                    level as u8,
                    id.as_ref().map(|i| i.to_string()),
                    String::new(),
                    events.len(),
                ));
                events.push(Event::Start(Tag::Heading {
                    level,
                    id,
                    classes,
                    attrs,
                }));
            }
            Event::End(TagEnd::Heading(level)) => {
                let (lvl, explicit, text, start) = heading.take().expect("heading was opened");
                let base = explicit.unwrap_or_else(|| slugify(&text, options.anchor_mode));
                let base = if base.is_empty() {
                    format!("section-{}", headings.len() + 1)
                } else {
                    base
                };
                let count = used_ids.entry(base.clone()).or_insert(0);
                let id = if *count == 0 {
                    base.clone()
                } else {
                    format!("{base}-{count}")
                };
                *count += 1;
                if let Event::Start(Tag::Heading { id: slot, .. }) = &mut events[start] {
                    *slot = Some(CowStr::from(id.clone()));
                }
                headings.push((lvl, id, text));
                events.push(Event::End(TagEnd::Heading(level)));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest = resolve_dest(&dest_url, resolve, &mut links)?;
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url: CowStr::from(dest),
                    title,
                    id,
                }));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest = resolve_dest(&dest_url, resolve, &mut links)?;
                events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url: CowStr::from(dest),
                    title,
                    id,
                }));
            }
            Event::Text(text) => {
                word_count += text.split_whitespace().count();
                if let Some((_, _, buffer, _)) = heading.as_mut() {
                    buffer.push_str(&text);
                }
                events.push(Event::Text(text));
            }
            Event::Code(text) => {
                word_count += text.split_whitespace().count();
                if let Some((_, _, buffer, _)) = heading.as_mut() {
                    buffer.push_str(&text);
                }
                events.push(Event::Code(text));
            }
            other => events.push(other),
        }
    }

    let mut html = String::with_capacity(markdown.len() * 3 / 2);
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    let summary = html
        .find(SUMMARY_MARKER)
        .map(|at| html[..at].trim_end().to_owned());
    links.sort();
    links.dedup();
    Ok(Rendered {
        html,
        summary,
        toc: build_toc(&headings),
        word_count,
        links,
    })
}

fn highlight_plain(code: &str, lang: &str) -> String {
    let class = if lang.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", crate::util::escape_html(lang))
    };
    format!(
        "<pre><code{class}>{}</code></pre>\n",
        crate::util::escape_html(code)
    )
}

fn resolve_dest(dest: &str, resolve: &LinkResolver, links: &mut Vec<String>) -> Result<String> {
    let Some(content_path) = dest.strip_prefix("@/") else {
        return Ok(dest.to_owned());
    };
    let (path, fragment) = match content_path.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (content_path, None),
    };
    let Some(permalink) = resolve(path) else {
        bail!("link to {dest:?}: no content file matches {path:?}");
    };
    links.push(path.to_owned());
    Ok(match fragment {
        Some(f) => format!("{permalink}#{f}"),
        None => permalink,
    })
}

fn build_toc(headings: &[(u8, String, String)]) -> Vec<Heading> {
    let mut root: Vec<Heading> = Vec::new();
    for (level, id, title) in headings {
        let node = Heading {
            level: *level,
            id: id.clone(),
            title: title.clone(),
            children: Vec::new(),
        };
        insert_heading(&mut root, node);
    }
    root
}

fn insert_heading(siblings: &mut Vec<Heading>, node: Heading) {
    match siblings.last_mut() {
        Some(last) if last.level < node.level => insert_heading(&mut last.children, node),
        _ => siblings.push(node),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RenderOptions {
        RenderOptions {
            smart_punctuation: false,
            highlight: false,
            anchor_mode: SlugMode::Safe,
        }
    }

    fn no_links(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn headings_get_ids_and_toc() {
        let out = render(
            "# One\n\ntext\n\n## Two words\n\n## Two words\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert!(out.html.contains("<h1 id=\"one\">One</h1>"));
        assert!(out.html.contains("<h2 id=\"two-words\">"));
        assert!(out.html.contains("<h2 id=\"two-words-1\">"));
        assert_eq!(out.toc.len(), 1);
        assert_eq!(out.toc[0].children.len(), 2);
        assert_eq!(out.word_count, 6);
    }

    #[test]
    fn summary_marker() {
        let out = render("first\n\n<!-- more -->\n\nrest\n", &opts(), &no_links).unwrap();
        assert_eq!(out.summary.as_deref(), Some("<p>first</p>"));
        assert!(out.html.contains("<p>rest</p>"));
    }

    #[test]
    fn internal_links_resolve_or_fail() {
        let resolve =
            |path: &str| (path == "blog/other.md").then(|| "https://x/en/blog/other/".to_owned());
        let out = render(
            "[a](@/blog/other.md#frag) [b](https://e.org/)",
            &opts(),
            &resolve,
        )
        .unwrap();
        assert!(out.html.contains("href=\"https://x/en/blog/other/#frag\""));
        assert!(out.html.contains("href=\"https://e.org/\""));
        assert_eq!(out.links, vec!["blog/other.md".to_owned()]);
        assert!(render("[a](@/blog/missing.md)", &opts(), &resolve).is_err());
    }

    #[test]
    fn code_blocks_highlight() {
        let highlighted = RenderOptions {
            highlight: true,
            ..opts()
        };
        let out = render("```rust\nfn main() {}\n```\n", &highlighted, &no_links).unwrap();
        assert!(out.html.contains("class=\"highlight\""));
        assert!(out.html.contains("language-rust"));
        let plain = render("```\nx < y\n```\n", &opts(), &no_links).unwrap();
        assert!(plain.html.contains("x &lt; y"));
    }

    #[test]
    fn hebrew_heading_keeps_its_letters() {
        let out = render("## שלום עולם\n", &opts(), &no_links).unwrap();
        assert!(out.html.contains("id=\"שלום-עולם\""));
    }

    #[test]
    fn ascii_anchors_transliterate_headings() {
        let ascii = RenderOptions {
            anchor_mode: SlugMode::Ascii,
            ..opts()
        };
        let out = render("## Café au lait\n", &ascii, &no_links).unwrap();
        assert!(out.html.contains("id=\"cafe-au-lait\""), "{}", out.html);
    }

    #[test]
    fn an_explicit_heading_id_wins_and_still_counts_for_duplicates() {
        let out = render(
            "## Intro {#start}\n\n## start\n\n## Other {#start}\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert!(
            out.html.contains("<h2 id=\"start\">Intro</h2>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<h2 id=\"start-1\">start</h2>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<h2 id=\"start-2\">Other</h2>"),
            "{}",
            out.html
        );
        assert_eq!(out.toc.len(), 3);
        assert_eq!(out.toc[0].id, "start");
        assert_eq!(out.toc[0].title, "Intro");
    }

    #[test]
    fn a_heading_without_letters_gets_a_numbered_id() {
        let out = render("## !!!\n\n## ???\n\n## ...\n", &opts(), &no_links).unwrap();
        assert!(out.html.contains("id=\"section-1\""), "{}", out.html);
        assert!(out.html.contains("id=\"section-2\""), "{}", out.html);
        assert!(out.html.contains("id=\"section-3\""), "{}", out.html);
    }

    #[test]
    fn heading_text_includes_code_spans_and_emphasis() {
        let out = render("## Using `cargo build` *now*\n", &opts(), &no_links).unwrap();
        assert!(
            out.html.contains("id=\"using-cargo-build-now\""),
            "{}",
            out.html
        );
        assert_eq!(out.toc[0].title, "Using cargo build now");
    }

    #[test]
    fn the_toc_nests_by_level_and_recovers_from_skips() {
        let out = render(
            "# A\n\n### A.x\n\n## A.1\n\n### A.1.a\n\n#### A.1.a.i\n\n## A.2\n\n# B\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert_eq!(out.toc.len(), 2);
        let a = &out.toc[0];
        assert_eq!(a.title, "A");
        assert_eq!(a.level, 1);
        let titles: Vec<&str> = a.children.iter().map(|h| h.title.as_str()).collect();
        assert_eq!(
            titles,
            ["A.x", "A.1", "A.2"],
            "a skipped level nests under the last ancestor"
        );
        assert_eq!(a.children[1].children.len(), 1);
        assert_eq!(a.children[1].children[0].children[0].title, "A.1.a.i");
        assert_eq!(out.toc[1].title, "B");
        assert!(out.toc[1].children.is_empty());
    }

    #[test]
    fn a_body_without_headings_has_an_empty_toc_and_no_summary() {
        let out = render("just text\n", &opts(), &no_links).unwrap();
        assert!(out.toc.is_empty());
        assert!(out.summary.is_none());
        assert_eq!(out.word_count, 2);
        assert!(out.links.is_empty());
    }

    #[test]
    fn the_summary_is_the_html_before_the_marker() {
        let out = render(
            "# Title\n\nfirst *para*\n\nsecond\n\n<!-- more -->\n\nrest\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        let summary = out.summary.unwrap();
        assert!(summary.starts_with("<h1 id=\"title\">Title</h1>"));
        assert!(summary.ends_with("<p>second</p>"), "{summary}");
        assert!(!summary.contains("rest"));
        assert!(out.html.contains("<p>rest</p>"));
        // Only the first marker counts.
        let twice = render(
            "a\n\n<!-- more -->\n\nb\n\n<!-- more -->\n\nc\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert_eq!(twice.summary.as_deref(), Some("<p>a</p>"));
    }

    #[test]
    fn commonmark_extensions_are_on() {
        let out = render(
            "| a | b |\n|---|---|\n| 1 | 2 |\n\n~~gone~~\n\n- [x] done\n- [ ] todo\n\nnote[^1]\n\n[^1]: the note\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert!(out.html.contains("<table>"), "{}", out.html);
        assert!(out.html.contains("<del>gone</del>"), "{}", out.html);
        assert!(out.html.contains("type=\"checkbox\""), "{}", out.html);
        assert!(out.html.contains("checked"), "{}", out.html);
        assert!(out.html.contains("footnote"), "{}", out.html);
    }

    #[test]
    fn smart_punctuation_is_opt_in() {
        let text = "\"quoted\" -- and...\n";
        let plain = render(text, &opts(), &no_links).unwrap();
        assert!(
            plain.html.contains("\"quoted\" -- and..."),
            "{}",
            plain.html
        );
        let smart = RenderOptions {
            smart_punctuation: true,
            ..opts()
        };
        let curly = render(text, &smart, &no_links).unwrap();
        assert!(curly.html.contains("“quoted” – and…"), "{}", curly.html);
    }

    #[test]
    fn raw_html_passes_through() {
        let out = render(
            "<div class=\"x\"><b>bold</b></div>\n\n<span>*not em*</span>\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert!(out.html.contains("<div class=\"x\"><b>bold</b></div>"));
        // Inline HTML in a paragraph is still markdown-processed around it.
        assert!(
            out.html.contains("<span><em>not em</em></span>"),
            "{}",
            out.html
        );
    }

    #[test]
    fn images_and_links_both_resolve_content_paths() {
        let resolve = |path: &str| match path {
            "blog/a.md" => Some("https://x/en/blog/a/".to_owned()),
            "blog/pic.png" => Some("https://x/en/blog/pic.png".to_owned()),
            _ => None,
        };
        let out = render(
            "![alt](@/blog/pic.png) [a](@/blog/a.md) [again](@/blog/a.md) [rel](other/) [abs](/x/) [mail](mailto:a@b)",
            &opts(),
            &resolve,
        )
        .unwrap();
        assert!(
            out.html
                .contains("<img src=\"https://x/en/blog/pic.png\" alt=\"alt\""),
            "{}",
            out.html
        );
        assert!(out.html.contains("href=\"https://x/en/blog/a/\""));
        assert!(out.html.contains("href=\"other/\""));
        assert!(out.html.contains("href=\"/x/\""));
        assert!(out.html.contains("href=\"mailto:a@b\""));
        assert_eq!(
            out.links,
            vec!["blog/a.md".to_owned(), "blog/pic.png".to_owned()],
            "sorted, deduplicated"
        );
        let broken = render("![x](@/blog/missing.png)", &opts(), &resolve).unwrap_err();
        assert!(
            broken
                .to_string()
                .contains("no content file matches \"blog/missing.png\"")
        );
    }

    #[test]
    fn a_fragment_on_a_content_link_is_kept() {
        let resolve = |path: &str| (path == "a.md").then(|| "https://x/en/a/".to_owned());
        let out = render("[a](@/a.md#sec-2)", &opts(), &resolve).unwrap();
        assert!(out.html.contains("href=\"https://x/en/a/#sec-2\""));
        assert_eq!(out.links, vec!["a.md".to_owned()]);
    }

    #[test]
    fn code_blocks_count_their_words_and_keep_the_language_class() {
        let out = render(
            "```python\nprint('hi there')\n```\n\n    indented\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert!(
            out.html.contains(
                "<pre><code class=\"language-python\">print(&#39;hi there&#39;)\n</code></pre>"
            ),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<pre><code>indented\n</code></pre>"),
            "{}",
            out.html
        );
        assert_eq!(out.word_count, 3);
    }

    #[test]
    fn fenced_info_strings_take_only_the_language_token() {
        let highlighted = RenderOptions {
            highlight: true,
            ..opts()
        };
        let out = render("```rust,ignore\nlet x = 1;\n```\n", &highlighted, &no_links).unwrap();
        assert!(out.html.contains("class=\"language-rust\""), "{}", out.html);
        let out = render("```sh title=x\necho hi\n```\n", &highlighted, &no_links).unwrap();
        assert!(out.html.contains("class=\"language-sh\""), "{}", out.html);
    }

    #[test]
    fn an_unknown_language_is_escaped_plain_code_even_when_highlighting() {
        let highlighted = RenderOptions {
            highlight: true,
            ..opts()
        };
        let out = render("```nosuchlang\na < b\n```\n", &highlighted, &no_links).unwrap();
        assert!(
            out.html
                .contains("<pre><code class=\"language-nosuchlang\">a &lt; b\n</code></pre>"),
            "{}",
            out.html
        );
        let out = render("```\"><script>\nx\n```\n", &highlighted, &no_links).unwrap();
        assert!(
            !out.html.contains("<script>"),
            "the class is escaped:\n{}",
            out.html
        );
    }

    #[test]
    fn highlighted_code_is_wrapped_in_spans_with_classes() {
        let highlighted = RenderOptions {
            highlight: true,
            ..opts()
        };
        let out = render(
            "```rust\nfn main() { let s = \"<x>\"; }\n```\n",
            &highlighted,
            &no_links,
        )
        .unwrap();
        assert!(
            out.html
                .starts_with("<pre class=\"highlight\"><code class=\"language-rust\">")
        );
        assert!(out.html.contains("<span class=\""), "{}", out.html);
        assert!(
            out.html.contains("&lt;x&gt;"),
            "code is escaped:\n{}",
            out.html
        );
        assert!(!out.html.contains("style=\""), "classes, not inline styles");
    }

    #[test]
    fn highlight_css_knows_its_themes() {
        let css = highlight_css("InspiredGitHub").unwrap();
        assert!(css.contains(".keyword"), "{css}");
        let error = highlight_css("nope").unwrap_err().to_string();
        assert!(error.contains("\"nope\" is not a known theme"), "{error}");
        assert!(
            error.contains("InspiredGitHub"),
            "the known names are listed:\n{error}"
        );
        assert!(error.contains("base16-ocean.dark"), "{error}");
    }

    #[test]
    fn word_count_covers_text_code_spans_and_blocks() {
        let out = render(
            "one two `three four` **five**\n\n```\nsix seven\n```\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert_eq!(out.word_count, 7);
    }

    #[test]
    fn heading_structs_serialize_for_templates() {
        let out = render("# T\n\n## U\n", &opts(), &no_links).unwrap();
        let json = serde_json::to_value(&out.toc).unwrap();
        assert_eq!(json[0]["id"], "t");
        assert_eq!(json[0]["level"], 1);
        assert_eq!(json[0]["children"][0]["title"], "U");
    }
}
