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

impl ContentAsset {
    pub fn rel(&self) -> String {
        if self.components.is_empty() {
            self.name.clone()
        } else {
            format!("{}/{}", self.components.join("/"), self.name)
        }
    }
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
    /// Write a listing of the section's dated pages per year, at
    /// `<section>/<year>/`, rendered with `archive_template`.
    #[serde(default)]
    pub year_archives: bool,
    #[serde(default)]
    pub archive_template: Option<String>,
    #[serde(default)]
    pub smart_punctuation: Option<bool>,
    /// A redirect per pager: `"/page/{n}/"` writes `/page/2/` sending the
    /// reader to this section's second pager, for every pager it has.
    #[serde(default)]
    pub pager_aliases: Option<String>,
    /// Every page of the section must exist in every language the section
    /// exists in; a missing translation is an error naming the page.
    #[serde(default)]
    pub require_translations: bool,
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
    let Some(rest) = text
        .strip_prefix("+++\r\n")
        .or_else(|| text.strip_prefix("+++\n"))
    else {
        if required {
            bail!("no front matter: the file must start with a +++ line");
        }
        return Ok((toml::Table::new(), text.to_owned()));
    };
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

#[derive(Debug)]
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

#[derive(Debug)]
pub struct ParsedSectionFront {
    pub front: SectionFront,
    pub updated: Option<DateValue>,
}

pub fn parse_section_front(mut table: toml::Table) -> Result<ParsedSectionFront> {
    if table.contains_key("date") {
        bail!("front matter: a section (_index) has no date; use `updated` for the sitemap");
    }
    let updated = take_date(&mut table, "updated")?;
    let mut front: SectionFront = toml::Value::Table(table)
        .try_into()
        .context("front matter")?;
    front.paginate_path = front.paginate_path.trim_matches('/').to_owned();
    if front.paginate_path.is_empty() || front.paginate_path.split('/').any(|p| p == "..") {
        bail!(
            "front matter: paginate_path {:?} must be a relative, non-empty path",
            front.paginate_path
        );
    }
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

    #[test]
    fn a_language_suffix_needs_a_name_before_it() {
        let error = split_language(".en.md", &langs()).unwrap_err().to_string();
        assert!(error.contains("no name before"), "{error}");
    }

    #[test]
    fn a_suffix_that_is_not_a_language_code_is_part_of_the_name() {
        // Four letters, digits, uppercase: not a language code, so with one
        // language the whole stem is the name.
        let one = vec!["en".to_owned()];
        assert_eq!(
            split_language("notes.draft.md", &one).unwrap().0,
            "notes.draft"
        );
        assert_eq!(split_language("a.EN.md", &one).unwrap().0, "a.EN");
        assert_eq!(split_language("a.v2.md", &one).unwrap().0, "a.v2");
        assert!(looks_like_language("fr"));
        assert!(looks_like_language("deu"));
        assert!(!looks_like_language("f"));
        assert!(!looks_like_language("abcd"));
        assert!(!looks_like_language("e1"));
        assert!(!looks_like_language("EN"));
    }

    #[test]
    fn with_several_languages_a_lookalike_suffix_is_named_in_the_error() {
        let error = split_language("post.de.md", &langs())
            .unwrap_err()
            .to_string();
        assert!(error.contains("\"de\""), "{error}");
        assert!(error.contains("en, he"), "{error}");
        let error = split_language("post.md", &langs()).unwrap_err().to_string();
        assert!(error.contains("no language suffix"), "{error}");
    }

    #[test]
    fn temp_and_hidden_files_are_skipped() {
        for name in [
            ".post.en.md",
            "#post.en.md#",
            "post.en.md~",
            ".post.en.md.swp",
        ] {
            assert!(is_temp_file(name), "{name}");
        }
        for name in ["post.en.md", "_index.en.md", "a#b.en.md", "x.swp.en.md"] {
            assert!(!is_temp_file(name), "{name}");
        }
    }

    #[test]
    fn front_matter_accepts_crlf_and_a_byte_order_mark() {
        let (front, body) =
            split_front_matter("\u{feff}+++\r\ntitle = \"x\"\r\n+++\r\nbody\r\n", true).unwrap();
        assert_eq!(front["title"].as_str(), Some("x"));
        assert_eq!(body, "body\r\n");
        let (front, body) = split_front_matter("+++\n+++\nonly body", true).unwrap();
        assert!(front.is_empty());
        assert_eq!(body, "only body");
        let (_, body) = split_front_matter("+++\ntitle = \"x\"\n+++", true).unwrap();
        assert_eq!(body, "", "a closing line without a newline still closes");
    }

    #[test]
    fn front_matter_errors_say_what_is_wrong() {
        let unclosed = split_front_matter("+++\ntitle = \"x\"\nbody\n", true).unwrap_err();
        assert!(unclosed.to_string().contains("not closed"));
        let missing = split_front_matter("body\n", true).unwrap_err();
        assert!(missing.to_string().contains("no front matter"));
        let invalid = format!(
            "{:#}",
            split_front_matter("+++\ntitle = \n+++\n", true).unwrap_err()
        );
        assert!(invalid.contains("not valid TOML"), "{invalid}");
        // A +++ inside the body is not a delimiter.
        let (_, body) = split_front_matter("+++\na = 1\n+++\ntext\n+++\nmore\n", true).unwrap();
        assert_eq!(body, "text\n+++\nmore\n");
    }

    #[test]
    fn optional_front_matter_for_html_and_tera_bodies() {
        let (front, body) =
            split_front_matter("+++\ntemplate = \"x.html\"\n+++\n<p>a</p>", false).unwrap();
        assert_eq!(front["template"].as_str(), Some("x.html"));
        assert_eq!(body, "<p>a</p>");
        let (front, body) = split_front_matter("", false).unwrap();
        assert!(front.is_empty() && body.is_empty());
    }

    #[test]
    fn page_front_matter_fields_parse() {
        let table: toml::Table = toml::from_str(
            "title = \"T\"\ndescription = \"D\"\nslug = \"s\"\ntemplate = \"t.html\"\ndraft = true\nweight = 3\naliases = [\"/a/\"]\ntranslation_key = \"k\"\nbody_from = \"he\"\nsmart_punctuation = true\n[taxonomies]\ntags = [\"a\", \"b\"]\n[extra]\nx = 1\n",
        )
        .unwrap();
        let parsed = parse_page_front(table).unwrap();
        let front = parsed.front;
        assert_eq!(front.title.as_deref(), Some("T"));
        assert_eq!(front.description.as_deref(), Some("D"));
        assert_eq!(front.slug.as_deref(), Some("s"));
        assert_eq!(front.template.as_deref(), Some("t.html"));
        assert!(front.draft);
        assert_eq!(front.weight, Some(3));
        assert_eq!(front.aliases, vec!["/a/".to_owned()]);
        assert_eq!(front.translation_key.as_deref(), Some("k"));
        assert_eq!(front.body_from.as_deref(), Some("he"));
        assert_eq!(front.smart_punctuation, Some(true));
        assert_eq!(
            front.taxonomies["tags"],
            vec!["a".to_owned(), "b".to_owned()]
        );
        assert_eq!(front.extra["x"].as_integer(), Some(1));
        assert!(parsed.date.is_none() && parsed.updated.is_none());
        let empty = parse_page_front(toml::Table::new()).unwrap();
        assert!(empty.front.title.is_none() && !empty.front.draft);
    }

    #[test]
    fn a_date_that_is_not_a_date_is_an_error() {
        let table: toml::Table = toml::from_str("title = \"t\"\ndate = 2026").unwrap();
        let error = format!("{:#}", parse_page_front(table).unwrap_err());
        assert!(error.contains("must be a date, found a integer"), "{error}");
        let table: toml::Table = toml::from_str("title = \"t\"\ndate = \"yesterday\"").unwrap();
        let error = format!("{:#}", parse_page_front(table).unwrap_err());
        assert!(error.contains("front matter date"), "{error}");
        assert!(error.contains("is not a date"), "{error}");
        let table: toml::Table = toml::from_str("title = \"t\"\nupdated = true").unwrap();
        assert!(parse_page_front(table).is_err());
    }

    #[test]
    fn section_front_matter_parses_and_rejects_a_date() {
        let table: toml::Table = toml::from_str(
            "title = \"Blog\"\nsort_by = \"weight\"\npaginate_by = 5\npaginate_path = \"p\"\ngenerate_feeds = false\nyear_archives = true\narchive_template = \"y.html\"\npage_template = \"post.html\"\nupdated = 2026-02-02\n",
        )
        .unwrap();
        let parsed = parse_section_front(table).unwrap();
        assert_eq!(parsed.front.sort_by, SortBy::Weight);
        assert_eq!(parsed.front.paginate_by, Some(5));
        assert_eq!(parsed.front.paginate_path, "p");
        assert_eq!(parsed.front.generate_feeds, Some(false));
        assert!(parsed.front.year_archives);
        assert_eq!(parsed.front.archive_template.as_deref(), Some("y.html"));
        assert_eq!(parsed.front.page_template.as_deref(), Some("post.html"));
        assert_eq!(parsed.updated.unwrap().ymd(), "2026-02-02");
        let defaults = parse_section_front(toml::Table::new()).unwrap();
        assert_eq!(defaults.front.sort_by, SortBy::None);
        assert_eq!(defaults.front.paginate_path, "page");
        assert!(defaults.front.generate_feeds.is_none());
        let dated: toml::Table = toml::from_str("date = 2026-01-01").unwrap();
        let error = parse_section_front(dated).unwrap_err().to_string();
        assert!(error.contains("has no date"), "{error}");
        let unknown: toml::Table = toml::from_str("sort_by = \"random\"").unwrap();
        assert!(parse_section_front(unknown).is_err());
        let page_key: toml::Table = toml::from_str("slug = \"x\"").unwrap();
        assert!(parse_section_front(page_key).is_err(), "slug is a page key");
    }

    fn scan_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rssite-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(dir: &Path, rel: &str, text: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn scanning_sorts_files_classifies_them_and_keeps_assets() {
        let dir = scan_dir("classify");
        put(&dir, "blog/_index.en.md", "+++\ntitle = \"B\"\n+++\n");
        put(&dir, "blog/post.en.md", "+++\ntitle = \"P\"\n+++\nbody\n");
        put(&dir, "blog/post.he.md", "+++\ntitle = \"פ\"\n+++\n");
        put(&dir, "blog/image.png", "not really a png");
        put(&dir, "blog/.post.en.md.swp", "junk");
        put(&dir, "apps/_index.en.html", "<p>no front matter</p>");
        put(
            &dir,
            "about.en.tera",
            "+++\ntitle = \"A\"\n+++\n{{ page.title }}",
        );
        let (files, assets) = scan(&dir, &langs()).unwrap();
        let rels: Vec<&str> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "about.en.tera",
                "apps/_index.en.html",
                "blog/_index.en.md",
                "blog/post.en.md",
                "blog/post.he.md"
            ]
        );
        let about = &files[0];
        assert_eq!(about.body_kind, BodyKind::Tera);
        assert_eq!(about.kind, FileKind::Page);
        assert!(about.components.is_empty());
        assert_eq!(about.stem, "about");
        assert_eq!(about.lang, "en");
        assert_eq!(about.body, "{{ page.title }}");
        let apps = &files[1];
        assert_eq!(apps.body_kind, BodyKind::Html);
        assert_eq!(apps.kind, FileKind::Index);
        assert!(apps.front.is_empty());
        assert_eq!(apps.body, "<p>no front matter</p>");
        let index = &files[2];
        assert_eq!(index.kind, FileKind::Index);
        assert_eq!(index.components, vec!["blog".to_owned()]);
        let post = &files[3];
        assert_eq!(post.kind, FileKind::Page);
        assert_eq!(post.body_kind, BodyKind::Markdown);
        assert_eq!(post.front["title"].as_str(), Some("P"));
        assert_eq!(post.body, "body\n");
        assert_eq!(files[4].lang, "he");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "image.png");
        assert_eq!(assets[0].components, vec!["blog".to_owned()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn scanning_names_the_file_in_its_errors() {
        let dir = scan_dir("errors");
        put(&dir, "blog/post.en.md", "no front matter\n");
        let error = format!("{:#}", scan(&dir, &langs()).unwrap_err());
        assert!(error.contains("blog/post.en.md"), "{error}");
        assert!(error.contains("front matter"), "{error}");
        std::fs::remove_file(dir.join("blog/post.en.md")).unwrap();
        put(&dir, "blog/post.md", "+++\n+++\n");
        let error = format!("{:#}", scan(&dir, &langs()).unwrap_err());
        assert!(error.contains("blog/post.md"), "{error}");
        assert!(error.contains("language suffix"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_content_directory_is_an_empty_site() {
        let dir = scan_dir("missing");
        let (files, assets) = scan(&dir.join("content"), &langs()).unwrap();
        assert!(files.is_empty() && assets.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn opening_front_matter_delimiter_requires_its_own_line_and_paginate_path_is_validated() {
        assert!(split_front_matter("+++title = \"x\"\n+++\nbody", true).is_err());
        let ok: toml::Table = toml::from_str("paginate_path = \"/p/\"").unwrap();
        assert_eq!(parse_section_front(ok).unwrap().front.paginate_path, "p");
        for bad in ["paginate_path = \"\"", "paginate_path = \"../p\""] {
            let table: toml::Table = toml::from_str(bad).unwrap();
            assert!(parse_section_front(table).is_err(), "{bad}");
        }
    }
}
