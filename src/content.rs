//! The content tree: which files are pages, sections and assets, what
//! language each is in, and what its front matter says. Every content file
//! names its language in its file name (`post.en.md`); a file that does not
//! is an error, never a page in some implicit language.

use crate::i18n::DateValue;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// `_index.<lang>.<ext>`: a section.
    Index,
    /// Any other content file: a page.
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BodyKind {
    Markdown,
    /// The body is HTML, used as is.
    Html,
    /// The body is a Tera template rendered with the page's context.
    Tera,
}

#[derive(Clone, Debug)]
pub struct ContentFile {
    /// Absolute path.
    pub path: PathBuf,
    /// Relative to the content directory, slash separated.
    pub rel: String,
    /// Directory components below the content root.
    pub components: Vec<String>,
    /// File name without language suffix and extension.
    pub stem: String,
    pub lang: String,
    pub kind: FileKind,
    pub body_kind: BodyKind,
    pub front: toml::Table,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct ContentAsset {
    pub path: PathBuf,
    pub components: Vec<String>,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    Date,
    Weight,
    Title,
    #[default]
    None,
}

/// Page front matter. `date` and `updated` are pulled out before
/// deserialization because TOML dates and date strings are both accepted.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageFront {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub weight: Option<i64>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub taxonomies: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub extra: toml::Table,
    /// Pairs this page with its translations under a key other than the
    /// file's base name, so a translation can carry its own file name.
    #[serde(default)]
    pub translation_key: Option<String>,
    /// Render the body of this page's translation in that language instead
    /// of this file's (empty) body.
    #[serde(default)]
    pub body_from: Option<String>,
    #[serde(default)]
    pub smart_punctuation: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SectionFront {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub page_template: Option<String>,
    #[serde(default)]
    pub sort_by: SortBy,
    #[serde(default)]
    pub paginate_by: Option<usize>,
    #[serde(default = "default_paginate_path")]
    pub paginate_path: String,
    /// Defaults to the language's `generate_feeds` for the root section and
    /// to false for any other section.
    #[serde(default)]
    pub generate_feeds: Option<bool>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub weight: Option<i64>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub extra: toml::Table,
    #[serde(default)]
    pub translation_key: Option<String>,
    #[serde(default)]
    pub body_from: Option<String>,
    #[serde(default)]
    pub smart_punctuation: Option<bool>,
}

fn default_paginate_path() -> String {
    "page".into()
}

/// Scan the content directory. Returns content files and non-content assets.
pub fn scan(
    content_dir: &Path,
    languages: &[String],
) -> Result<(Vec<ContentFile>, Vec<ContentAsset>)> {
    let mut files = Vec::new();
    let mut assets = Vec::new();
    for path in crate::util::walk_files(content_dir)? {
        let rel = crate::util::rel(&path, content_dir);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_temp_file(&name) {
            continue;
        }
        let mut components: Vec<String> = rel.split('/').map(str::to_owned).collect();
        components.pop();
        let body_kind = match path.extension().and_then(|e| e.to_str()) {
            Some("md") => BodyKind::Markdown,
            Some("html") => BodyKind::Html,
            Some("tera") => BodyKind::Tera,
            _ => {
                assets.push(ContentAsset {
                    path,
                    components,
                    name,
                });
                continue;
            }
        };
        let (stem, lang) =
            split_language(&name, languages).with_context(|| format!("content file {rel}"))?;
        let kind = if stem == "_index" {
            FileKind::Index
        } else {
            FileKind::Page
        };
        let text = crate::util::read(&path)?;
        let required = body_kind == BodyKind::Markdown;
        let (front, body) =
            split_front_matter(&text, required).with_context(|| format!("content file {rel}"))?;
        files.push(ContentFile {
            path,
            rel,
            components,
            stem,
            lang,
            kind,
            body_kind,
            front,
            body,
        });
    }
    Ok((files, assets))
}

fn is_temp_file(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('#') || name.ends_with('~') || name.ends_with(".swp")
}

/// `post.en.md` -> ("post", "en"). With one configured language the suffix
/// may be omitted; with several it is required.
fn split_language(name: &str, languages: &[String]) -> Result<(String, String)> {
    let without_ext = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
    if let Some((stem, suffix)) = without_ext.rsplit_once('.') {
        if languages.iter().any(|l| l == suffix) {
            if stem.is_empty() {
                bail!("file name {name:?} has no name before its language suffix");
            }
            return Ok((stem.to_owned(), suffix.to_owned()));
        }
        if looks_like_language(suffix) {
            bail!(
                "file name {name:?} carries the language suffix {suffix:?}, which is not a configured language ({})",
                languages.join(", ")
            );
        }
    }
    if languages.len() == 1 {
        return Ok((without_ext.to_owned(), languages[0].clone()));
    }
    bail!(
        "file name {name:?} has no language suffix; with several languages configured every content file is named <name>.<lang>.<ext>, lang one of {}",
        languages.join(", ")
    )
}

fn looks_like_language(suffix: &str) -> bool {
    (2..=3).contains(&suffix.len()) && suffix.chars().all(|c| c.is_ascii_lowercase())
}

/// Split `+++` TOML front matter from the body. When `required` is false a
/// file without front matter is an empty table and the whole text.
pub fn split_front_matter(text: &str, required: bool) -> Result<(toml::Table, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text.strip_prefix("+++") else {
        if required {
            bail!("no front matter: the file must start with a +++ line");
        }
        return Ok((toml::Table::new(), text.to_owned()));
    };
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))
        .unwrap_or(rest);
    let mut front_end = None;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "+++" {
            front_end = Some((offset, offset + line.len()));
            break;
        }
        offset += line.len();
    }
    let Some((end, body_start)) = front_end else {
        bail!("front matter is not closed: no +++ line after the opening one");
    };
    let front: toml::Table =
        toml::from_str(&rest[..end]).context("front matter is not valid TOML")?;
    Ok((front, rest[body_start..].to_owned()))
}

/// Take a date-valued key out of a front matter table.
fn take_date(table: &mut toml::Table, key: &str) -> Result<Option<DateValue>> {
    match table.remove(key) {
        None => Ok(None),
        Some(toml::Value::Datetime(d)) => Ok(Some(
            DateValue::parse(&d.to_string()).with_context(|| format!("front matter {key}"))?,
        )),
        Some(toml::Value::String(s)) => Ok(Some(
            DateValue::parse(&s).with_context(|| format!("front matter {key}"))?,
        )),
        Some(other) => bail!(
            "front matter {key} must be a date, found a {}",
            other.type_str()
        ),
    }
}

pub struct ParsedPageFront {
    pub front: PageFront,
    pub date: Option<DateValue>,
    pub updated: Option<DateValue>,
}

pub fn parse_page_front(mut table: toml::Table) -> Result<ParsedPageFront> {
    let date = take_date(&mut table, "date")?;
    let updated = take_date(&mut table, "updated")?;
    let front: PageFront = toml::Value::Table(table)
        .try_into()
        .context("front matter")?;
    Ok(ParsedPageFront {
        front,
        date,
        updated,
    })
}

pub struct ParsedSectionFront {
    pub front: SectionFront,
    pub updated: Option<DateValue>,
}

pub fn parse_section_front(mut table: toml::Table) -> Result<ParsedSectionFront> {
    if table.contains_key("date") {
        bail!("front matter: a section (_index) has no date; use `updated` for the sitemap");
    }
    let updated = take_date(&mut table, "updated")?;
    let front: SectionFront = toml::Value::Table(table)
        .try_into()
        .context("front matter")?;
    Ok(ParsedSectionFront { front, updated })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs() -> Vec<String> {
        vec!["en".into(), "he".into()]
    }

    #[test]
    fn language_suffix_required_with_several_languages() {
        assert_eq!(
            split_language("post.en.md", &langs()).unwrap(),
            ("post".into(), "en".into())
        );
        assert_eq!(
            split_language("_index.he.md", &langs()).unwrap(),
            ("_index".into(), "he".into())
        );
        assert!(split_language("post.md", &langs()).is_err());
        assert!(split_language("post.fr.md", &langs()).is_err());
    }

    #[test]
    fn single_language_may_omit_suffix() {
        let one = vec!["en".to_owned()];
        assert_eq!(
            split_language("post.md", &one).unwrap(),
            ("post".into(), "en".into())
        );
        assert_eq!(
            split_language("post.en.md", &one).unwrap(),
            ("post".into(), "en".into())
        );
        assert!(split_language("post.fr.md", &one).is_err());
        assert_eq!(
            split_language("v1.2.md", &one).unwrap(),
            ("v1.2".into(), "en".into())
        );
    }

    #[test]
    fn front_matter_splits() {
        let (front, body) = split_front_matter("+++\ntitle = \"x\"\n+++\nbody\n", true).unwrap();
        assert_eq!(front["title"].as_str(), Some("x"));
        assert_eq!(body, "body\n");
        assert!(split_front_matter("no front\n", true).is_err());
        let (front, body) = split_front_matter("<p>raw</p>", false).unwrap();
        assert!(front.is_empty());
        assert_eq!(body, "<p>raw</p>");
        assert!(split_front_matter("+++\ntitle = 1\n", true).is_err());
    }

    #[test]
    fn dates_in_both_forms() {
        let table: toml::Table = toml::from_str(
            "title = \"t\"\ndate = 2026-04-28\nupdated = \"2026-05-01T10:00:00+02:00\"",
        )
        .unwrap();
        let parsed = parse_page_front(table).unwrap();
        assert_eq!(parsed.date.unwrap().ymd(), "2026-04-28");
        assert_eq!(
            parsed.updated.unwrap().rfc3339(),
            "2026-05-01T10:00:00+02:00"
        );
    }

    #[test]
    fn unknown_front_matter_key_is_an_error() {
        let table: toml::Table = toml::from_str("title = \"t\"\nlang = \"he\"").unwrap();
        assert!(parse_page_front(table).is_err());
    }
}
