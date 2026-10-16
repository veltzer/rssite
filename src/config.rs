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
