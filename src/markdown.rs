//! Markdown to HTML: CommonMark with tables, footnotes, strikethrough, task
//! lists, definition lists, GitHub callouts and (opt-in) math and emoji
//! shortcodes; heading ids, optional anchor links and a table of contents;
//! build-time syntax highlighting with optional line numbers, highlighted
//! lines and titles; diagram blocks left for a client-side renderer; code kept
//! left to right on right-to-left pages; `@/` links to other content resolved
//! to permalinks, with a broken link a hard error; optional `rel`/`target` on
//! links that leave the site.

use crate::slug::{SlugMode, slugify};
use anyhow::{Context, Result, bail};
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use syntect::html::{
    ClassStyle, ClassedHTMLGenerator, css_for_theme_with_class_style, line_tokens_to_classed_spans,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxSet};

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
    /// The shortcode calls in the body, in order; call `n` is the marker
    /// `shortcode::marker(n)` in `html` until it is rendered.
    pub shortcodes: Vec<crate::shortcode::Call>,
}

#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub smart_punctuation: bool,
    pub highlight: bool,
    pub anchor_mode: SlugMode,
    /// `$…$` and `$$…$$` become `<span class="math math-inline">` and
    /// `<span class="math math-display">` for a client-side renderer.
    pub math: bool,
    pub external_links: ExternalLinks,
    /// Links under this URL are the site's own, never external.
    pub base_url: String,
    /// Fenced block languages written as `<pre class="LANG">` for a
    /// client-side renderer (mermaid.js), neither highlighted nor numbered.
    pub diagrams: Vec<String>,
    pub heading_anchors: HeadingAnchors,
    /// `:rocket:` and the rest of GitHub's shortcodes as emoji.
    pub emoji: bool,
    /// The page is right to left: code blocks and inline code get
    /// `dir="ltr"`, so the bidi algorithm does not reorder them.
    pub code_ltr: bool,
    /// The shortcodes the site has, when calls are to be taken out of the
    /// body; `None` leaves `{{ name() }}` as text (the `markdown` filter).
    pub shortcodes: Option<BTreeSet<String>>,
    /// The file line the body starts on, for line numbers in errors.
    pub first_line: usize,
}

/// Where a heading's `#` link to itself goes. Start and end, not left and
/// right: on a right-to-left page the start is the right.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HeadingAnchors {
    #[default]
    None,
    Start,
    End,
}

/// What a markdown link that leaves the site gets.
#[derive(Clone, Debug, Default)]
pub struct ExternalLinks {
    /// `target="_blank"`, with `rel="noopener"`.
    pub target_blank: bool,
    pub no_follow: bool,
    pub no_referrer: bool,
}

impl ExternalLinks {
    fn any(&self) -> bool {
        self.target_blank || self.no_follow || self.no_referrer
    }
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
    let mut css = css_for_theme_with_class_style(theme, ClassStyle::Spaced)?;
    let mark = theme
        .settings
        .line_highlight
        .map(|c| {
            format!(
                "rgba({}, {}, {}, {:.3})",
                c.r,
                c.g,
                c.b,
                f32::from(c.a) / 255.0
            )
        })
        .unwrap_or_else(|| "rgba(255, 220, 0, 0.25)".to_owned());
    css.push_str(&format!(
        "\n/* Code block lines, present only with linenos or hl_lines. */\n\
         pre code .line {{ display: block; }}\n\
         pre code .line.hl {{ background-color: {mark}; }}\n\
         pre code .line[data-line]::before {{ content: attr(data-line); display: inline-block; \
         min-width: 3ch; margin-inline-end: 1.5ch; text-align: end; opacity: 0.5; user-select: none; }}\n"
    ));
    Ok(css)
}

/// A fenced code block's info string: the language, then comma-separated
/// options. `linenos`, `linenostart=N`, `hl_lines=1 3-5` and `title=NAME`
/// (quotes optional) are read; any other option (mdBook's `ignore`) is left
/// alone.
#[derive(Debug, Default, PartialEq)]
struct Fence {
    lang: String,
    linenos: bool,
    linenostart: usize,
    hl_lines: BTreeSet<usize>,
    /// Shown above the block, usually a file name.
    title: Option<String>,
}

impl Fence {
    fn parse(info: &str) -> Result<Fence> {
        let info = info.trim();
        let (lang, rest) = match info.find([' ', '\t', ',']) {
            Some(at) => (&info[..at], &info[at + 1..]),
            None => (info, ""),
        };
        let mut fence = Fence {
            lang: lang.to_owned(),
            linenostart: 1,
            ..Fence::default()
        };
        for option in rest.split(',').map(str::trim) {
            if option == "linenos" {
                fence.linenos = true;
            } else if let Some(start) = option.strip_prefix("linenostart=") {
                fence.linenostart =
                    start
                        .trim()
                        .parse()
                        .ok()
                        .filter(|n| *n > 0)
                        .with_context(|| {
                            format!("code block {info:?}: linenostart wants a positive number")
                        })?;
            } else if let Some(title) = option.strip_prefix("title=") {
                let title = title.trim();
                let title = title
                    .strip_prefix('"')
                    .and_then(|t| t.strip_suffix('"'))
                    .unwrap_or(title);
                if !title.is_empty() {
                    fence.title = Some(title.to_owned());
                }
            } else if let Some(ranges) = option.strip_prefix("hl_lines=") {
                for range in ranges.split_whitespace() {
                    let (from, to) = range.split_once('-').unwrap_or((range, range));
                    let bounds = from.parse::<usize>().ok().zip(to.parse::<usize>().ok());
                    let Some((from, to)) = bounds.filter(|(f, t)| *f > 0 && f <= t) else {
                        bail!(
                            "code block {info:?}: hl_lines wants line numbers like `2` or `3-5`, got {range:?}"
                        );
                    };
                    fence.hl_lines.extend(from..=to);
                }
            }
        }
        Ok(fence)
    }

    /// Lines are wrapped one by one only when something needs them; a plain
    /// block renders as it always did.
    fn wraps_lines(&self) -> bool {
        self.linenos || !self.hl_lines.is_empty()
    }
}

/// `lines` (the HTML of each source line, its newline included) each in a
/// `<span class="line">`: `hl` on the lines `hl_lines` names, `data-line`
/// with the line's number when `linenos` is on.
fn wrap_lines(lines: &[String], fence: &Fence) -> Result<String> {
    if let Some(last) = fence.hl_lines.last()
        && *last > lines.len()
    {
        bail!(
            "code block {:?}: hl_lines names line {last}, but the block has {} lines",
            fence.lang,
            lines.len()
        );
    }
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        out.push_str("<span class=\"line");
        if fence.hl_lines.contains(&(i + 1)) {
            out.push_str(" hl");
        }
        out.push('"');
        if fence.linenos {
            out.push_str(&format!(" data-line=\"{}\"", fence.linenostart + i));
        }
        out.push('>');
        out.push_str(line);
        out.push_str("</span>");
    }
    Ok(out)
}

/// Highlighted HTML per source line, each line closing the spans it leaves
/// open and the next reopening them, so every line is well-formed on its own.
fn highlighted_lines(
    code: &str,
    syntax: &syntect::parsing::SyntaxReference,
) -> Result<Vec<String>> {
    let set = syntax_set();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut open: Vec<String> = Vec::new();
    let mut lines = Vec::new();
    for line in syntect::util::LinesWithEndings::from(code) {
        let ops = state.parse_line(line, set)?;
        let (html, _) = line_tokens_to_classed_spans(line, &ops, ClassStyle::Spaced, &mut stack)?;
        let mut out = open.concat();
        out.push_str(&html);
        // syntect writes only `<span class="...">` and `</span>`; text is
        // escaped, so every `<` starts one of the two.
        let mut rest = html.as_str();
        while let Some(at) = rest.find('<') {
            rest = &rest[at..];
            if let Some(after) = rest.strip_prefix("</span>") {
                open.pop();
                rest = after;
            } else {
                let end = rest.find('>').map_or(rest.len(), |e| e + 1);
                open.push(rest[..end].to_owned());
                rest = &rest[end..];
            }
        }
        out.push_str(&"</span>".repeat(open.len()));
        lines.push(out);
    }
    Ok(lines)
}

fn highlight(code: &str, fence: &Fence) -> Result<String> {
    let lang = fence.lang.as_str();
    let set = syntax_set();
    let syntax = if lang.is_empty() {
        None
    } else {
        set.find_syntax_by_token(lang)
    };
    let Some(syntax) = syntax else {
        return highlight_plain(code, fence);
    };
    if fence.wraps_lines() {
        return Ok(format!(
            "<pre class=\"highlight\"><code class=\"language-{}\">{}</code></pre>\n",
            crate::util::escape_html(lang),
            wrap_lines(&highlighted_lines(code, syntax)?, fence)?
        ));
    }
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
    let (markdown, shortcodes) = match &options.shortcodes {
        Some(known) => {
            crate::shortcode::extract(markdown, &|name| known.contains(name), options.first_line)?
        }
        None => (markdown.to_owned(), Vec::new()),
    };
    let markdown = markdown.as_str();
    let mut parser_options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_GFM;
    if options.smart_punctuation {
        parser_options |= Options::ENABLE_SMART_PUNCTUATION;
    }
    // Opt-in: with it on, two dollar amounts in one sentence are a formula.
    if options.math {
        parser_options |= Options::ENABLE_MATH;
    }
    let parser = Parser::new_ext(markdown, parser_options);

    let mut events: Vec<Event> = Vec::new();
    let mut links = Vec::new();
    let mut word_count = 0usize;
    let mut headings: Vec<(u8, String, String)> = Vec::new();
    let mut used_ids: BTreeMap<String, usize> = BTreeMap::new();
    let mut heading: Option<(u8, Option<String>, String, usize)> = None;
    let mut code: Option<(Fence, String)> = None;

    for event in parser {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                let fence = match kind {
                    CodeBlockKind::Fenced(info) => Fence::parse(&info)?,
                    CodeBlockKind::Indented => Fence::parse("")?,
                };
                code = Some((fence, String::new()));
            }
            Event::Text(text) if code.is_some() => {
                if let Some((_, buffer)) = code.as_mut() {
                    buffer.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let (fence, buffer) = code.take().expect("code block was opened");
                word_count += buffer.split_whitespace().count();
                let html = if options.diagrams.contains(&fence.lang) {
                    diagram(&buffer, &fence.lang)
                } else if options.highlight {
                    highlight(&buffer, &fence)?
                } else {
                    highlight_plain(&buffer, &fence)?
                };
                let html = frame_block(html, &fence, options.code_ltr);
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
                // Hidden from screen readers and the tab order: the heading
                // itself is what they announce, the `#` is for pointing.
                let anchor = || {
                    Event::InlineHtml(CowStr::from(format!(
                        "<a class=\"heading-anchor\" href=\"#{}\" aria-hidden=\"true\" tabindex=\"-1\">#</a>",
                        crate::util::escape_html(&id)
                    )))
                };
                match options.heading_anchors {
                    HeadingAnchors::None => {}
                    HeadingAnchors::Start => {
                        events.insert(start + 1, anchor());
                        events.insert(start + 2, Event::InlineHtml(CowStr::from(" ")));
                    }
                    HeadingAnchors::End => {
                        events.push(Event::InlineHtml(CowStr::from(" ")));
                        events.push(anchor());
                    }
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
                if options.external_links.any() && is_external(&dest, &options.base_url) {
                    // The writer still closes it: `TagEnd::Link` is `</a>`.
                    events.push(Event::InlineHtml(CowStr::from(external_link_open(
                        &dest,
                        &title,
                        &options.external_links,
                    ))));
                } else {
                    events.push(Event::Start(Tag::Link {
                        link_type,
                        dest_url: CowStr::from(dest),
                        title,
                        id,
                    }));
                }
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
                let text = match options.emoji.then(|| replace_emoji(&text)).flatten() {
                    Some(replaced) => CowStr::from(replaced),
                    None => text,
                };
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
                if options.code_ltr {
                    let mut html = String::from("<code dir=\"ltr\">");
                    pulldown_cmark_escape::escape_html_body_text(&mut html, &text)
                        .expect("writing to a String");
                    html.push_str("</code>");
                    events.push(Event::InlineHtml(CowStr::from(html)));
                } else {
                    events.push(Event::Code(text));
                }
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
        shortcodes,
    })
}

fn highlight_plain(code: &str, fence: &Fence) -> Result<String> {
    let lang = fence.lang.as_str();
    let class = if lang.is_empty() {
        String::new()
    } else {
        format!(" class=\"language-{}\"", crate::util::escape_html(lang))
    };
    let body = if fence.wraps_lines() {
        let lines: Vec<String> = syntect::util::LinesWithEndings::from(code)
            .map(crate::util::escape_html)
            .collect();
        wrap_lines(&lines, fence)?
    } else {
        crate::util::escape_html(code)
    };
    Ok(format!("<pre><code{class}>{body}</code></pre>\n"))
}

/// A diagram block: the source, escaped, for a renderer that reads the
/// element's text (mermaid.js decodes the entities back).
fn diagram(code: &str, lang: &str) -> String {
    format!(
        "<pre class=\"{}\">{}</pre>\n",
        crate::util::escape_html(lang),
        crate::util::escape_html(code)
    )
}

/// A rendered block (it starts with `<pre`) given its title and direction:
/// a title wraps it in a `<figure>` with a `<figcaption>`, and `ltr` marks
/// the outermost element `dir="ltr"`.
fn frame_block(html: String, fence: &Fence, ltr: bool) -> String {
    let dir = if ltr { " dir=\"ltr\"" } else { "" };
    match &fence.title {
        Some(title) => format!(
            "<figure class=\"code-block\"{dir}><figcaption>{}</figcaption>\n{html}</figure>\n",
            crate::util::escape_html(title)
        ),
        None if ltr => html.replacen("<pre", "<pre dir=\"ltr\"", 1),
        None => html,
    }
}

/// `text` with every known `:shortcode:` replaced by its emoji; unknown
/// ones, and colons that are not shortcodes (`10:30:00`), stay as written.
fn replace_emoji(text: &str) -> Option<String> {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r":([a-z0-9_+-]+):").expect("valid regex"));
    if !re.is_match(text) {
        return None;
    }
    let replaced = re.replace_all(text, |c: &regex::Captures| {
        match emojis::get_by_shortcode(&c[1]) {
            Some(emoji) => emoji.as_str().to_owned(),
            None => c[0].to_owned(),
        }
    });
    Some(replaced.into_owned())
}

/// A link that leaves the site: an absolute http(s) URL outside `base_url`.
fn is_external(dest: &str, base_url: &str) -> bool {
    let lower = dest.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return false;
    }
    let own = !base_url.is_empty()
        && dest
            .strip_prefix(base_url)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']));
    !own
}

/// The opening `<a>` of an external link, written by hand because the HTML
/// writer has no way to add attributes.
fn external_link_open(dest: &str, title: &str, links: &ExternalLinks) -> String {
    let mut out = String::from("<a href=\"");
    pulldown_cmark_escape::escape_href(&mut out, dest).expect("writing to a String");
    out.push('"');
    if !title.is_empty() {
        out.push_str(" title=\"");
        pulldown_cmark_escape::escape_html(&mut out, title).expect("writing to a String");
        out.push('"');
    }
    let mut rel = Vec::new();
    if links.target_blank {
        rel.push("noopener");
    }
    if links.no_follow {
        rel.push("nofollow");
    }
    if links.no_referrer {
        rel.push("noreferrer");
    }
    if !rel.is_empty() {
        out.push_str(&format!(" rel=\"{}\"", rel.join(" ")));
    }
    if links.target_blank {
        out.push_str(" target=\"_blank\"");
    }
    out.push('>');
    out
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
            math: false,
            external_links: ExternalLinks::default(),
            base_url: "https://x".to_owned(),
            diagrams: vec!["mermaid".to_owned()],
            heading_anchors: HeadingAnchors::None,
            emoji: false,
            code_ltr: false,
            shortcodes: None,
            first_line: 1,
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

    #[test]
    fn callouts_become_classed_blockquotes() {
        let out = render(
            "> [!NOTE]\n> Read this.\n\n> [!WARNING]\n> Careful.\n\n> plain quote\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert!(
            out.html
                .contains("<blockquote class=\"markdown-alert-note\">"),
            "{}",
            out.html
        );
        assert!(out.html.contains("<p>Read this.</p>"), "{}", out.html);
        assert!(
            out.html
                .contains("<blockquote class=\"markdown-alert-warning\">"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<blockquote>\n<p>plain quote</p>"),
            "{}",
            out.html
        );
    }

    #[test]
    fn definition_lists_render() {
        let out = render("Term\n: The definition.\n", &opts(), &no_links).unwrap();
        assert!(
            out.html
                .contains("<dl>\n<dt>Term</dt>\n<dd>The definition.</dd>\n</dl>"),
            "{}",
            out.html
        );
    }

    #[test]
    fn math_is_opt_in() {
        let text = "It cost $5 and then $x^2$ more.\n\n$$\\sum_i i$$\n";
        let plain = render(text, &opts(), &no_links).unwrap();
        assert!(!plain.html.contains("class=\"math"), "{}", plain.html);
        assert!(plain.html.contains("$x^2$"), "{}", plain.html);
        let math = RenderOptions {
            math: true,
            ..opts()
        };
        let out = render(text, &math, &no_links).unwrap();
        assert!(
            out.html
                .contains("<span class=\"math math-inline\">x^2</span>"),
            "{}",
            out.html
        );
        assert!(
            out.html
                .contains("<span class=\"math math-display\">\\sum_i i</span>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("cost $5 and"),
            "a lone dollar stays text:\n{}",
            out.html
        );
    }

    #[test]
    fn external_links_get_rel_and_target_when_asked() {
        let text = "[out](https://e.org/a?b=1&c=2 \"T <x>\") [own](https://x/en/) [sub](https://xy.org/) <https://auto.org/> [rel](other/) [mail](mailto:a@b) [in](@/a.md)";
        let resolve = |path: &str| (path == "a.md").then(|| "https://x/en/a/".to_owned());
        let untouched = render(text, &opts(), &resolve).unwrap();
        assert!(
            !untouched.html.contains("rel="),
            "off by default:\n{}",
            untouched.html
        );
        let all = RenderOptions {
            external_links: ExternalLinks {
                target_blank: true,
                no_follow: true,
                no_referrer: true,
            },
            ..opts()
        };
        let out = render(text, &all, &resolve).unwrap();
        assert!(
            out.html.contains("<a href=\"https://e.org/a?b=1&amp;c=2\" title=\"T &lt;x&gt;\" rel=\"noopener nofollow noreferrer\" target=\"_blank\">out</a>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<a href=\"https://xy.org/\" rel="),
            "a host that only starts like base_url is external:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<a href=\"https://auto.org/\" rel="),
            "autolinks too:\n{}",
            out.html
        );
        assert!(
            out.html.contains("<a href=\"https://x/en/\">own</a>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<a href=\"other/\">rel</a>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<a href=\"mailto:a@b\">mail</a>"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains("<a href=\"https://x/en/a/\">in</a>"),
            "a resolved content link is the site's own:\n{}",
            out.html
        );
        let nofollow = RenderOptions {
            external_links: ExternalLinks {
                no_follow: true,
                ..ExternalLinks::default()
            },
            ..opts()
        };
        let out = render("[o](http://e.org/)", &nofollow, &no_links).unwrap();
        assert_eq!(
            out.html,
            "<p><a href=\"http://e.org/\" rel=\"nofollow\">o</a></p>\n"
        );
    }

    #[test]
    fn fence_options_parse() {
        let fence = Fence::parse("rust,linenos,hl_lines=1 3-5,linenostart=10").unwrap();
        assert_eq!(fence.lang, "rust");
        assert!(fence.linenos);
        assert_eq!(fence.linenostart, 10);
        assert_eq!(fence.hl_lines, BTreeSet::from([1, 3, 4, 5]));
        let plain = Fence::parse("rust,ignore").unwrap();
        assert!(!plain.wraps_lines(), "unknown options are left alone");
        assert_eq!(Fence::parse("").unwrap().lang, "");
        for bad in [
            "rust,hl_lines=x",
            "rust,hl_lines=5-3",
            "rust,hl_lines=0",
            "rust,linenostart=0",
        ] {
            assert!(Fence::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn plain_code_lines_number_and_highlight() {
        let out = render(
            "```text,linenos,hl_lines=2\na\nb < c\n```\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert_eq!(
            out.html,
            "<pre><code class=\"language-text\"><span class=\"line\" data-line=\"1\">a\n</span><span class=\"line hl\" data-line=\"2\">b &lt; c\n</span></code></pre>\n"
        );
        let out = render("```\nx\ny\n```\n", &opts(), &no_links).unwrap();
        assert_eq!(
            out.html, "<pre><code>x\ny\n</code></pre>\n",
            "no options, no wrapping"
        );
        let error = render("```text,hl_lines=3\na\nb\n```\n", &opts(), &no_links).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("hl_lines names line 3, but the block has 2 lines"),
            "{error}"
        );
    }

    #[test]
    fn highlighted_lines_are_each_well_formed() {
        let highlighted = RenderOptions {
            highlight: true,
            ..opts()
        };
        let out = render(
            "```rust,hl_lines=2\nlet a = 1; /* one\ntwo */ let b = \"<s>\";\n```\n",
            &highlighted,
            &no_links,
        )
        .unwrap();
        let code = out
            .html
            .strip_prefix("<pre class=\"highlight\"><code class=\"language-rust\">")
            .and_then(|h| h.strip_suffix("</code></pre>\n"))
            .unwrap_or_else(|| panic!("{}", out.html));
        let lines: Vec<&str> = code.split("<span class=\"line").skip(1).collect();
        assert_eq!(lines.len(), 2, "{code}");
        assert!(lines[1].starts_with(" hl\">"), "{code}");
        for line in &lines {
            let opens = line.matches("<span").count() + 1;
            assert_eq!(opens, line.matches("</span>").count(), "balanced: {line}");
        }
        assert!(
            lines[1].contains("comment"),
            "the comment's span is reopened on its second line:\n{code}"
        );
        assert!(code.contains("&lt;s&gt;"), "{code}");
    }

    #[test]
    fn highlight_css_styles_the_lines() {
        let css = highlight_css("InspiredGitHub").unwrap();
        assert!(css.contains("pre code .line { display: block; }"), "{css}");
        assert!(
            css.contains("pre code .line.hl { background-color:"),
            "{css}"
        );
        assert!(css.contains("content: attr(data-line)"), "{css}");
    }

    #[test]
    fn diagram_blocks_are_left_for_the_browser() {
        let highlighted = RenderOptions {
            highlight: true,
            ..opts()
        };
        let out = render(
            "```mermaid,linenos\ngraph LR\n  A --> B\n```\n",
            &highlighted,
            &no_links,
        )
        .unwrap();
        assert_eq!(
            out.html, "<pre class=\"mermaid\">graph LR\n  A --&gt; B\n</pre>\n",
            "escaped, not highlighted, not numbered"
        );
        let none = RenderOptions {
            diagrams: Vec::new(),
            ..opts()
        };
        let out = render("```mermaid\ngraph LR\n```\n", &none, &no_links).unwrap();
        assert!(
            out.html.contains("<code class=\"language-mermaid\">"),
            "{}",
            out.html
        );
    }

    #[test]
    fn heading_anchors_go_at_the_start_or_the_end() {
        let text = "## Two words\n\n## Two words\n";
        let none = render(text, &opts(), &no_links).unwrap();
        assert!(!none.html.contains("heading-anchor"), "off by default");
        let end = RenderOptions {
            heading_anchors: HeadingAnchors::End,
            ..opts()
        };
        let out = render(text, &end, &no_links).unwrap();
        assert!(
            out.html.contains("<h2 id=\"two-words-1\">Two words <a class=\"heading-anchor\" href=\"#two-words-1\" aria-hidden=\"true\" tabindex=\"-1\">#</a></h2>"),
            "{}",
            out.html
        );
        assert_eq!(out.toc[0].title, "Two words", "the # is not in the toc");
        let start = RenderOptions {
            heading_anchors: HeadingAnchors::Start,
            ..opts()
        };
        let out = render("# T\n", &start, &no_links).unwrap();
        assert_eq!(
            out.html,
            "<h1 id=\"t\"><a class=\"heading-anchor\" href=\"#t\" aria-hidden=\"true\" tabindex=\"-1\">#</a> T</h1>\n"
        );
    }

    #[test]
    fn a_titled_code_block_is_a_figure() {
        let out = render(
            "```rust,title=\"src/<main>.rs\"\nfn main() {}\n```\n",
            &opts(),
            &no_links,
        )
        .unwrap();
        assert_eq!(
            out.html,
            "<figure class=\"code-block\"><figcaption>src/&lt;main&gt;.rs</figcaption>\n<pre><code class=\"language-rust\">fn main() {}\n</code></pre>\n</figure>\n"
        );
        assert_eq!(
            Fence::parse("sh,title=run.sh").unwrap().title.as_deref(),
            Some("run.sh")
        );
        assert_eq!(Fence::parse("sh,title=").unwrap().title, None);
    }

    #[test]
    fn code_stays_left_to_right_on_right_to_left_pages() {
        let rtl = RenderOptions {
            code_ltr: true,
            ..opts()
        };
        let out = render(
            "שלום `a < b` עולם\n\n```c\nx;\n```\n\n```c,title=a.c\ny;\n```\n",
            &rtl,
            &no_links,
        )
        .unwrap();
        assert!(
            out.html
                .contains("שלום <code dir=\"ltr\">a &lt; b</code> עולם"),
            "{}",
            out.html
        );
        assert!(
            out.html
                .contains("<pre dir=\"ltr\"><code class=\"language-c\">x;"),
            "{}",
            out.html
        );
        assert!(
            out.html.contains(
                "<figure class=\"code-block\" dir=\"ltr\"><figcaption>a.c</figcaption>\n<pre><code"
            ),
            "a titled block is marked once, on the figure:\n{}",
            out.html
        );
        let ltr = render("`a`\n", &opts(), &no_links).unwrap();
        assert_eq!(ltr.html, "<p><code>a</code></p>\n");
    }

    #[test]
    fn emoji_shortcodes_are_opt_in() {
        let text = "Ship it :rocket: at 10:30:00, :not_an_emoji: stays. `:rocket:` in code stays.\n\n```\n:rocket:\n```\n";
        let plain = render(text, &opts(), &no_links).unwrap();
        assert!(!plain.html.contains('🚀'), "{}", plain.html);
        let emoji = RenderOptions {
            emoji: true,
            ..opts()
        };
        let out = render(text, &emoji, &no_links).unwrap();
        assert!(
            out.html
                .contains("Ship it 🚀 at 10:30:00, :not_an_emoji: stays."),
            "{}",
            out.html
        );
        assert!(out.html.contains("<code>:rocket:</code>"), "{}", out.html);
        assert!(
            out.html.contains("<pre><code>:rocket:\n</code></pre>"),
            "{}",
            out.html
        );
    }

    #[test]
    fn fenced_code_info_string_trims_leading_space_and_splits_tabs() {
        let out = render("``` rust\tlinenos\nx\n```\n", &opts(), &no_links).unwrap();
        assert!(out.html.contains("class=\"language-rust\""), "{}", out.html);
    }
}
