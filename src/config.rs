//! `rssite.toml`. Every key is known; an unknown one is an error rather
//! than a silently ignored line, and every directory the generator reads or
//! writes is configurable.

use crate::slug::SlugMode;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const CONFIG_FILE: &str = "rssite.toml";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The site's public root, without a trailing slash.
    pub base_url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_content_dir")]
    pub content_dir: String,
    #[serde(default = "default_templates_dir")]
    pub templates_dir: String,
    #[serde(default = "default_static_dir")]
    pub static_dir: String,
    #[serde(default = "default_sass_dir")]
    pub sass_dir: String,
    #[serde(default = "default_i18n_dir")]
    pub i18n_dir: String,
    /// A theme: `<themes_dir>/<theme>/` holds `templates`, `sass`, `static`
    /// and `i18n` directories that sit behind the site's own. The site's
    /// file of the same name wins, file by file; the manifest names the
    /// file that was used, so an override is visible rather than silent.
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default = "default_themes_dir")]
    pub themes_dir: String,
    /// Directories published into the output as they are: a submodule's
    /// stylesheets and fonts, say. Each file is its own output with itself
    /// as its only source.
    #[serde(default)]
    pub publish: Vec<Publish>,
    /// The language the root redirect and the global `404.html` use. Defaults
    /// to the first configured language. It is a fallback for hosts that
    /// cannot negotiate, not a "default language": every language is
    /// written, addressed and rendered the same way.
    #[serde(default)]
    pub fallback_language: Option<String>,
    #[serde(default)]
    pub languages: Vec<Language>,
    #[serde(default)]
    pub taxonomies: Vec<Taxonomy>,
    #[serde(default)]
    pub slugify: Slugify,
    #[serde(default)]
    pub markdown: Markdown,
    #[serde(default)]
    pub feeds: Feeds,
    #[serde(default)]
    pub sitemap: Sitemap,
    /// Free-form values exposed to templates as `config.extra`.
    #[serde(default)]
    pub extra: toml::Table,
}

/// One `[[publish]]` entry: the files of `from` matching `files` are written
/// under `to` in the output.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Publish {
    /// A directory relative to the site root.
    pub from: String,
    /// A directory relative to the output root.
    pub to: String,
    /// Globs relative to `from` (`*`, `**` and `?`); everything by default.
    #[serde(default = "default_publish_files")]
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub direction: Direction,
    /// Site title in this language; defaults to the top-level title.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub generate_feeds: bool,
    /// URL prefix, `/{code}/` by default. Must start and end with `/`.
    #[serde(default)]
    pub url_prefix: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Ltr => "ltr",
            Direction::Rtl => "rtl",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Taxonomy {
    pub name: String,
    #[serde(default)]
    pub paginate_by: Option<usize>,
    #[serde(default = "default_paginate_path")]
    pub paginate_path: String,
    /// Emit a feed per term.
    #[serde(default)]
    pub feed: bool,
    /// A TOML file of `[[terms]]` tables mapping language code to term name,
    /// pairing terms across languages. Relative to the site root.
    #[serde(default)]
    pub translations: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Slugify {
    #[serde(default)]
    pub paths: SlugMode,
    #[serde(default)]
    pub taxonomies: SlugMode,
    #[serde(default)]
    pub anchors: SlugMode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Markdown {
    #[serde(default)]
    pub smart_punctuation: bool,
    #[serde(default = "default_true")]
    pub highlight: bool,
    /// A syntect theme name; its CSS is written to `highlight.css`.
    #[serde(default = "default_highlight_theme")]
    pub highlight_theme: String,
}

impl Default for Markdown {
    fn default() -> Self {
        Markdown {
            smart_punctuation: false,
            highlight: true,
            highlight_theme: default_highlight_theme(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Feeds {
    /// Newest entries per feed. A feed is a window, not an archive.
    #[serde(default = "default_feed_limit")]
    pub limit: usize,
    #[serde(default = "default_feed_filename")]
    pub filename: String,
}

impl Default for Feeds {
    fn default() -> Self {
        Feeds {
            limit: default_feed_limit(),
            filename: default_feed_filename(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sitemap {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for Sitemap {
    fn default() -> Self {
        Sitemap { enabled: true }
    }
}

fn default_output_dir() -> String {
    "_site".into()
}
fn default_content_dir() -> String {
    "content".into()
}
fn default_templates_dir() -> String {
    "templates".into()
}
fn default_static_dir() -> String {
    "static".into()
}
fn default_sass_dir() -> String {
    "sass".into()
}
fn default_i18n_dir() -> String {
    "i18n".into()
}
fn default_themes_dir() -> String {
    "themes".into()
}
fn default_publish_files() -> Vec<String> {
    vec!["**/*".into()]
}
fn default_paginate_path() -> String {
    "page".into()
}
fn default_highlight_theme() -> String {
    "InspiredGitHub".into()
}
fn default_feed_limit() -> usize {
    20
}
fn default_feed_filename() -> String {
    "atom.xml".into()
}
fn default_true() -> bool {
    true
}

impl Config {
    /// Read and validate `<root>/rssite.toml`.
    pub fn load(root: &Path) -> Result<Config> {
        let path = root.join(CONFIG_FILE);
        let text = crate::util::read(&path)?;
        let mut config: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&mut self) -> Result<()> {
        while self.base_url.ends_with('/') {
            self.base_url.pop();
        }
        if self.base_url.is_empty() {
            bail!("{CONFIG_FILE}: base_url must be set");
        }
        if self.languages.is_empty() {
            bail!("{CONFIG_FILE}: at least one [[languages]] entry is required");
        }
        let mut seen = std::collections::BTreeSet::new();
        for language in &mut self.languages {
            if language.code.is_empty()
                || !language
                    .code
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
            {
                bail!(
                    "{CONFIG_FILE}: language code {:?} must be alphanumeric",
                    language.code
                );
            }
            if !seen.insert(language.code.clone()) {
                bail!(
                    "{CONFIG_FILE}: language {:?} is listed twice",
                    language.code
                );
            }
            let prefix = language
                .url_prefix
                .clone()
                .unwrap_or_else(|| format!("/{}/", language.code));
            if !prefix.starts_with('/') || !prefix.ends_with('/') {
                bail!(
                    "{CONFIG_FILE}: url_prefix {prefix:?} of language {:?} must start and end with '/'",
                    language.code
                );
            }
            language.url_prefix = Some(prefix);
        }
        match &self.fallback_language {
            None => self.fallback_language = Some(self.languages[0].code.clone()),
            Some(code) if !seen.contains(code) => {
                bail!("{CONFIG_FILE}: fallback_language {code:?} is not a configured language")
            }
            Some(_) => {}
        }
        let mut names = std::collections::BTreeSet::new();
        for taxonomy in &self.taxonomies {
            if taxonomy.name.is_empty() {
                bail!("{CONFIG_FILE}: a taxonomy needs a name");
            }
            if !names.insert(taxonomy.name.clone()) {
                bail!(
                    "{CONFIG_FILE}: taxonomy {:?} is listed twice",
                    taxonomy.name
                );
            }
        }
        for dir in [
            &self.content_dir,
            &self.templates_dir,
            &self.static_dir,
            &self.sass_dir,
            &self.i18n_dir,
            &self.output_dir,
            &self.themes_dir,
        ] {
            if dir.is_empty() || Path::new(dir).is_absolute() {
                bail!("{CONFIG_FILE}: directory {dir:?} must be a relative, non-empty path");
            }
        }
        if let Some(theme) = &self.theme
            && (theme.is_empty() || theme.contains('/') || theme.contains('\\') || theme == "..")
        {
            bail!(
                "{CONFIG_FILE}: theme {theme:?} must be a directory name under {}",
                self.themes_dir
            );
        }
        for entry in &mut self.publish {
            for (what, value) in [("from", &entry.from), ("to", &entry.to)] {
                if value.is_empty()
                    || Path::new(value).is_absolute()
                    || value.split('/').any(|part| part == "..")
                {
                    bail!(
                        "{CONFIG_FILE}: [[publish]] {what} = {value:?} must be a relative path inside the site"
                    );
                }
            }
            while entry.to.ends_with('/') {
                entry.to.pop();
            }
            while entry.from.ends_with('/') {
                entry.from.pop();
            }
            if entry.files.is_empty() {
                bail!(
                    "{CONFIG_FILE}: [[publish]] from = {:?} lists no files",
                    entry.from
                );
            }
        }
        Ok(())
    }

    pub fn language(&self, code: &str) -> Option<&Language> {
        self.languages.iter().find(|l| l.code == code)
    }

    pub fn language_codes(&self) -> Vec<String> {
        self.languages.iter().map(|l| l.code.clone()).collect()
    }

    pub fn fallback(&self) -> &str {
        self.fallback_language
            .as_deref()
            .unwrap_or(&self.languages[0].code)
    }
}

impl Language {
    pub fn prefix(&self) -> &str {
        self.url_prefix.as_deref().unwrap_or("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse and validate a config text the way `Config::load` would.
    fn parse(text: &str) -> Result<Config> {
        let mut config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    const MINIMAL: &str =
        "base_url = \"https://example.org\"\n[[languages]]\ncode = \"en\"\nname = \"English\"\n";

    fn error_of(text: &str) -> String {
        format!("{:#}", parse(text).expect_err("config should be rejected"))
    }

    #[test]
    fn minimal_config_fills_every_default() {
        let config = parse(MINIMAL).unwrap();
        assert_eq!(config.output_dir, "_site");
        assert_eq!(config.content_dir, "content");
        assert_eq!(config.templates_dir, "templates");
        assert_eq!(config.static_dir, "static");
        assert_eq!(config.sass_dir, "sass");
        assert_eq!(config.i18n_dir, "i18n");
        assert_eq!(config.themes_dir, "themes");
        assert!(config.theme.is_none());
        assert!(config.publish.is_empty());
        assert!(config.taxonomies.is_empty());
        assert_eq!(config.title, "");
        assert_eq!(config.feeds.limit, 20);
        assert_eq!(config.feeds.filename, "atom.xml");
        assert!(config.sitemap.enabled);
        assert!(config.markdown.highlight);
        assert!(!config.markdown.smart_punctuation);
        assert_eq!(config.markdown.highlight_theme, "InspiredGitHub");
        assert_eq!(config.slugify.paths, SlugMode::Safe);
        assert_eq!(config.slugify.taxonomies, SlugMode::Safe);
        assert_eq!(config.slugify.anchors, SlugMode::Safe);
        assert!(config.extra.is_empty());
    }

    #[test]
    fn the_first_language_is_the_fallback_and_gets_its_prefix() {
        let config = parse(MINIMAL).unwrap();
        assert_eq!(config.fallback(), "en");
        assert_eq!(config.fallback_language.as_deref(), Some("en"));
        let en = config.language("en").unwrap();
        assert_eq!(en.prefix(), "/en/");
        assert_eq!(en.direction, Direction::Ltr);
        assert!(en.generate_feeds);
        assert!(en.title.is_none());
        assert!(config.language("fr").is_none());
        assert_eq!(config.language_codes(), vec!["en".to_owned()]);
    }

    #[test]
    fn trailing_slashes_are_stripped_from_the_base_url() {
        let config = parse(&MINIMAL.replace("example.org", "example.org///")).unwrap();
        assert_eq!(config.base_url, "https://example.org");
    }

    #[test]
    fn an_empty_base_url_is_an_error() {
        assert!(error_of(&MINIMAL.replace("https://example.org", "")).contains("base_url"));
        assert!(error_of(&MINIMAL.replace("https://example.org", "/")).contains("base_url"));
        let missing = "[[languages]]\ncode = \"en\"\nname = \"English\"\n";
        assert!(parse(missing).is_err(), "base_url is required");
    }

    #[test]
    fn at_least_one_language_is_required() {
        assert!(error_of("base_url = \"https://example.org\"\n").contains("[[languages]]"));
    }

    #[test]
    fn language_codes_are_checked() {
        let bad = "base_url = \"https://x\"\n[[languages]]\ncode = \"e n\"\nname = \"x\"\n";
        assert!(error_of(bad).contains("must be alphanumeric"));
        let empty = "base_url = \"https://x\"\n[[languages]]\ncode = \"\"\nname = \"x\"\n";
        assert!(error_of(empty).contains("must be alphanumeric"));
        let dashed = "base_url = \"https://x\"\n[[languages]]\ncode = \"pt-br\"\nname = \"x\"\n";
        assert_eq!(
            parse(dashed).unwrap().language("pt-br").unwrap().prefix(),
            "/pt-br/"
        );
        let twice = format!("{MINIMAL}[[languages]]\ncode = \"en\"\nname = \"Again\"\n");
        assert!(error_of(&twice).contains("listed twice"));
    }

    #[test]
    fn url_prefix_must_be_bracketed_by_slashes() {
        for prefix in ["en/", "/en", "en"] {
            let text = format!("{MINIMAL}url_prefix = \"{prefix}\"\n");
            assert!(
                error_of(&text).contains("must start and end with '/'"),
                "{prefix}"
            );
        }
        let custom = format!("{MINIMAL}url_prefix = \"/english/\"\n");
        assert_eq!(
            parse(&custom).unwrap().language("en").unwrap().prefix(),
            "/english/"
        );
        let root = format!("{MINIMAL}url_prefix = \"/\"\n");
        assert_eq!(parse(&root).unwrap().language("en").unwrap().prefix(), "/");
    }

    #[test]
    fn the_fallback_language_must_be_configured() {
        let text = format!("fallback_language = \"he\"\n{MINIMAL}");
        assert!(error_of(&text).contains("fallback_language \"he\""));
        let text = format!(
            "fallback_language = \"he\"\n{MINIMAL}[[languages]]\ncode = \"he\"\nname = \"Hebrew\"\ndirection = \"rtl\"\n"
        );
        let config = parse(&text).unwrap();
        assert_eq!(config.fallback(), "he");
        assert_eq!(config.language("he").unwrap().direction, Direction::Rtl);
        assert_eq!(
            config.language_codes(),
            vec!["en".to_owned(), "he".to_owned()]
        );
    }

    #[test]
    fn taxonomies_need_distinct_names() {
        let twice =
            format!("{MINIMAL}[[taxonomies]]\nname = \"tags\"\n[[taxonomies]]\nname = \"tags\"\n");
        assert!(error_of(&twice).contains("listed twice"));
        let unnamed = format!("{MINIMAL}[[taxonomies]]\nname = \"\"\n");
        assert!(error_of(&unnamed).contains("needs a name"));
        let one =
            format!("{MINIMAL}[[taxonomies]]\nname = \"tags\"\npaginate_by = 5\nfeed = true\n");
        let config = parse(&one).unwrap();
        assert_eq!(config.taxonomies[0].paginate_by, Some(5));
        assert_eq!(config.taxonomies[0].paginate_path, "page");
        assert!(config.taxonomies[0].feed);
        assert!(config.taxonomies[0].translations.is_none());
    }

    #[test]
    fn directories_are_relative_and_non_empty() {
        for key in [
            "content_dir",
            "templates_dir",
            "static_dir",
            "sass_dir",
            "i18n_dir",
            "output_dir",
            "themes_dir",
        ] {
            let absolute = format!("{key} = \"/tmp/x\"\n{MINIMAL}");
            assert!(error_of(&absolute).contains("relative, non-empty"), "{key}");
            let empty = format!("{key} = \"\"\n{MINIMAL}");
            assert!(error_of(&empty).contains("relative, non-empty"), "{key}");
        }
        let custom = format!("output_dir = \"public\"\ncontent_dir = \"src/content\"\n{MINIMAL}");
        let config = parse(&custom).unwrap();
        assert_eq!(config.output_dir, "public");
        assert_eq!(config.content_dir, "src/content");
    }

    #[test]
    fn a_theme_is_a_bare_directory_name() {
        for theme in ["", "a/b", "a\\b", ".."] {
            let text = format!("theme = {theme:?}\n{MINIMAL}");
            assert!(
                error_of(&text).contains("must be a directory name"),
                "{theme:?}"
            );
        }
        let text = format!("theme = \"fleet\"\nthemes_dir = \"shared\"\n{MINIMAL}");
        let config = parse(&text).unwrap();
        assert_eq!(config.theme.as_deref(), Some("fleet"));
        assert_eq!(config.themes_dir, "shared");
    }

    #[test]
    fn publish_entries_stay_inside_the_site() {
        for (from, to) in [
            ("../x", "y"),
            ("/abs", "y"),
            ("x", "../y"),
            ("", "y"),
            ("x", ""),
        ] {
            let text = format!("{MINIMAL}[[publish]]\nfrom = {from:?}\nto = {to:?}\n");
            assert!(
                error_of(&text).contains("must be a relative path inside the site"),
                "{from:?} -> {to:?}"
            );
        }
        let text = format!("{MINIMAL}[[publish]]\nfrom = \"shared/x/\"\nto = \"x//\"\n");
        let config = parse(&text).unwrap();
        assert_eq!(config.publish[0].from, "shared/x");
        assert_eq!(config.publish[0].to, "x");
        assert_eq!(config.publish[0].files, vec!["**/*".to_owned()]);
        let none = format!("{MINIMAL}[[publish]]\nfrom = \"x\"\nto = \"y\"\nfiles = []\n");
        assert!(error_of(&none).contains("lists no files"));
    }

    #[test]
    fn unknown_keys_are_rejected_at_every_level() {
        for text in [
            format!("colour = 1\n{MINIMAL}"),
            format!("{MINIMAL}nam = \"x\"\n"),
            format!("{MINIMAL}[markdown]\nhighlite = true\n"),
            format!("{MINIMAL}[feeds]\nlimit = 2\nlimits = 3\n"),
            format!("{MINIMAL}[sitemap]\nenable = false\n"),
            format!("{MINIMAL}[slugify]\npath = \"ascii\"\n"),
            format!("{MINIMAL}[[taxonomies]]\nname = \"tags\"\npaginate = 2\n"),
            format!("{MINIMAL}[[publish]]\nfrom = \"a\"\nto = \"b\"\nglob = \"*\"\n"),
        ] {
            assert!(parse(&text).is_err(), "accepted: {text}");
        }
    }

    #[test]
    fn extra_is_free_form_and_the_tables_parse() {
        let text = format!(
            "{MINIMAL}[markdown]\nsmart_punctuation = true\nhighlight = false\nhighlight_theme = \"base16-ocean.dark\"\n[feeds]\nlimit = 3\nfilename = \"feed.xml\"\n[sitemap]\nenabled = false\n[slugify]\npaths = \"ascii\"\nanchors = \"ascii\"\n[extra]\nyear = 2026\n[extra.nested]\nlist = [\"a\", \"b\"]\n"
        );
        let config = parse(&text).unwrap();
        assert!(config.markdown.smart_punctuation);
        assert!(!config.markdown.highlight);
        assert_eq!(config.markdown.highlight_theme, "base16-ocean.dark");
        assert_eq!(config.feeds.limit, 3);
        assert_eq!(config.feeds.filename, "feed.xml");
        assert!(!config.sitemap.enabled);
        assert_eq!(config.slugify.paths, SlugMode::Ascii);
        assert_eq!(config.slugify.taxonomies, SlugMode::Safe);
        assert_eq!(config.slugify.anchors, SlugMode::Ascii);
        assert_eq!(config.extra["year"].as_integer(), Some(2026));
        assert_eq!(config.extra["nested"]["list"][1].as_str(), Some("b"));
    }

    #[test]
    fn direction_names() {
        assert_eq!(Direction::Ltr.as_str(), "ltr");
        assert_eq!(Direction::Rtl.as_str(), "rtl");
        assert_eq!(Direction::default(), Direction::Ltr);
    }

    #[test]
    fn load_names_the_file_in_its_errors() {
        let dir = std::env::temp_dir().join(format!("rssite-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let missing = format!("{:#}", Config::load(&dir).unwrap_err());
        assert!(missing.contains("rssite.toml"), "{missing}");
        std::fs::write(dir.join(CONFIG_FILE), "base_url = [\n").unwrap();
        let invalid = format!("{:#}", Config::load(&dir).unwrap_err());
        assert!(invalid.contains("parsing"), "{invalid}");
        std::fs::write(dir.join(CONFIG_FILE), MINIMAL).unwrap();
        assert_eq!(Config::load(&dir).unwrap().base_url, "https://example.org");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
