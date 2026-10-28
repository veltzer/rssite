//! The site model: every page and section in every language, paired with
//! its translations, sorted, with its taxonomy terms and its rendered body.
//! Everything here is computed before a single template runs, and every
//! inconsistency (a page in a section with no index, two pages with one
//! URL, a tag the config does not declare) is an error with a file name.

use crate::config::Config;
use crate::content::{
    self, BodyKind, ContentAsset, ContentFile, FileKind, PageFront, SectionFront, SortBy,
};
use crate::i18n::{DateValue, Translations};
use crate::markdown::{self, RenderOptions, Rendered};
use crate::slug::slugify;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Page {
    pub file: ContentFile,
    pub front: PageFront,
    pub date: Option<DateValue>,
    pub updated: Option<DateValue>,
    pub section: usize,
    pub key: String,
    pub slug: String,
    pub path: String,
    pub permalink: String,
    /// This page and its translations, one per language that has one, in
    /// the configured language order.
    pub translations: Vec<usize>,
    pub previous: Option<usize>,
    pub next: Option<usize>,
    /// The page whose body this one renders (`body_from`), if not its own.
    pub body_source: Option<usize>,
    pub rendered: Rendered,
    /// Content files the body links to, relative to the site root.
    pub link_sources: Vec<String>,
    /// Section files whose `[cascade]` this page's front matter took
    /// settings from, relative to the site root.
    pub cascade_sources: Vec<String>,
    /// The pages of the same language sharing the most terms, best first.
    pub related: Vec<usize>,
}

#[derive(Debug)]
pub struct Section {
    pub file: Option<ContentFile>,
    pub front: SectionFront,
    pub updated: Option<DateValue>,
    pub lang: String,
    pub components: Vec<String>,
    pub key: String,
    pub path: String,
    pub permalink: String,
    pub pages: Vec<usize>,
    pub subsections: Vec<usize>,
    pub parent: Option<usize>,
    pub translations: Vec<usize>,
    pub body_source: Option<usize>,
    pub rendered: Rendered,
    pub link_sources: Vec<String>,
    /// The cascade this section passes down: its ancestors' merged with its
    /// own, nearer keys winning.
    pub cascade_down: toml::Table,
    /// The section files whose cascades make up `cascade_down`, relative
    /// to the site root.
    pub cascade_sources: Vec<String>,
}

impl Section {
    pub fn is_root(&self) -> bool {
        self.components.is_empty()
    }

    pub fn title(&self) -> &str {
        self.front.title.as_deref().unwrap_or("")
    }

    pub fn rel(&self) -> Option<&str> {
        self.file.as_ref().map(|f| f.rel.as_str())
    }
}

#[derive(Debug)]
pub struct Term {
    pub name: String,
    pub slug: String,
    pub path: String,
    pub permalink: String,
    pub pages: Vec<usize>,
    /// (language, term index within that language's taxonomy)
    pub translations: Vec<(String, usize)>,
}

#[derive(Debug)]
pub struct TaxonomyModel {
    pub name: String,
    pub lang: String,
    pub path: String,
    pub permalink: String,
    pub paginate_by: Option<usize>,
    pub paginate_path: String,
    pub feed: bool,
    pub terms: Vec<Term>,
    /// The translations file, relative to the site root, when configured.
    pub translations_file: Option<String>,
}

#[derive(Debug, Default)]
pub struct Templates {
    /// Template name (relative path with `/`) to source text.
    pub files: BTreeMap<String, String>,
    /// Template name to the file it was read from, relative to the site
    /// root: the site's own directory or the theme's.
    pub origins: BTreeMap<String, String>,
}

/// A file under a root directory (`static/` or `sass/`, the site's or the
/// theme's) with its path below that root, which names its output.
#[derive(Clone, Debug)]
pub struct SourceFile {
    pub path: PathBuf,
    pub rel: String,
}

/// A file a `[[publish]]` entry copies: where it is, where it goes.
#[derive(Clone, Debug)]
pub struct Published {
    pub path: PathBuf,
    /// Relative to the output root.
    pub out: String,
    /// Relative to the site root.
    pub source: String,
}

impl Templates {
    /// Every user template reachable from `name` through extends, include
    /// and import, `name` itself included when it is a user template.
    pub fn closure(&self, name: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![name.to_owned()];
        while let Some(current) = stack.pop() {
            let Some(text) = self.files.get(&current) else {
                continue;
            };
            if !seen.insert(current.clone()) {
                continue;
            }
            for found in template_references(text) {
                stack.push(found);
            }
        }
        seen
    }

    pub fn has(&self, name: &str) -> bool {
        self.files.contains_key(name)
    }
}

fn template_references(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"\{%-?\s*(?:extends|include|import)\s+"([^"]+)""#).expect("valid regex")
    });
    re.captures_iter(text).map(|c| c[1].to_owned()).collect()
}

/// Literal `load_data(path = "...")` paths in a template or body.
pub fn load_data_paths(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"load_data\([^)]*\bpath\s*=\s*"([^"]+)""#).expect("valid regex")
    });
    re.captures_iter(text).map(|c| c[1].to_owned()).collect()
}

/// Literal `get_url(path = "@/...")` content paths in a template or body.
pub fn content_url_paths(text: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r##"get_url\([^)]*\bpath\s*=\s*"@/([^"#]+)"##).expect("valid regex")
    });
    re.captures_iter(text).map(|c| c[1].to_owned()).collect()
}

/// Whether a template reads other pages, which makes the output depend on
/// the whole content tree.
pub fn reads_other_pages(text: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"get_page\(|get_section\(|get_taxonomy\(|get_taxonomy_url\(|\.pages\b|subsections|\.previous\b|\.next\b|\.related\b")
            .expect("valid regex")
    });
    re.is_match(text)
}

/// A page's or section's front matter settings that replace the site's
/// `[markdown]` ones for its body; `None` keeps the site's.
#[derive(Clone, Copy)]
struct Overrides {
    smart: Option<bool>,
    math: Option<bool>,
}

pub struct Site {
    pub root: PathBuf,
    pub config: Config,
    pub drafts: bool,
    pub i18n: BTreeMap<String, Translations>,
    pub pages: Vec<Page>,
    pub sections: Vec<Section>,
    pub taxonomies: Vec<TaxonomyModel>,
    pub templates: Templates,
    pub static_files: Vec<SourceFile>,
    pub sass_files: Vec<SourceFile>,
    pub published: Vec<Published>,
    pub assets: Vec<ContentAsset>,
    /// Every content file, relative to the site root.
    pub content_files: Vec<String>,
    /// `(lang, path below content without language suffix)` to page index.
    pub page_lookup: BTreeMap<(String, String), usize>,
    pub section_lookup: BTreeMap<(String, String), usize>,
    /// `(directory components, language)` of every section index dropped
    /// as a draft: its pages and assets go with it.
    pub drafted_sections: BTreeSet<(Vec<String>, String)>,
    /// The commit the site root is checked out at, when it is in git.
    pub git: Option<crate::git::GitInfo>,
    /// When this build ran, the same for every page it writes.
    pub build: crate::build_info::BuildInfo,
}

/// A content path with the language suffix removed: `blog/post.en.md` ->
/// `blog/post.md`.
pub(crate) fn strip_lang(rel: &str, lang: &str) -> String {
    let Some((before_ext, ext)) = rel.rsplit_once('.') else {
        return rel.to_owned();
    };
    if ext.contains('/') {
        return rel.to_owned();
    }
    match before_ext.strip_suffix(&format!(".{lang}")) {
        Some(stem) if !stem.is_empty() && !stem.ends_with('/') => format!("{stem}.{ext}"),
        _ => rel.to_owned(),
    }
}

impl Site {
    pub fn load(root: &Path, drafts: bool) -> Result<Site> {
        let root = root
            .canonicalize()
            .with_context(|| format!("site root {}", root.display()))?;
        let config = Config::load(&root)?;
        let codes = config.language_codes();

        let theme_dir = match &config.theme {
            Some(name) => {
                let dir = root.join(&config.themes_dir).join(name);
                if !dir.is_dir() {
                    bail!(
                        "theme {name:?} not found: {}/{name}/ is not a directory",
                        config.themes_dir
                    );
                }
                Some(dir)
            }
            None => None,
        };
        // Translation roots, the theme's first so the site's override it.
        let i18n_dirs: Vec<PathBuf> = theme_dir
            .iter()
            .map(|d| d.join(&config.i18n_dir))
            .chain(std::iter::once(root.join(&config.i18n_dir)))
            .collect();
        if let Some(theme) = &theme_dir {
            // A theme speaking a language the site does not declare is a
            // mistake on one side or the other, not something to skip.
            for path in crate::util::walk_files(&theme.join(&config.i18n_dir))? {
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let is_toml = path.extension().is_some_and(|e| e == "toml");
                if is_toml && !codes.contains(&stem) {
                    bail!(
                        "{} is for language {stem:?}, which the site does not declare",
                        crate::util::rel(&path, &root)
                    );
                }
            }
        }
        let mut i18n = BTreeMap::new();
        for code in &codes {
            i18n.insert(code.clone(), Translations::load_layered(&i18n_dirs, code)?);
        }

        let content_dir = root.join(&config.content_dir);
        let (files, assets) = content::scan(&content_dir, &codes)?;

        let mut site = Site {
            root: root.clone(),
            config,
            drafts,
            i18n,
            pages: Vec::new(),
            sections: Vec::new(),
            taxonomies: Vec::new(),
            templates: Templates::default(),
            static_files: Vec::new(),
            sass_files: Vec::new(),
            published: Vec::new(),
            assets,
            content_files: Vec::new(),
            page_lookup: BTreeMap::new(),
            section_lookup: BTreeMap::new(),
            drafted_sections: BTreeSet::new(),
            git: None,
            build: crate::build_info::BuildInfo::now(),
        };
        site.content_files = files
            .iter()
            .map(|f| crate::util::rel(&f.path, &root))
            .collect();
        site.build_sections(&files)?;
        site.build_pages(files)?;
        site.pair_translations()?;
        site.require_translations()?;
        site.resolve_body_sources()?;
        site.sort_sections()?;
        site.build_taxonomies()?;
        site.relate_pages();
        site.git = crate::git::GitInfo::read(&site.root);
        site.render_bodies()?;
        site.load_templates()?;
        site.static_files = site.layered_files(&site.config.static_dir)?;
        site.sass_files = site.layered_files(&site.config.sass_dir)?;
        site.published = site.published_files()?;
        Ok(site)
    }

    /// The theme's directory, when a theme is configured.
    pub fn theme_dir(&self) -> Option<PathBuf> {
        self.config
            .theme
            .as_ref()
            .map(|name| self.root.join(&self.config.themes_dir).join(name))
    }

    /// The roots a layered directory (`templates`, `static`, `sass`, `i18n`)
    /// is read from: the theme's first, the site's last, so the site wins.
    fn roots(&self) -> Vec<PathBuf> {
        self.theme_dir()
            .into_iter()
            .chain(std::iter::once(self.root.clone()))
            .collect()
    }

    /// The files under `<root>/<dir>` for every root, keyed by their path
    /// below the directory; a later root's file replaces an earlier one's.
    fn layered_files(&self, dir: &str) -> Result<Vec<SourceFile>> {
        let mut by_rel: BTreeMap<String, PathBuf> = BTreeMap::new();
        for root in self.roots() {
            let base = root.join(dir);
            for path in crate::util::walk_files(&base)? {
                by_rel.insert(crate::util::rel(&path, &base), path);
            }
        }
        Ok(by_rel
            .into_iter()
            .map(|(rel, path)| SourceFile { path, rel })
            .collect())
    }

    /// The files every `[[publish]]` entry copies, in config then path order.
    fn published_files(&self) -> Result<Vec<Published>> {
        let mut out = Vec::new();
        for entry in &self.config.publish {
            let from = self.root.join(&entry.from);
            if !from.is_dir() {
                bail!(
                    "[[publish]] from = {:?} is not a directory (a submodule not checked out?)",
                    entry.from
                );
            }
            let mut matched = 0usize;
            for path in crate::util::walk_files(&from)? {
                let rel = crate::util::rel(&path, &from);
                let mut hit = false;
                for pattern in &entry.files {
                    hit |= crate::util::glob_match(pattern, &rel)?;
                }
                if !hit {
                    continue;
                }
                matched += 1;
                out.push(Published {
                    out: format!("{}/{rel}", entry.to),
                    source: format!("{}/{rel}", entry.from),
                    path,
                });
            }
            if matched == 0 {
                bail!(
                    "[[publish]] from = {:?} matches no file with files = {:?}",
                    entry.from,
                    entry.files
                );
            }
        }
        Ok(out)
    }

    pub fn rel(&self, path: &Path) -> String {
        crate::util::rel(path, &self.root)
    }

    fn prefix(&self, lang: &str) -> String {
        self.config
            .language(lang)
            .map(|l| l.prefix().to_owned())
            .unwrap_or_else(|| format!("/{lang}/"))
    }

    pub(crate) fn permalink(&self, path: &str) -> String {
        format!("{}{}", self.config.base_url, path)
    }

    fn build_sections(&mut self, files: &[ContentFile]) -> Result<()> {
        let codes = self.config.language_codes();
        // Every (directory, language) that has an index file, plus a root
        // section for every language.
        let mut seen_sections: BTreeMap<(Vec<String>, String), String> = BTreeMap::new();
        let mut drafted: BTreeSet<(Vec<String>, String)> = BTreeSet::new();
        for file in files.iter().filter(|f| f.kind == FileKind::Index) {
            if let Some(other) = seen_sections.insert(
                (file.components.clone(), file.lang.clone()),
                file.rel.clone(),
            ) {
                let dir = if file.components.is_empty() {
                    "(root)".to_owned()
                } else {
                    file.components.join("/")
                };
                bail!(
                    "content files {other} and {} are both the {:?} section index of {dir}",
                    file.rel,
                    file.lang
                );
            }
            let parsed = content::parse_section_front(file.front.clone())
                .with_context(|| format!("content file {}", file.rel))?;
            let ancestor_drafted = (0..=file.components.len())
                .any(|n| drafted.contains(&(file.components[..n].to_vec(), file.lang.clone())));
            if (parsed.front.draft && !self.drafts) || ancestor_drafted {
                drafted.insert((file.components.clone(), file.lang.clone()));
                continue;
            }
            let path = self.section_path(&file.lang, &file.components)?;
            let key = parsed
                .front
                .translation_key
                .clone()
                .unwrap_or_else(|| file.components.join("/"));
            let section = Section {
                file: Some(file.clone()),
                front: parsed.front,
                updated: parsed.updated,
                lang: file.lang.clone(),
                components: file.components.clone(),
                key,
                permalink: self.permalink(&path),
                path,
                pages: Vec::new(),
                subsections: Vec::new(),
                parent: None,
                translations: Vec::new(),
                body_source: None,
                rendered: Rendered::default(),
                link_sources: Vec::new(),
                cascade_down: toml::Table::new(),
                cascade_sources: Vec::new(),
            };
            let lookup_key = (file.lang.clone(), strip_lang(&file.rel, &file.lang));
            self.section_lookup.insert(lookup_key, self.sections.len());
            self.sections.push(section);
        }
        for code in &codes {
            let exists = self.sections.iter().any(|s| s.lang == *code && s.is_root());
            if exists || drafted.contains(&(Vec::new(), code.clone())) {
                continue;
            }
            let language = self.config.language(code).expect("configured language");
            let path = self.section_path(code, &[])?;
            let section = Section {
                file: None,
                front: SectionFront {
                    title: Some(
                        language
                            .title
                            .clone()
                            .unwrap_or_else(|| self.config.title.clone()),
                    ),
                    description: language
                        .description
                        .clone()
                        .or_else(|| Some(self.config.description.clone())),
                    ..SectionFront::default()
                },
                updated: None,
                lang: code.clone(),
                components: Vec::new(),
                key: String::new(),
                permalink: self.permalink(&path),
                path,
                pages: Vec::new(),
                subsections: Vec::new(),
                parent: None,
                translations: Vec::new(),
                body_source: None,
                rendered: Rendered::default(),
                link_sources: Vec::new(),
                cascade_down: toml::Table::new(),
                cascade_sources: Vec::new(),
            };
            self.section_lookup
                .insert((code.clone(), "_index.md".to_owned()), self.sections.len());
            self.sections.push(section);
        }
        // Parents: the nearest ancestor directory with a section in the same
        // language.
        let index: BTreeMap<(String, Vec<String>), usize> = self
            .sections
            .iter()
            .enumerate()
            .map(|(i, s)| ((s.lang.clone(), s.components.clone()), i))
            .collect();
        for i in 0..self.sections.len() {
            let lang = self.sections[i].lang.clone();
            let mut components = self.sections[i].components.clone();
            while !components.is_empty() {
                components.pop();
                if let Some(&parent) = index.get(&(lang.clone(), components.clone())) {
                    self.sections[i].parent = Some(parent);
                    self.sections[parent].subsections.push(i);
                    break;
                }
            }
        }
        self.drafted_sections = drafted;
        self.cascade_sections();
        Ok(())
    }

    /// Hand each section's `[cascade]` down the tree: parents before
    /// children, so a section receives its ancestors' merged cascade, fills
    /// in its own front matter from it, and passes on that merged with its
    /// own.
    fn cascade_sections(&mut self) {
        let mut order: Vec<usize> = (0..self.sections.len()).collect();
        order.sort_by_key(|&i| self.sections[i].components.len());
        for i in order {
            let (inherited, mut sources) = match self.sections[i].parent {
                Some(p) => (
                    self.sections[p].cascade_down.clone(),
                    self.sections[p].cascade_sources.clone(),
                ),
                None => (toml::Table::new(), Vec::new()),
            };
            if !self.sections[i].front.cascade.is_empty()
                && let Some(file) = &self.sections[i].file
            {
                sources.push(self.rel(&file.path));
            }
            let section = &mut self.sections[i];
            if let Some(math) = inherited.get("math").and_then(toml::Value::as_bool) {
                section.front.math.get_or_insert(math);
            }
            if let Some(smart) = inherited
                .get("smart_punctuation")
                .and_then(toml::Value::as_bool)
            {
                section.front.smart_punctuation.get_or_insert(smart);
            }
            if let Some(toml::Value::Table(extra)) = inherited.get("extra") {
                for (k, v) in extra {
                    section
                        .front
                        .extra
                        .entry(k.clone())
                        .or_insert_with(|| v.clone());
                }
            }
            section.cascade_down =
                crate::content::merge_cascade(&inherited, &section.front.cascade);
            section.cascade_sources = sources;
        }
    }

    /// Whether a content file in `components` is under a section index of
    /// `lang` that was dropped as a draft, at that directory or above.
    pub fn under_drafted_section(&self, components: &[String], lang: &str) -> bool {
        (0..=components.len()).any(|n| {
            self.drafted_sections
                .contains(&(components[..n].to_vec(), lang.to_owned()))
        })
    }

    pub(crate) fn section_path(&self, lang: &str, components: &[String]) -> Result<String> {
        let mut path = self.prefix(lang);
        for component in components {
            let slug = slugify(component, self.config.slugify.paths);
            if slug.is_empty() {
                bail!("content directory {component:?} has no characters usable in a URL");
            }
            path.push_str(&slug);
            path.push('/');
        }
        Ok(path)
    }

    fn build_pages(&mut self, files: Vec<ContentFile>) -> Result<()> {
        let section_index: BTreeMap<(String, Vec<String>), usize> = self
            .sections
            .iter()
            .enumerate()
            .map(|(i, s)| ((s.lang.clone(), s.components.clone()), i))
            .collect();
        let mut by_url: BTreeMap<String, String> = BTreeMap::new();
        for file in files.into_iter().filter(|f| f.kind == FileKind::Page) {
            let mut front = file.front.clone();
            let cascade = section_index
                .get(&(file.lang.clone(), file.components.clone()))
                .map(|&s| &self.sections[s]);
            if let Some(section) = cascade {
                content::apply_cascade(&mut front, &section.cascade_down);
            }
            let cascade_sources = cascade
                .map(|s| s.cascade_sources.clone())
                .unwrap_or_default();
            let parsed = content::parse_page_front(front)
                .with_context(|| format!("content file {}", file.rel))?;
            if parsed.front.draft && !self.drafts {
                continue;
            }
            // A drafted section takes its pages with it.
            if self.under_drafted_section(&file.components, &file.lang) {
                continue;
            }
            let Some(&section) = section_index.get(&(file.lang.clone(), file.components.clone()))
            else {
                let dir = if file.components.is_empty() {
                    String::new()
                } else {
                    format!("{}/", file.components.join("/"))
                };
                bail!(
                    "content file {}: its directory has no section index for language {:?}; add {dir}_index.{}.md",
                    file.rel,
                    file.lang,
                    file.lang
                );
            };
            let slug = match &parsed.front.slug {
                Some(slug) => slugify(slug, self.config.slugify.paths),
                None => slugify(&file.stem, self.config.slugify.paths),
            };
            if slug.is_empty() {
                bail!(
                    "content file {}: its name has no characters usable in a URL; set `slug` in the front matter",
                    file.rel
                );
            }
            let path = format!("{}{}/", self.sections[section].path, slug);
            if let Some(other) = by_url.insert(path.clone(), file.rel.clone()) {
                bail!(
                    "content files {other} and {} both render to {path}",
                    file.rel
                );
            }
            let title = parsed.front.title.clone();
            if title.is_none() && file.body_kind == BodyKind::Markdown {
                bail!("content file {}: front matter has no title", file.rel);
            }
            let key = format!(
                "{}/{}",
                file.components.join("/"),
                parsed
                    .front
                    .translation_key
                    .clone()
                    .unwrap_or_else(|| file.stem.clone())
            );
            let lookup_key = (file.lang.clone(), strip_lang(&file.rel, &file.lang));
            let page = Page {
                front: parsed.front,
                date: parsed.date,
                updated: parsed.updated,
                section,
                key,
                slug,
                permalink: self.permalink(&path),
                path,
                translations: Vec::new(),
                previous: None,
                next: None,
                body_source: None,
                rendered: Rendered::default(),
                link_sources: Vec::new(),
                cascade_sources,
                related: Vec::new(),
                file,
            };
            self.page_lookup.insert(lookup_key, self.pages.len());
            self.sections[section].pages.push(self.pages.len());
            self.pages.push(page);
        }
        Ok(())
    }

    fn pair_translations(&mut self) -> Result<()> {
        let codes = self.config.language_codes();
        let mut groups: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for (i, page) in self.pages.iter().enumerate() {
            if let Some(other) = groups
                .entry(page.key.clone())
                .or_default()
                .insert(page.file.lang.clone(), i)
            {
                bail!(
                    "content files {} and {} are both the {:?} translation of {:?}; give one a different translation_key",
                    self.pages[other].file.rel,
                    page.file.rel,
                    page.file.lang,
                    page.key
                );
            }
        }
        let keys: Vec<String> = self.pages.iter().map(|p| p.key.clone()).collect();
        for (i, key) in keys.iter().enumerate() {
            let group = &groups[key];
            self.pages[i].translations =
                codes.iter().filter_map(|c| group.get(c).copied()).collect();
        }
        let mut section_groups: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for (i, section) in self.sections.iter().enumerate() {
            if let Some(other) = section_groups
                .entry(section.key.clone())
                .or_default()
                .insert(section.lang.clone(), i)
            {
                bail!(
                    "content files {} and {} are both the {:?} translation of section {:?}; give one a different translation_key",
                    self.sections[other].rel().unwrap_or("_index"),
                    section.rel().unwrap_or("_index"),
                    section.lang,
                    section.key
                );
            }
        }
        let keys: Vec<String> = self.sections.iter().map(|s| s.key.clone()).collect();
        for (i, key) in keys.iter().enumerate() {
            let group = &section_groups[key];
            self.sections[i].translations =
                codes.iter().filter_map(|c| group.get(c).copied()).collect();
        }
        Ok(())
    }

    /// A section with `require_translations` has every page in every
    /// language the section itself exists in.
    fn require_translations(&self) -> Result<()> {
        for section in self
            .sections
            .iter()
            .filter(|s| s.front.require_translations)
        {
            let languages: Vec<&str> = section
                .translations
                .iter()
                .map(|&t| self.sections[t].lang.as_str())
                .collect();
            for &p in &section.pages {
                let page = &self.pages[p];
                let missing: Vec<&str> = languages
                    .iter()
                    .copied()
                    .filter(|lang| {
                        !page
                            .translations
                            .iter()
                            .any(|&t| self.pages[t].file.lang == *lang)
                    })
                    .collect();
                if !missing.is_empty() {
                    bail!(
                        "content file {}: section {} sets require_translations, but this page has no {} translation",
                        page.file.rel,
                        section.rel().unwrap_or("(root)"),
                        missing.join(", ")
                    );
                }
            }
        }
        Ok(())
    }

    fn resolve_body_sources(&mut self) -> Result<()> {
        for i in 0..self.pages.len() {
            let Some(from) = self.pages[i].front.body_from.clone() else {
                continue;
            };
            let rel = self.pages[i].file.rel.clone();
            if from == self.pages[i].file.lang {
                bail!("content file {rel}: body_from names its own language");
            }
            let Some(&source) = self.pages[i]
                .translations
                .iter()
                .find(|&&t| self.pages[t].file.lang == from)
            else {
                bail!(
                    "content file {rel}: body_from = {from:?} but this page has no {from:?} translation"
                );
            };
            if self.pages[source].front.body_from.is_some() {
                bail!(
                    "content file {rel}: body_from points at {}, which itself uses body_from",
                    self.pages[source].file.rel
                );
            }
            if !self.pages[i].file.body.trim().is_empty() {
                bail!("content file {rel}: has both a body and body_from; remove one");
            }
            self.pages[i].body_source = Some(source);
        }
        for i in 0..self.sections.len() {
            let Some(from) = self.sections[i].front.body_from.clone() else {
                continue;
            };
            let rel = self.sections[i].rel().unwrap_or("_index").to_owned();
            if from == self.sections[i].lang {
                bail!("content file {rel}: body_from names its own language");
            }
            let Some(&source) = self.sections[i]
                .translations
                .iter()
                .find(|&&t| self.sections[t].lang == from && self.sections[t].file.is_some())
            else {
                bail!(
                    "content file {rel}: body_from = {from:?} but this section has no {from:?} translation"
                );
            };
            if self.sections[source].front.body_from.is_some() {
                bail!(
                    "content file {rel}: body_from points at a section which itself uses body_from"
                );
            }
            if self.sections[i]
                .file
                .as_ref()
                .is_some_and(|f| !f.body.trim().is_empty())
            {
                bail!("content file {rel}: has both a body and body_from; remove one");
            }
            self.sections[i].body_source = Some(source);
        }
        Ok(())
    }

    fn sort_sections(&mut self) -> Result<()> {
        for s in 0..self.sections.len() {
            let sort_by = self.sections[s].front.sort_by;
            let mut pages = std::mem::take(&mut self.sections[s].pages);
            match sort_by {
                SortBy::Date => {
                    let undated: Vec<&str> = pages
                        .iter()
                        .filter(|&&p| self.pages[p].date.is_none())
                        .map(|&p| self.pages[p].file.rel.as_str())
                        .collect();
                    if !undated.is_empty() {
                        bail!(
                            "section {} sorts by date but these pages have none: {}",
                            self.sections[s].rel().unwrap_or("(root)"),
                            undated.join(", ")
                        );
                    }
                    pages.sort_by(|&a, &b| {
                        self.pages[b]
                            .date
                            .cmp(&self.pages[a].date)
                            .then_with(|| self.pages[a].file.rel.cmp(&self.pages[b].file.rel))
                    });
                }
                SortBy::Weight => {
                    let unweighted: Vec<&str> = pages
                        .iter()
                        .filter(|&&p| self.pages[p].front.weight.is_none())
                        .map(|&p| self.pages[p].file.rel.as_str())
                        .collect();
                    if !unweighted.is_empty() {
                        bail!(
                            "section {} sorts by weight but these pages have none: {}",
                            self.sections[s].rel().unwrap_or("(root)"),
                            unweighted.join(", ")
                        );
                    }
                    pages.sort_by(|&a, &b| {
                        self.pages[a]
                            .front
                            .weight
                            .cmp(&self.pages[b].front.weight)
                            .then_with(|| self.pages[a].file.rel.cmp(&self.pages[b].file.rel))
                    });
                }
                SortBy::Title => pages.sort_by(|&a, &b| {
                    self.pages[a]
                        .front
                        .title
                        .cmp(&self.pages[b].front.title)
                        .then_with(|| self.pages[a].file.rel.cmp(&self.pages[b].file.rel))
                }),
                SortBy::None => {
                    pages.sort_by(|&a, &b| self.pages[a].file.rel.cmp(&self.pages[b].file.rel))
                }
            }
            for (position, &p) in pages.iter().enumerate() {
                self.pages[p].previous = (position > 0).then(|| pages[position - 1]);
                self.pages[p].next = pages.get(position + 1).copied();
            }
            self.sections[s].pages = pages;
            let mut subsections = std::mem::take(&mut self.sections[s].subsections);
            subsections.sort_by(|&a, &b| {
                self.sections[a]
                    .front
                    .weight
                    .unwrap_or(0)
                    .cmp(&self.sections[b].front.weight.unwrap_or(0))
                    .then_with(|| self.sections[a].path.cmp(&self.sections[b].path))
            });
            self.sections[s].subsections = subsections;
        }
        if let Some(bad) = self
            .sections
            .iter()
            .find(|s| s.front.paginate_by == Some(0))
        {
            bail!(
                "section {}: paginate_by must be at least 1",
                bad.rel().unwrap_or("(root)")
            );
        }
        Ok(())
    }

    fn build_taxonomies(&mut self) -> Result<()> {
        let codes = self.config.language_codes();
        // (lang, taxonomy name) -> term name -> page indexes
        let mut collected: BTreeMap<(String, String), BTreeMap<String, Vec<usize>>> =
            BTreeMap::new();
        for code in &codes {
            for taxonomy in &self.config.taxonomies {
                collected.insert((code.clone(), taxonomy.name.clone()), BTreeMap::new());
            }
        }
        for (i, page) in self.pages.iter().enumerate() {
            for (name, terms) in &page.front.taxonomies {
                let Some(bucket) = collected.get_mut(&(page.file.lang.clone(), name.clone()))
                else {
                    bail!(
                        "content file {}: taxonomy {name:?} is not declared in rssite.toml ([[taxonomies]])",
                        page.file.rel
                    );
                };
                for term in terms {
                    if term.trim().is_empty() {
                        bail!(
                            "content file {}: taxonomy {name:?} has an empty term",
                            page.file.rel
                        );
                    }
                    bucket.entry(term.clone()).or_default().push(i);
                }
            }
        }
        let mut translation_tables: BTreeMap<String, Vec<BTreeMap<String, String>>> =
            BTreeMap::new();
        for taxonomy in &self.config.taxonomies {
            if let Some(file) = &taxonomy.translations {
                translation_tables
                    .insert(taxonomy.name.clone(), self.load_term_translations(file)?);
            } else if taxonomy.infer_translations {
                translation_tables.insert(
                    taxonomy.name.clone(),
                    self.infer_term_translations(&taxonomy.name)?,
                );
            }
        }
        for code in &codes {
            for taxonomy in &self.config.taxonomies {
                let path = format!(
                    "{}{}/",
                    self.prefix(code),
                    slugify(&taxonomy.name, self.config.slugify.taxonomies)
                );
                let mut terms = Vec::new();
                let mut by_slug: BTreeMap<String, String> = BTreeMap::new();
                for (name, mut pages) in collected
                    .remove(&(code.clone(), taxonomy.name.clone()))
                    .unwrap_or_default()
                {
                    let slug = slugify(&name, self.config.slugify.taxonomies);
                    if slug.is_empty() {
                        bail!(
                            "taxonomy {}: term {name:?} has no characters usable in a URL",
                            taxonomy.name
                        );
                    }
                    if let Some(other) = by_slug.insert(slug.clone(), name.clone()) {
                        bail!(
                            "taxonomy {}: terms {other:?} and {name:?} both slugify to {slug:?} in language {code}",
                            taxonomy.name
                        );
                    }
                    pages.sort_by(|&a, &b| {
                        self.pages[b]
                            .date
                            .cmp(&self.pages[a].date)
                            .then_with(|| self.pages[a].file.rel.cmp(&self.pages[b].file.rel))
                    });
                    pages.dedup();
                    let term_path = format!("{path}{slug}/");
                    terms.push(Term {
                        name,
                        slug,
                        permalink: self.permalink(&term_path),
                        path: term_path,
                        pages,
                        translations: Vec::new(),
                    });
                }
                self.taxonomies.push(TaxonomyModel {
                    name: taxonomy.name.clone(),
                    lang: code.clone(),
                    permalink: self.permalink(&path),
                    path,
                    paginate_by: taxonomy.paginate_by,
                    paginate_path: taxonomy.paginate_path.clone(),
                    feed: taxonomy.feed,
                    terms,
                    translations_file: taxonomy.translations.clone(),
                });
            }
        }
        if let Some(bad) = self.taxonomies.iter().find(|t| t.paginate_by == Some(0)) {
            bail!("taxonomy {}: paginate_by must be at least 1", bad.name);
        }
        // Resolve term translations: every (lang, name) in a row that exists
        // as a term is paired with every other existing one in that row.
        for (name, rows) in translation_tables {
            for row in rows {
                let mut present: Vec<(String, usize, usize)> = Vec::new();
                for (lang, term_name) in &row {
                    let Some(t) = self
                        .taxonomies
                        .iter()
                        .position(|t| t.name == name && &t.lang == lang)
                    else {
                        bail!(
                            "taxonomy {name} translations: {lang:?} is not a configured language"
                        );
                    };
                    if let Some(term) = self.taxonomies[t]
                        .terms
                        .iter()
                        .position(|term| &term.name == term_name)
                    {
                        present.push((lang.clone(), t, term));
                    }
                }
                for (_, t, term) in &present {
                    let others: Vec<(String, usize)> = present
                        .iter()
                        .map(|(lang, _, other_term)| (lang.clone(), *other_term))
                        .collect();
                    let slot = &mut self.taxonomies[*t].terms[*term].translations;
                    if !slot.is_empty() {
                        bail!(
                            "taxonomy {name} translations: term {:?} appears in two rows",
                            self.taxonomies[*t].terms[*term].name
                        );
                    }
                    *slot = others;
                }
            }
        }
        Ok(())
    }

    /// `related` for every page: the pages of its language sharing the most
    /// terms of the counted taxonomies, then the newest, then by file name.
    fn relate_pages(&mut self) {
        let limit = self.config.related.limit;
        let counted = &self.config.related.taxonomies;
        let mut all = Vec::with_capacity(self.pages.len());
        for (i, page) in self.pages.iter().enumerate() {
            let mut shared: BTreeMap<usize, usize> = BTreeMap::new();
            if limit > 0 {
                for (name, terms) in &page.front.taxonomies {
                    if !counted.is_empty() && !counted.contains(name) {
                        continue;
                    }
                    let Some(taxonomy) = self
                        .taxonomies
                        .iter()
                        .find(|t| t.name == *name && t.lang == page.file.lang)
                    else {
                        continue;
                    };
                    for term in terms {
                        let Some(term) = taxonomy.terms.iter().find(|t| t.name == *term) else {
                            continue;
                        };
                        for &other in term.pages.iter().filter(|&&o| o != i) {
                            *shared.entry(other).or_default() += 1;
                        }
                    }
                }
            }
            let mut related: Vec<(usize, usize)> = shared.into_iter().collect();
            related.sort_by(|&(a, ca), &(b, cb)| {
                cb.cmp(&ca)
                    .then_with(|| self.pages[b].date.cmp(&self.pages[a].date))
                    .then_with(|| self.pages[a].file.rel.cmp(&self.pages[b].file.rel))
            });
            related.truncate(limit);
            all.push(related.into_iter().map(|(p, _)| p).collect());
        }
        for (page, related) in self.pages.iter_mut().zip(all) {
            page.related = related;
        }
    }

    /// Term translation rows read off the pages: a page and its translation
    /// list their terms in the same order, so position N pairs the two Nth
    /// terms. Pairs chain into rows (en-he from one page, he-fr from
    /// another), and a row holding two terms of one language is an error:
    /// some page lists its terms in a different order than its translation.
    fn infer_term_translations(&self, name: &str) -> Result<Vec<BTreeMap<String, String>>> {
        // Union-find over (lang, term), each union remembering the page
        // that made it so a conflict can name a file.
        let mut ids: BTreeMap<(String, String), usize> = BTreeMap::new();
        let mut parent: Vec<usize> = Vec::new();
        fn find(parent: &mut [usize], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }
        let mut evidence: BTreeMap<(String, String), String> = BTreeMap::new();
        let empty = Vec::new();
        for (i, page) in self.pages.iter().enumerate() {
            // Each group once, from its first member.
            if page.translations.first() != Some(&i) || page.translations.len() < 2 {
                continue;
            }
            let lists: Vec<(usize, &Vec<String>)> = page
                .translations
                .iter()
                .map(|&t| {
                    (
                        t,
                        self.pages[t].front.taxonomies.get(name).unwrap_or(&empty),
                    )
                })
                .collect();
            let (first, first_terms) = lists[0];
            for &(other, terms) in &lists[1..] {
                if terms.len() != first_terms.len() {
                    bail!(
                        "taxonomy {name} infers its translations from the pages, but content file {} lists {} {name} and its translation {} lists {}",
                        self.pages[first].file.rel,
                        first_terms.len(),
                        self.pages[other].file.rel,
                        terms.len()
                    );
                }
            }
            for position in 0..first_terms.len() {
                let mut members = Vec::new();
                for &(t, terms) in &lists {
                    let key = (self.pages[t].file.lang.clone(), terms[position].clone());
                    evidence
                        .entry(key.clone())
                        .or_insert_with(|| self.pages[t].file.rel.clone());
                    let next = ids.len();
                    let id = *ids.entry(key).or_insert(next);
                    if id == parent.len() {
                        parent.push(id);
                    }
                    members.push(id);
                }
                for &m in &members[1..] {
                    let (a, b) = (find(&mut parent, members[0]), find(&mut parent, m));
                    parent[b] = a;
                }
            }
        }
        let mut rows: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        for ((lang, term), &id) in &ids {
            let root = find(&mut parent, id);
            let row = rows.entry(root).or_default();
            if let Some(other) = row.insert(lang.clone(), term.clone()) {
                bail!(
                    "taxonomy {name}: the {lang:?} terms {other:?} and {term:?} are both paired with the same term in another language (see {} and {}); a page and its translation must list their terms in the same order",
                    evidence[&(lang.clone(), other.clone())],
                    evidence[&(lang.clone(), term.clone())]
                );
            }
        }
        Ok(rows.into_values().collect())
    }

    fn load_term_translations(&self, file: &str) -> Result<Vec<BTreeMap<String, String>>> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Table {
            #[serde(default)]
            terms: Vec<BTreeMap<String, String>>,
        }
        let path = self.root.join(file);
        let text = crate::util::read(&path)?;
        let table: Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(table.terms)
    }

    /// Resolve a `@/` content path for links written in `lang`.
    pub fn resolve_content_path(&self, path: &str, lang: &str) -> Option<(String, String)> {
        // Explicit language suffix wins; otherwise the current language.
        let codes = self.config.language_codes();
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let without_ext = file_name
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(file_name);
        let (lang, key) = match without_ext.rsplit_once('.') {
            Some((_, suffix)) if codes.iter().any(|c| c == suffix) => {
                (suffix.to_owned(), strip_lang(path, suffix))
            }
            _ => (lang.to_owned(), path.to_owned()),
        };
        if let Some(&p) = self.page_lookup.get(&(lang.clone(), key.clone())) {
            return Some((
                self.pages[p].permalink.clone(),
                self.pages[p].file.rel.clone(),
            ));
        }
        if let Some(&s) = self.section_lookup.get(&(lang.clone(), key.clone())) {
            let section = &self.sections[s];
            return Some((
                section.permalink.clone(),
                section.rel().unwrap_or("").to_owned(),
            ));
        }
        if let Some(asset) = self.assets.iter().find(|a| {
            let rel = a.rel();
            rel == path || rel == key
        }) && !self.under_drafted_section(&asset.components, &lang)
            && let Ok(dir) = self.section_path(&lang, &asset.components)
        {
            let permalink = self.permalink(&format!("{dir}{}", asset.name));
            return Some((permalink, asset.rel()));
        }
        None
    }

    fn render_options(&self, overrides: Overrides) -> RenderOptions {
        let site = self.config.render_options();
        RenderOptions {
            smart_punctuation: overrides.smart.unwrap_or(site.smart_punctuation),
            math: overrides.math.unwrap_or(site.math),
            ..site
        }
    }

    fn render_bodies(&mut self) -> Result<()> {
        let content_dir = self.config.content_dir.clone();
        for i in 0..self.pages.len() {
            let source = self.pages[i].body_source.unwrap_or(i);
            let lang = self.pages[i].file.lang.clone();
            let body_kind = self.pages[source].file.body_kind;
            let body = self.pages[source].file.body.clone();
            let (page, from) = (&self.pages[i].front, &self.pages[source].front);
            let overrides = Overrides {
                smart: page.smart_punctuation.or(from.smart_punctuation),
                math: page.math.or(from.math),
            };
            let (rendered, links) = self
                .render_body(body_kind, &body, &lang, overrides)
                .with_context(|| format!("content file {}", self.pages[i].file.rel))?;
            self.pages[i].rendered = rendered;
            self.pages[i].link_sources = links
                .into_iter()
                .map(|rel| format!("{content_dir}/{rel}"))
                .collect();
        }
        for i in 0..self.sections.len() {
            let source = self.sections[i].body_source.unwrap_or(i);
            let Some(file) = self.sections[source].file.clone() else {
                continue;
            };
            let lang = self.sections[i].lang.clone();
            let (section, from) = (&self.sections[i].front, &self.sections[source].front);
            let overrides = Overrides {
                smart: section.smart_punctuation.or(from.smart_punctuation),
                math: section.math.or(from.math),
            };
            let (rendered, links) = self
                .render_body(file.body_kind, &file.body, &lang, overrides)
                .with_context(|| format!("content file {}", file.rel))?;
            self.sections[i].rendered = rendered;
            self.sections[i].link_sources = links
                .into_iter()
                .map(|rel| format!("{content_dir}/{rel}"))
                .collect();
        }
        Ok(())
    }

    fn render_body(
        &self,
        kind: BodyKind,
        body: &str,
        lang: &str,
        overrides: Overrides,
    ) -> Result<(Rendered, Vec<String>)> {
        match kind {
            BodyKind::Markdown => {
                let options = RenderOptions {
                    code_ltr: self
                        .config
                        .language(lang)
                        .is_some_and(|l| l.direction == crate::config::Direction::Rtl),
                    ..self.render_options(overrides)
                };
                let resolve = |path: &str| {
                    self.resolve_content_path(path, lang)
                        .map(|(permalink, _)| permalink)
                };
                let rendered = markdown::render(body, &options, &resolve)?;
                let links = rendered
                    .links
                    .iter()
                    .filter_map(|l| self.resolve_content_path(l, lang).map(|(_, rel)| rel))
                    .filter(|rel| !rel.is_empty())
                    .collect();
                Ok((rendered, links))
            }
            BodyKind::Html => Ok((
                Rendered {
                    html: body.to_owned(),
                    summary: body
                        .find(markdown::SUMMARY_MARKER)
                        .map(|at| body[..at].trim_end().to_owned()),
                    toc: Vec::new(),
                    word_count: body.split_whitespace().count(),
                    links: Vec::new(),
                },
                Vec::new(),
            )),
            // Rendered at template time, with the page's context.
            BodyKind::Tera => Ok((Rendered::default(), Vec::new())),
        }
    }

    /// The theme's templates first, then the site's, which replace a theme
    /// template of the same name.
    fn load_templates(&mut self) -> Result<()> {
        let mut files = BTreeMap::new();
        let mut origins = BTreeMap::new();
        for root in self.roots() {
            let dir = root.join(&self.config.templates_dir);
            for path in crate::util::walk_files(&dir)? {
                let name = crate::util::rel(&path, &dir);
                files.insert(name.clone(), crate::util::read(&path)?);
                origins.insert(name, self.rel(&path));
            }
        }
        self.templates = Templates { files, origins };
        Ok(())
    }

    pub fn page_template(&self, page: &Page) -> String {
        page.front
            .template
            .clone()
            .or_else(|| self.sections[page.section].front.page_template.clone())
            .unwrap_or_else(|| "page.html".to_owned())
    }

    /// The years of a section's dated pages, newest first, each with the
    /// indexes of its pages in section order. Empty for a section without
    /// dates.
    pub fn years_of(&self, section: &Section) -> Vec<(i32, Vec<usize>)> {
        let mut years: Vec<(i32, Vec<usize>)> = Vec::new();
        for &p in &section.pages {
            let Some(date) = &self.pages[p].date else {
                continue;
            };
            let year = chrono::Datelike::year(&date.datetime);
            match years.iter_mut().find(|(y, _)| *y == year) {
                Some((_, pages)) => pages.push(p),
                None => years.push((year, vec![p])),
            }
        }
        years.sort_by_key(|(y, _)| std::cmp::Reverse(*y));
        years
    }

    pub fn archive_template(&self, section: &Section) -> String {
        section
            .front
            .archive_template
            .clone()
            .unwrap_or_else(|| "archive.html".to_owned())
    }

    pub fn section_template(&self, section: &Section) -> String {
        section.front.template.clone().unwrap_or_else(|| {
            if section.is_root() {
                "index.html".to_owned()
            } else {
                "section.html".to_owned()
            }
        })
    }

    /// Whether a section emits its own feed.
    pub fn section_feeds(&self, section: &Section) -> bool {
        if section.is_root() {
            self.language_feeds(&section.lang)
        } else {
            section.front.generate_feeds.unwrap_or_default()
        }
    }

    pub fn language_feeds(&self, lang: &str) -> bool {
        if let Some(root) = self.sections.iter().find(|s| s.lang == lang && s.is_root())
            && let Some(generate) = root.front.generate_feeds
        {
            return generate;
        }
        self.config.language(lang).is_some_and(|l| l.generate_feeds)
    }

    /// The translation files of `lang` that exist, theme's then site's.
    pub fn i18n_sources(&self, lang: &str) -> Vec<String> {
        self.i18n
            .get(lang)
            .map(|t| t.paths.iter().map(|p| self.rel(p)).collect())
            .unwrap_or_default()
    }

    fn text_extra_sources(&self, text: &str, lang: &str) -> Vec<String> {
        let mut out: Vec<String> = load_data_paths(text)
            .into_iter()
            .filter(|p| self.root.join(p).is_file())
            .collect();
        if let Some(git) = &self.git
            && crate::git::reads_git(text)
        {
            out.extend(git.sources.iter().cloned());
        }
        for c in content_url_paths(text) {
            if let Some((_, rel)) = self.resolve_content_path(&c, lang)
                && !rel.is_empty()
            {
                out.push(format!("{}/{rel}", self.config.content_dir));
            }
        }
        out
    }

    pub fn template_sources(
        &self,
        name: &str,
        lang: &str,
    ) -> (BTreeSet<String>, bool, Vec<String>) {
        let closure = self.templates.closure(name);
        let mut cross = false;
        let mut data = Vec::new();
        let mut sources = BTreeSet::new();
        for template in &closure {
            let text = &self.templates.files[template];
            cross |= reads_other_pages(text);
            data.extend(self.text_extra_sources(text, lang));
            sources.insert(self.templates.origins[template].clone());
        }
        (sources, cross, data)
    }

    pub fn body_template_sources(
        &self,
        body: &str,
        lang: &str,
    ) -> (BTreeSet<String>, bool, Vec<String>) {
        let mut cross = reads_other_pages(body);
        let mut data = self.text_extra_sources(body, lang);
        let mut sources = BTreeSet::new();
        for name in template_references(body) {
            let (files, t_cross, t_data) = self.template_sources(&name, lang);
            sources.extend(files);
            cross |= t_cross;
            data.extend(t_data);
        }
        (sources, cross, data)
    }

    pub fn template_reads_translations(&self, name: &str) -> bool {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| {
            regex::Regex::new(r"\b(?:translations|alternates)\b").expect("valid regex")
        });
        self.templates
            .closure(name)
            .into_iter()
            .any(|t| re.is_match(&self.templates.files[&t]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_lang_removes_only_the_suffix() {
        assert_eq!(strip_lang("blog/post.en.md", "en"), "blog/post.md");
        assert_eq!(strip_lang("blog/_index.he.md", "he"), "blog/_index.md");
        assert_eq!(strip_lang("blog/post.md", "en"), "blog/post.md");
    }

    #[test]
    fn template_references_found() {
        let refs = template_references(
            r#"{% extends "base.html" %} {%- include "_date.html" -%} {% import "macros.html" as m %}"#,
        );
        assert_eq!(refs, vec!["base.html", "_date.html", "macros.html"]);
    }

    #[test]
    fn cross_page_detection() {
        assert!(reads_other_pages("{{ get_section(path=\"x\") }}"));
        assert!(reads_other_pages("{% for p in section.pages %}"));
        assert!(!reads_other_pages(
            "{{ page.title }} {{ page.content | safe }}"
        ));
    }

    #[test]
    fn every_cross_page_construct_is_detected() {
        for text in [
            "{{ get_page(path=\"a.md\") }}",
            "{{ get_taxonomy(name=\"tags\") }}",
            "{{ get_taxonomy_url(name=\"tags\", term=\"x\") }}",
            "{% for s in section.subsections %}",
            "{% if page.previous %}",
            "{{ page.next.title }}",
            "{{ term.pages | length }}",
        ] {
            assert!(reads_other_pages(text), "{text}");
        }
        for text in [
            "{{ page.pages_note }}",
            "{{ pagesize }}",
            "{{ previous_label }}",
            "{{ trans(key=\"next\") }}",
            "",
        ] {
            assert!(!reads_other_pages(text), "{text}");
        }
    }

    #[test]
    fn load_data_paths_are_the_literal_ones() {
        let found = load_data_paths(
            "{{ load_data(path=\"out/build_info.toml\") }} {% set x = load_data( required=false, path = \"data/a.json\") %} {{ load_data(path=dynamic) }}",
        );
        assert_eq!(found, vec!["out/build_info.toml", "data/a.json"]);
        assert!(load_data_paths("{{ page.title }}").is_empty());
        let urls = content_url_paths(
            "{{ get_url(lang=\"he\", path=\"@/blog/post.md#top\") }} {{ get_url(path=\"style.css\") }}",
        );
        assert_eq!(urls, vec!["blog/post.md"]);
    }

    #[test]
    fn template_references_tolerate_whitespace_control_and_imports() {
        let refs = template_references(
            "{%- extends \"base.html\" -%}\n{%include \"a.html\"%}\n{%   import   \"m.html\" as m %}\n{% include 'single.html' %}",
        );
        assert_eq!(
            refs,
            vec!["base.html", "a.html", "m.html"],
            "single quotes are not tracked"
        );
    }

    fn templates(files: &[(&str, &str)]) -> Templates {
        Templates {
            files: files
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            origins: files
                .iter()
                .map(|(k, _)| ((*k).to_owned(), format!("templates/{k}")))
                .collect(),
        }
    }

    #[test]
    fn the_closure_follows_references_and_survives_cycles() {
        let t = templates(&[
            (
                "page.html",
                "{% extends \"base.html\" %}{% include \"_x.html\" %}",
            ),
            (
                "base.html",
                "{% include \"_nav.html\" %}{% include \"missing.html\" %}",
            ),
            ("_nav.html", "{% include \"base.html\" %}"),
            ("_x.html", "x"),
            ("other.html", "unrelated"),
        ]);
        let closure = t.closure("page.html");
        let mut names: Vec<&str> = closure.iter().map(String::as_str).collect();
        names.sort();
        assert_eq!(names, ["_nav.html", "_x.html", "base.html", "page.html"]);
        assert!(
            t.closure("missing.html").is_empty(),
            "an unknown root has no closure"
        );
        assert_eq!(t.closure("_x.html").len(), 1);
        assert!(t.has("page.html") && !t.has("missing.html"));
    }

    #[test]
    fn default_templates_are_default() {
        let t = Templates::default();
        assert!(t.files.is_empty());
        assert!(!t.has("page.html"));
        assert!(t.closure("page.html").is_empty());
    }

    #[test]
    fn strip_lang_takes_the_last_suffix_only() {
        assert_eq!(strip_lang("en/notes.en.md", "en"), "en/notes.md");
        assert_eq!(strip_lang("a.en.b.en.md", "en"), "a.en.b.md");
        assert_eq!(strip_lang("x.he.md", "en"), "x.he.md");
        assert_eq!(
            strip_lang("dir.en.name/post.md", "en"),
            "dir.en.name/post.md"
        );
        assert_eq!(strip_lang("blog/v1.en.2.md", "en"), "blog/v1.en.2.md");
    }
}
