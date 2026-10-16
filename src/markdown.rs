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
}
