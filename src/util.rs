//! Small helpers shared by every module: file reading with context, sorted
//! directory walks, slash-separated relative paths, and change-aware writes.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Read a file to a string, naming the file in the error.
pub fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Every regular file below `dir`, sorted by path, following symlinks. A
/// missing directory is empty rather than an error: a site without a
/// `static/` tree is a valid site.
pub fn walk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(true)
        .sort_by_file_name()
    {
        let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
        if entry.file_type().is_file() {
            files.push(entry.into_path());
        }
    }
    files.sort();
    Ok(files)
}

/// `path` relative to `base`, with `/` separators whatever the platform.
pub fn rel(path: &Path, base: &Path) -> String {
    let relative = path.strip_prefix(base).unwrap_or(path);
    unix(relative)
}

/// A path as a `/`-separated string.
pub fn unix(path: &Path) -> String {
    let mut out = String::new();
    for component in path.components() {
        match component {
            std::path::Component::RootDir => out.push('/'),
            other => {
                if !out.is_empty() && !out.ends_with('/') {
                    out.push('/');
                }
                out.push_str(&other.as_os_str().to_string_lossy());
            }
        }
    }
    out
}

/// Write `bytes` to `path` unless the file already holds exactly them.
/// Returns whether anything was written. Leaving an unchanged file alone
/// keeps its mtime, which is what lets a downstream build system skip the
/// work that depends on it.
pub fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool> {
    if let Ok(existing) = std::fs::read(path)
        && existing == bytes
    {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// A TOML value as JSON, for template contexts. TOML datetimes become their
/// RFC 3339 text; serde would otherwise emit them as a private tagged map.
pub fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::from(*i),
        toml::Value::Float(f) => serde_json::Value::from(*f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(a) => serde_json::Value::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => serde_json::Value::Object(
            t.iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect(),
        ),
    }
}

/// Escape the five XML/HTML special characters.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// The text of an HTML fragment: tags dropped (with what `<script>` and
/// `<style>` hold), the common entities decoded, whitespace collapsed.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        let tag = &rest[at..];
        // An inline element joins the text around it; anything else (a
        // paragraph, a heading, a line break) separates words.
        let name: String = tag[1..]
            .trim_start_matches('/')
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        const INLINE: &[&str] = &[
            "a", "abbr", "b", "cite", "code", "del", "em", "i", "ins", "kbd", "mark", "q", "s",
            "small", "span", "strong", "sub", "sup", "time", "u",
        ];
        if !INLINE.contains(&name.as_str()) {
            out.push(' ');
        }
        let lower: String = tag.chars().take(8).collect::<String>().to_ascii_lowercase();
        let skip_to = if lower.starts_with("<script") {
            Some("</script>")
        } else if lower.starts_with("<style") {
            Some("</style>")
        } else if tag.starts_with("<!--") {
            Some("-->")
        } else {
            None
        };
        rest = match skip_to {
            Some(end) => match tag.to_ascii_lowercase().find(end) {
                Some(e) => &tag[e + end.len()..],
                None => "",
            },
            None => match tag.find('>') {
                Some(e) => &tag[e + 1..],
                None => "",
            },
        };
    }
    out.push_str(rest);
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters of `text`, cut at a word boundary with an
/// ellipsis when anything was cut.
pub fn truncate_words(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(at) if at > 0 => &cut[..at],
        _ => cut.as_str(),
    };
    format!(
        "{}…",
        cut.trim_end_matches(|c: char| c.is_whitespace() || ",;:.-".contains(c))
    )
}

/// Whether `rel`, a `/`-separated path, matches the glob `pattern`: `*`
/// matches within one path segment, `**` any number of segments, `?` one
/// character. `fonts/*` matches `fonts/a.woff2` but not `fonts/x/a.woff2`;
/// `**/*.css` matches a `.css` file at any depth.
pub fn glob_match(pattern: &str, rel: &str) -> Result<bool> {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    re.push_str("(?:.*/)?");
                } else {
                    re.push_str(".*");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            other => re.push_str(&regex::escape(&other.to_string())),
        }
    }
    re.push('$');
    let re = regex::Regex::new(&re).with_context(|| format!("glob {pattern:?}"))?;
    Ok(re.is_match(rel))
}

/// Print `text` and a newline on stdout. A reader that closed the pipe
/// (`rssite plan | head`) is not an error: the output ends there, quietly,
/// where `println!` would panic.
pub fn emit(text: &str) -> Result<()> {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let written = out
        .write_all(text.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    match written {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        other => other.context("writing to stdout"),
    }
}

#[cfg(test)]
mod text_tests {
    use super::{html_to_text, truncate_words};

    #[test]
    fn html_becomes_its_text() {
        assert_eq!(html_to_text("<p>a <b>b</b></p>\n<p>c&amp;d</p>"), "a b c&d");
        assert_eq!(
            html_to_text("<style>p{x:1}</style>x<script>y()</script>z"),
            "x z"
        );
        assert_eq!(html_to_text("a<!-- more -->b"), "a b");
        assert_eq!(html_to_text("&lt;tag&gt; &quot;q&quot;"), "<tag> \"q\"");
        assert_eq!(html_to_text("unclosed <br"), "unclosed");
        assert_eq!(html_to_text("see <a href=\"x\">this</a>."), "see this.");
        assert_eq!(html_to_text("a<br>b"), "a b");
    }

    #[test]
    fn truncation_cuts_at_a_word() {
        assert_eq!(truncate_words("short", 10), "short");
        assert_eq!(truncate_words("one two three", 9), "one two…");
        assert_eq!(truncate_words("one, two three", 6), "one…");
        assert_eq!(truncate_words("abcdefghij", 4), "abcd…");
        assert_eq!(truncate_words("שלום עולם גדול", 10), "שלום עולם…");
    }
}

#[cfg(test)]
mod glob_tests {
    use super::glob_match;

    #[test]
    fn globs_match_segments_as_documented() {
        assert!(glob_match("fonts/*", "fonts/a.woff2").unwrap());
        assert!(!glob_match("fonts/*", "fonts/x/a.woff2").unwrap());
        assert!(glob_match("**/*.css", "a.css").unwrap());
        assert!(glob_match("**/*.css", "x/y/a.css").unwrap());
        assert!(!glob_match("*.css", "x/a.css").unwrap());
        assert!(glob_match("**/*", "x/y/z").unwrap());
        assert!(glob_match("a?c", "abc").unwrap());
        assert!(!glob_match("a?c", "a/c").unwrap());
        assert!(glob_match("themes.css", "themes.css").unwrap());
        assert!(!glob_match("themes.css", "themes-css").unwrap());
    }

    #[test]
    fn a_pattern_matches_the_whole_path() {
        assert!(!glob_match("a", "ab").unwrap());
        assert!(!glob_match("b", "ab").unwrap());
        assert!(!glob_match("*.css", "a.css.map").unwrap());
        assert!(!glob_match("fonts/*", "fonts").unwrap());
        assert!(!glob_match("fonts/*", "xfonts/a").unwrap());
    }

    #[test]
    fn regex_metacharacters_are_literal() {
        assert!(glob_match("a.b", "a.b").unwrap());
        assert!(!glob_match("a.b", "axb").unwrap());
        assert!(glob_match("c++/(x).js", "c++/(x).js").unwrap());
        assert!(!glob_match("c++/(x).js", "c/x.js").unwrap());
        assert!(glob_match("[a]", "[a]").unwrap());
        assert!(!glob_match("[a]", "a").unwrap());
        assert!(glob_match("$x^", "$x^").unwrap());
    }

    #[test]
    fn double_star_in_the_middle_and_at_the_end() {
        assert!(glob_match("a/**/z", "a/z").unwrap());
        assert!(glob_match("a/**/z", "a/b/z").unwrap());
        assert!(glob_match("a/**/z", "a/b/c/z").unwrap());
        assert!(!glob_match("a/**/z", "a/b/zz").unwrap());
        assert!(glob_match("a/**", "a/").unwrap());
        assert!(glob_match("a/**", "a/b/c").unwrap());
        assert!(glob_match("**", "anything/at/all").unwrap());
        assert!(glob_match("x**y", "xabc/defy").unwrap());
    }

    #[test]
    fn star_and_question_mark_never_cross_a_slash() {
        assert!(glob_match("*", "file").unwrap());
        assert!(!glob_match("*", "dir/file").unwrap());
        assert!(glob_match("*/*", "dir/file").unwrap());
        assert!(!glob_match("*/*", "a/b/c").unwrap());
        assert!(glob_match("?", "a").unwrap());
        assert!(!glob_match("?", "/").unwrap());
        assert!(glob_match("a*", "a").unwrap(), "star matches nothing too");
        assert!(glob_match("ש*.md", "שלום.md").unwrap(), "unicode is fine");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rssite-util-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn html_escaping_covers_the_five_characters_and_nothing_else() {
        assert_eq!(
            escape_html("<a href=\"x\">Tom & Jerry's</a>"),
            "&lt;a href=&quot;x&quot;&gt;Tom &amp; Jerry&#39;s&lt;/a&gt;"
        );
        assert_eq!(
            escape_html("/en/blog/"),
            "/en/blog/",
            "slashes are not escaped"
        );
        assert_eq!(escape_html("שלום – «ok»"), "שלום – «ok»");
        assert_eq!(escape_html(""), "");
        assert_eq!(
            escape_html("&amp;"),
            "&amp;amp;",
            "no double-escape detection"
        );
    }

    #[test]
    fn toml_values_become_their_json_counterparts() {
        let table: toml::Table = toml::from_str(
            "s = \"x\"\ni = 3\nf = 1.5\nb = true\nd = 2026-04-28\ndt = 2026-04-28T10:00:00+02:00\na = [1, \"two\"]\n[t]\nk = \"v\"\n",
        )
        .unwrap();
        let json = toml_to_json(&toml::Value::Table(table));
        assert_eq!(json["s"], "x");
        assert_eq!(json["i"], 3);
        assert_eq!(json["f"], 1.5);
        assert_eq!(json["b"], true);
        assert_eq!(json["d"], "2026-04-28");
        assert_eq!(json["dt"], "2026-04-28T10:00:00+02:00");
        assert_eq!(json["a"][0], 1);
        assert_eq!(json["a"][1], "two");
        assert_eq!(json["t"]["k"], "v");
        assert!(json["missing"].is_null());
    }

    #[test]
    fn toml_key_order_is_kept() {
        let table: toml::Table = toml::from_str("z = 1\na = 2\nm = 3\n").unwrap();
        let json = toml_to_json(&toml::Value::Table(table));
        let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
        // toml::Table is a BTreeMap: sorted, and the JSON keeps that order.
        assert_eq!(keys, ["a", "m", "z"]);
    }

    #[test]
    fn paths_are_slash_separated_and_relative_to_a_base() {
        let base = Path::new("/srv/site");
        assert_eq!(
            rel(Path::new("/srv/site/content/a.md"), base),
            "content/a.md"
        );
        assert_eq!(rel(Path::new("/srv/site"), base), "");
        assert_eq!(
            rel(Path::new("/elsewhere/x"), base),
            "/elsewhere/x",
            "a path outside the base is returned whole"
        );
        assert_eq!(unix(Path::new("a/b/c.txt")), "a/b/c.txt");
        assert_eq!(unix(Path::new("/a/b")), "/a/b");
        assert_eq!(unix(Path::new("")), "");
    }

    #[test]
    fn write_if_changed_writes_once_and_creates_parents() {
        let dir = temp("write");
        let path = dir.join("deep/er/file.txt");
        assert!(write_if_changed(&path, b"one").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        assert!(
            !write_if_changed(&path, b"one").unwrap(),
            "same bytes: untouched"
        );
        assert!(write_if_changed(&path, b"two").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(
            write_if_changed(&path, b"").unwrap(),
            "emptying is a change"
        );
        assert!(!write_if_changed(&path, b"").unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn write_if_changed_keeps_the_mtime_of_an_unchanged_file() {
        let dir = temp("mtime");
        let path = dir.join("f");
        write_if_changed(&path, b"x").unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_if_changed(&path, b"x").unwrap();
        let after = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(before, after);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn walk_files_is_sorted_and_tolerates_a_missing_directory() {
        let dir = temp("walk");
        assert!(walk_files(&dir.join("nope")).unwrap().is_empty());
        for name in ["b.txt", "a/z.txt", "a/b/c.txt", "a.txt"] {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, name).unwrap();
        }
        std::fs::create_dir_all(dir.join("empty")).unwrap();
        let found: Vec<String> = walk_files(&dir)
            .unwrap()
            .iter()
            .map(|p| rel(p, &dir))
            .collect();
        // PathBuf order: component by component, so `a/` sorts before `a.txt`.
        assert_eq!(found, ["a/b/c.txt", "a/z.txt", "a.txt", "b.txt"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn read_names_the_missing_file() {
        let error = format!(
            "{:#}",
            read(Path::new("/nonexistent/rssite/file")).unwrap_err()
        );
        assert!(error.contains("/nonexistent/rssite/file"), "{error}");
    }
}
