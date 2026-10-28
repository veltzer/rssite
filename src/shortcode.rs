//! Shortcodes: small templates called from markdown, in zola's syntax.
//!
//! `{{ name(key=value, ...) }}` calls `templates/shortcodes/name.html`;
//! `{% name(key=value) %}...{% end %}` does the same with the text between as
//! `body`. Arguments are TOML values: strings, numbers, booleans, arrays.
//!
//! Markdown is rendered while the site loads, before the template engines
//! exist, so a call is cut out of the text and replaced by a marker, an HTML
//! comment: it vanishes from every text a page's HTML is reduced to (meta
//! descriptions, the search index), and alone on its line it is an HTML block
//! rather than a paragraph. The calls are rendered later, with the engine,
//! and each output replaces its marker.
//!
//! Calls inside code blocks and inline code are text, so a page can show the
//! syntax.

use anyhow::{Result, bail};
use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::Range;

/// Where shortcode templates live, under the templates directory.
pub const DIR: &str = "shortcodes/";

/// A shortcode call found in a body.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Call {
    pub name: String,
    /// The arguments, as the template sees them.
    pub args: serde_json::Map<String, serde_json::Value>,
    /// The text between `{% name() %}` and `{% end %}`; `None` for `{{ }}`.
    pub body: Option<String>,
    /// 1 for the first call of this shortcode in the body, 2 for the next.
    pub nth: usize,
}

impl Call {
    /// The template this call renders.
    pub fn template(&self) -> String {
        template_of(&self.name)
    }
}

pub fn template_of(name: &str) -> String {
    format!("{DIR}{name}.html")
}

/// The marker standing in for call `index` until it is rendered.
pub fn marker(index: usize) -> String {
    format!("<!--rssite-shortcode-{index}-->")
}

/// `markdown` with every shortcode call replaced by its marker, and the
/// calls in order. `known` says which names have a template; calling any
/// other is an error, as are malformed arguments and a missing `{% end %}`.
/// `first_line` is the file line `markdown` starts on, for those errors.
pub fn extract(
    markdown: &str,
    known: &dyn Fn(&str) -> bool,
    first_line: usize,
) -> Result<(String, Vec<Call>)> {
    if !markdown.contains("{{") && !markdown.contains("{%") {
        return Ok((markdown.to_owned(), Vec::new()));
    }
    let code = code_ranges(markdown);
    let in_code = |at: usize| code.iter().any(|r| r.contains(&at));
    let mut out = String::with_capacity(markdown.len());
    let mut calls: Vec<Call> = Vec::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut i = 0;
    while let Some(found) = markdown[i..].find('{') {
        let at = i + found;
        let parsed = if in_code(at) {
            None
        } else {
            parse_call(markdown, at, first_line)?
        };
        let Some(parsed) = parsed else {
            out.push_str(&markdown[i..=at]);
            i = at + 1;
            continue;
        };
        let line = markdown[..at].matches('\n').count() + first_line;
        if !known(&parsed.name) {
            bail!(
                "line {line}: no shortcode {:?}; add templates/{}",
                parsed.name,
                template_of(&parsed.name)
            );
        }
        let args = parse_args(&parsed.args)
            .map_err(|e| anyhow::anyhow!("line {line}: shortcode {}: {e}", parsed.name))?;
        let nth = counts.entry(parsed.name.clone()).or_insert(0);
        *nth += 1;
        out.push_str(&markdown[i..at]);
        out.push_str(&marker(calls.len()));
        calls.push(Call {
            name: parsed.name,
            args,
            body: parsed.body,
            nth: *nth,
        });
        i = parsed.end;
    }
    out.push_str(&markdown[i..]);
    Ok((out, calls))
}

/// Byte ranges of code blocks and inline code spans: text, not calls.
fn code_ranges(markdown: &str) -> Vec<Range<usize>> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_DEFINITION_LIST
        | Options::ENABLE_GFM;
    Parser::new_ext(markdown, options)
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => Some(range),
            _ => None,
        })
        .collect()
}

struct Parsed {
    name: String,
    args: String,
    body: Option<String>,
    /// Byte offset just past the call (past `{% end %}` for a block).
    end: usize,
}

/// The call starting at `at`, if one does; `None` for any other `{`, so
/// prose with braces stays prose. A block call without its `{% end %}` is an
/// error.
fn parse_call(text: &str, at: usize, first_line: usize) -> Result<Option<Parsed>> {
    let rest = &text[at..];
    let (block, close) = if rest.starts_with("{{") {
        (false, "}}")
    } else if rest.starts_with("{%") {
        (true, "%}")
    } else {
        return Ok(None);
    };
    let mut p = at + 2;
    p = skip_ws(text, p);
    let name_start = p;
    for (offset, c) in text[p..].char_indices() {
        let ok = if offset == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_' || c == '-'
        };
        if !ok {
            break;
        }
        p = name_start + offset + c.len_utf8();
    }
    if p == name_start {
        return Ok(None);
    }
    let name = text[name_start..p].to_owned();
    if block && name == "end" {
        return Ok(None);
    }
    p = skip_ws(text, p);
    if !text[p..].starts_with('(') {
        return Ok(None);
    }
    let Some(args_end) = closing_paren(text, p + 1) else {
        return Ok(None);
    };
    let args = text[p + 1..args_end].to_owned();
    p = skip_ws(text, args_end + 1);
    if !text[p..].starts_with(close) {
        return Ok(None);
    }
    p += close.len();
    if !block {
        return Ok(Some(Parsed {
            name,
            args,
            body: None,
            end: p,
        }));
    }
    let Some((body_end, end)) = find_end(text, p) else {
        let line = text[..at].matches('\n').count() + first_line;
        bail!("line {line}: shortcode {name:?} has no {{% end %}}");
    };
    let body = text[p..body_end]
        .strip_prefix('\n')
        .unwrap_or(&text[p..body_end]);
    let body = body.strip_suffix('\n').unwrap_or(body);
    Ok(Some(Parsed {
        name,
        args,
        body: Some(body.to_owned()),
        end,
    }))
}

fn skip_ws(text: &str, mut p: usize) -> usize {
    while let Some(c) = text[p..].chars().next()
        && c.is_whitespace()
    {
        p += c.len_utf8();
    }
    p
}

/// The index of the `)` closing an argument list that starts at `from`,
/// skipping quoted strings and nested brackets.
fn closing_paren(text: &str, from: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (offset, c) in text[from..].char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' && q == '"' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' => depth = depth.checked_sub(1)?,
            ')' if depth == 0 => return Some(from + offset),
            ')' => depth -= 1,
            '\n' if text[from..from + offset].ends_with('\n') => return None,
            _ => {}
        }
    }
    None
}

/// The first `{% end %}` at or after `from`: where the body stops and where
/// the call does.
fn find_end(text: &str, from: usize) -> Option<(usize, usize)> {
    let mut search = from;
    while let Some(found) = text[search..].find("{%") {
        let at = search + found;
        let p = skip_ws(text, at + 2);
        if text[p..].starts_with("end") {
            let q = skip_ws(text, p + 3);
            if text[q..].starts_with("%}") {
                return Some((at, q + 2));
            }
        }
        search = at + 2;
    }
    None
}

/// `key=value, ...` as a map, each value a TOML value.
fn parse_args(args: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    if args.trim().is_empty() {
        return Ok(serde_json::Map::new());
    }
    let table: toml::Table = toml::from_str(&format!("args = {{ {args} }}")).map_err(|e| {
        anyhow::anyhow!(
            "arguments {args:?} are not key=value pairs of TOML values: {}",
            e.message()
        )
    })?;
    let value = serde_json::to_value(&table["args"])?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => unreachable!("an inline table is an object"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(name: &str) -> bool {
        ["youtube", "figure", "note"].contains(&name)
    }

    #[test]
    fn inline_calls_become_markers_with_their_arguments() {
        let (out, calls) = extract(
            "See {{ youtube(id=\"abc\", start=30) }} and {{youtube()}}.\n",
            &known,
            1,
        )
        .unwrap();
        assert_eq!(out, format!("See {} and {}.\n", marker(0), marker(1)));
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "youtube");
        assert_eq!(calls[0].args["id"], "abc");
        assert_eq!(calls[0].args["start"], 30);
        assert_eq!(calls[0].nth, 1);
        assert_eq!(calls[1].nth, 2);
        assert!(calls[1].args.is_empty());
        assert!(calls[0].body.is_none());
    }

    #[test]
    fn a_block_call_takes_the_text_up_to_end_as_its_body() {
        let (out, calls) = extract(
            "Before\n\n{% note(kind=\"tip\") %}\nSome *markdown*.\n{% end %}\n\nAfter\n",
            &known,
            1,
        )
        .unwrap();
        assert_eq!(out, format!("Before\n\n{}\n\nAfter\n", marker(0)));
        assert_eq!(calls[0].body.as_deref(), Some("Some *markdown*."));
        assert_eq!(calls[0].args["kind"], "tip");
        let error = extract("{% note() %}\nno end\n", &known, 1).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("line 1: shortcode \"note\" has no {% end %}"),
            "{error}"
        );
    }

    #[test]
    fn calls_in_code_are_text() {
        let text =
            "`{{ youtube(id=\"a\") }}`\n\n```\n{{ figure(src=\"x\") }}\n```\n\n    {{ note() }}\n";
        let (out, calls) = extract(text, &known, 1).unwrap();
        assert_eq!(out, text);
        assert!(calls.is_empty());
    }

    #[test]
    fn braces_that_are_not_calls_stay_prose() {
        let text =
            "a {b} {{ page.title }} {% if x %} {{ youtube }} {{ figure(src=\"a\" }} {{ 3() }}\n";
        let (out, calls) = extract(text, &known, 1).unwrap();
        assert_eq!(out, text);
        assert!(calls.is_empty());
    }

    #[test]
    fn an_unknown_shortcode_or_bad_arguments_is_an_error() {
        let error = extract("x\n{{ nope(a=1) }}\n", &known, 1).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("line 2: no shortcode \"nope\"; add templates/shortcodes/nope.html"),
            "{error}"
        );
        let error = extract("{{ figure(src=unquoted) }}\n", &known, 1).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("line 1: shortcode figure: arguments"),
            "{error}"
        );
    }

    #[test]
    fn arguments_take_any_toml_value_and_parentheses_in_strings() {
        let (_, calls) = extract(
            "{{ figure(src='a (1).png', alt=\"say \\\"hi)\\\"\", widths=[1, 2], wide=true, ratio=1.5) }}",
            &known,
            1,
        )
        .unwrap();
        let args = &calls[0].args;
        assert_eq!(args["src"], "a (1).png");
        assert_eq!(args["alt"], "say \"hi)\"");
        assert_eq!(args["widths"], serde_json::json!([1, 2]));
        assert_eq!(args["wide"], true);
        assert_eq!(args["ratio"], 1.5);
    }

    #[test]
    fn a_body_without_braces_skips_parsing() {
        let (out, calls) = extract("plain text\n", &|_| false, 1).unwrap();
        assert_eq!(out, "plain text\n");
        assert!(calls.is_empty());
    }
}
