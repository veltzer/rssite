//! The plan: every file the build writes, each with the inputs whose change
//! can change it. `rssite plan` prints it; `rssite build` runs it. One
//! function, two commands.

use crate::site::Site;
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Kind {
    Page(usize),
    Section {
        section: usize,
        pager: Option<usize>,
    },
    TaxonomyList(usize),
    Term {
        taxonomy: usize,
        term: usize,
        pager: Option<usize>,
    },
    /// A section's listing of the pages of one year.
    YearArchive {
        section: usize,
        year: i32,
    },
    LanguageFeed(String),
    SearchIndex(String),
    SectionFeed(usize),
    TermFeed {
        taxonomy: usize,
        term: usize,
    },
    Sitemap,
    Robots,
    Root,
    NotFound(String),
    Static(PathBuf),
    Sass(PathBuf),
    Asset(PathBuf),
    /// A resized or re-encoded image, by its output path in `Site::images`.
    Image(String),
    Alias {
        target: String,
    },
    HighlightCss,
}

#[derive(Clone, Debug)]
pub struct Output {
    /// Relative to the output directory, `/` separated.
    pub path: String,
    /// Relative to the site root.
    pub sources: BTreeSet<String>,
    pub kind: Kind,
}

#[derive(Serialize)]
struct ManifestEntry<'a> {
    path: String,
    sources: &'a BTreeSet<String>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    version: u32,
    outputs: Vec<ManifestEntry<'a>>,
}

pub struct Plan {
    pub outputs: Vec<Output>,
}

impl Plan {
    pub fn manifest(&self, output_dir: &str) -> serde_json::Value {
        let manifest = Manifest {
            version: 1,
            outputs: self
                .outputs
                .iter()
                .map(|o| ManifestEntry {
                    path: format!("{output_dir}/{}", o.path),
                    sources: &o.sources,
                })
                .collect(),
        };
        serde_json::to_value(manifest).expect("manifest serializes")
    }

    pub fn paths(&self) -> BTreeSet<String> {
        self.outputs.iter().map(|o| o.path.clone()).collect()
    }

    pub fn compute(site: &Site) -> Result<Self> {
        let mut planner = Planner {
            site,
            outputs: Vec::new(),
            config_file: crate::config::CONFIG_FILE.to_owned(),
            all_content: site.content_files.iter().cloned().collect(),
        };
        planner.plan_pages();
        planner.plan_sections()?;
        planner.plan_languages()?;
        planner.plan_taxonomies();
        let local_targets = planner.plan_redirects()?;
        planner.plan_site_files();
        planner.plan_static_files();
        planner.plan_images();
        planner.check(&local_targets)?;
        Ok(Self {
            outputs: planner.outputs,
        })
    }
}

/// The state `Plan::compute` builds up: the outputs so far and what most of
/// them depend on.
struct Planner<'a> {
    site: &'a Site,
    outputs: Vec<Output>,
    config_file: String,
    all_content: BTreeSet<String>,
}

impl Planner<'_> {
    /// Every content file of one language.
    fn content_of_lang(&self, lang: &str) -> BTreeSet<String> {
        let site = self.site;
        let mut set = BTreeSet::new();
        for page in site.pages.iter().filter(|p| p.file.lang == lang) {
            set.insert(site.rel(&page.file.path));
        }
        for section in site.sections.iter().filter(|s| s.lang == lang) {
            if let Some(file) = &section.file {
                set.insert(site.rel(&file.path));
            }
        }
        set
    }

    /// What every output of one language depends on.
    fn common(&self, lang: &str) -> BTreeSet<String> {
        let mut set = BTreeSet::new();
        set.insert(self.config_file.clone());
        set.extend(self.site.i18n_sources(lang));
        set
    }

    /// `sources` plus what rendering `template` in `lang` reads.
    fn with_template(
        &self,
        mut sources: BTreeSet<String>,
        template: &str,
        lang: &str,
    ) -> BTreeSet<String> {
        let (files, cross, data) = self.site.template_sources(template, lang);
        sources.extend(files);
        sources.extend(data);
        sources.extend(self.common(lang));
        if cross {
            sources.extend(self.all_content.iter().cloned());
        }
        sources
    }

    /// Pages and their aliases.
    fn plan_pages(&mut self) {
        let site = self.site;
        for (i, page) in site.pages.iter().enumerate() {
            let mut sources = BTreeSet::new();
            sources.insert(site.rel(&page.file.path));
            for &t in &page.translations {
                sources.insert(site.rel(&site.pages[t].file.path));
            }
            if let Some(source) = page.body_source {
                sources.insert(site.rel(&site.pages[source].file.path));
            }
            sources.extend(page.link_sources.iter().cloned());
            sources.extend(page.cascade_sources.iter().cloned());
            if let Some(file) = &site.sections[page.section].file {
                sources.insert(site.rel(&file.path));
            }
            for call in &page.rendered.shortcodes {
                let (files, cross, data) = site.template_sources(&call.template(), &page.file.lang);
                sources.extend(files);
                sources.extend(data);
                if cross {
                    sources.extend(self.all_content.iter().cloned());
                }
            }
            let body_file = &site.pages[page.body_source.unwrap_or(i)].file;
            if body_file.body_kind == crate::content::BodyKind::Tera {
                let (files, cross, data) =
                    site.body_template_sources(&body_file.body, &page.file.lang);
                sources.extend(files);
                sources.extend(data);
                if cross {
                    sources.extend(self.all_content.iter().cloned());
                }
            }
            let sources = self.with_template(sources, &site.page_template(page), &page.file.lang);
            self.outputs.push(Output {
                path: format!("{}index.html", strip_root(&page.path)),
                sources: sources.clone(),
                kind: Kind::Page(i),
            });
            for alias in &page.front.aliases {
                let mut alias_sources = BTreeSet::new();
                alias_sources.insert(site.rel(&page.file.path));
                alias_sources.insert(self.config_file.clone());
                self.outputs.push(Output {
                    path: alias_path(alias),
                    sources: alias_sources,
                    kind: Kind::Alias {
                        target: page.permalink.clone(),
                    },
                });
            }
        }
    }

    /// Sections, their pagers, feeds and aliases.
    fn plan_sections(&mut self) -> Result<()> {
        let site = self.site;
        for (i, section) in site.sections.iter().enumerate() {
            let sources = self.section_sources(i, section);
            let pagers = pager_count(section.pages.len(), section.front.paginate_by);
            let base = strip_root(&section.path);
            if pagers.is_none() {
                self.outputs.push(Output {
                    path: format!("{base}index.html"),
                    sources: sources.clone(),
                    kind: Kind::Section {
                        section: i,
                        pager: None,
                    },
                });
            } else {
                for n in 1..=pagers.unwrap_or(1) {
                    let path = if n == 1 {
                        format!("{base}index.html")
                    } else {
                        format!("{base}{}/{n}/index.html", section.front.paginate_path)
                    };
                    self.outputs.push(Output {
                        path,
                        sources: sources.clone(),
                        kind: Kind::Section {
                            section: i,
                            pager: Some(n),
                        },
                    });
                }
            }
            if let Some(pattern) = &section.front.pager_aliases {
                self.plan_pager_aliases(section, pattern, pagers)?;
            }
            if section.front.year_archives {
                // A year's listing depends on every content file of the
                // language, like the section listing: any post can move
                // between years or be added. When the archive template
                // reads `translations` or `alternates`, it also depends on
                // the translated sections and their languages' content.
                let archive_template = section.archive_template();
                let mut archive_content = self.content_of_lang(&section.lang);
                if site.template_reads_translations(&archive_template) {
                    for &t in &section.translations {
                        archive_content.extend(self.content_of_lang(&site.sections[t].lang));
                    }
                }
                let archive_sources =
                    self.with_template(archive_content, &archive_template, &section.lang);
                for (year, _) in site.years_of(section) {
                    self.outputs.push(Output {
                        path: format!("{base}{year}/index.html"),
                        sources: archive_sources.clone(),
                        kind: Kind::YearArchive { section: i, year },
                    });
                }
            }
            if site.section_feeds(section) && !section.is_root() {
                let feed_sources = self.with_template(
                    self.content_of_lang(&section.lang),
                    "atom.xml",
                    &section.lang,
                );
                self.outputs.push(Output {
                    path: format!("{base}{}", site.config.feeds.filename),
                    sources: feed_sources,
                    kind: Kind::SectionFeed(i),
                });
            }
            for alias in &section.front.aliases {
                let mut alias_sources = BTreeSet::new();
                if let Some(file) = &section.file {
                    alias_sources.insert(site.rel(&file.path));
                }
                alias_sources.insert(self.config_file.clone());
                self.outputs.push(Output {
                    path: alias_path(alias),
                    sources: alias_sources,
                    kind: Kind::Alias {
                        target: section.permalink.clone(),
                    },
                });
            }
        }
        Ok(())
    }

    /// Language feeds, search indexes and 404 pages.
    fn plan_languages(&mut self) -> Result<()> {
        let site = self.site;
        for language in &site.config.languages {
            let lang = &language.code;
            if site.language_feeds(lang) {
                let sources = self.with_template(self.content_of_lang(lang), "atom.xml", lang);
                let path = format!(
                    "{}{}",
                    strip_root(language.prefix()),
                    site.config.feeds.filename
                );
                // The fallback language's feed, again at the site root.
                if site.config.feeds.root
                    && *lang == site.config.fallback()
                    && path != site.config.feeds.filename
                {
                    self.outputs.push(Output {
                        path: site.config.feeds.filename.clone(),
                        sources: sources.clone(),
                        kind: Kind::LanguageFeed(lang.clone()),
                    });
                }
                self.outputs.push(Output {
                    path,
                    sources,
                    kind: Kind::LanguageFeed(lang.clone()),
                });
            } else if site.config.feeds.root && *lang == site.config.fallback() {
                bail!(
                    "{}: feeds.root is set but the fallback language {lang:?} has no feed",
                    crate::config::CONFIG_FILE
                );
            }
            if site.config.search.enabled {
                let mut sources = self.content_of_lang(lang);
                sources.insert(self.config_file.clone());
                self.outputs.push(Output {
                    path: format!(
                        "{}{}",
                        strip_root(language.prefix()),
                        site.config.search.filename
                    ),
                    sources,
                    kind: Kind::SearchIndex(lang.clone()),
                });
            }
            let sources = self.with_template(BTreeSet::new(), "404.html", lang);
            self.outputs.push(Output {
                path: format!("{}404.html", strip_root(language.prefix())),
                sources,
                kind: Kind::NotFound(lang.clone()),
            });
        }
        let fallback = site.config.fallback().to_owned();
        let has_root_lang = site.config.languages.iter().any(|l| l.prefix() == "/");
        if !has_root_lang {
            self.outputs.push(Output {
                path: "404.html".to_owned(),
                sources: self.with_template(BTreeSet::new(), "404.html", &fallback),
                kind: Kind::NotFound(fallback.clone()),
            });
        }
        Ok(())
    }

    /// Taxonomy lists, terms, pagers and feeds.
    fn plan_taxonomies(&mut self) {
        let site = self.site;
        for (t, taxonomy) in site.taxonomies.iter().enumerate() {
            let sources = self.with_template(
                self.content_of_lang(&taxonomy.lang),
                "taxonomy_list.html",
                &taxonomy.lang,
            );
            let mut sources = sources;
            if let Some(file) = &taxonomy.translations_file {
                sources.insert(file.clone());
            }
            // Inferred pairings come from the pages of every language.
            if site
                .config
                .taxonomies
                .iter()
                .any(|c| c.name == taxonomy.name && c.infer_translations)
            {
                sources.extend(self.all_content.iter().cloned());
            }
            self.outputs.push(Output {
                path: format!("{}index.html", strip_root(&taxonomy.path)),
                sources: sources.clone(),
                kind: Kind::TaxonomyList(t),
            });
            let term_sources =
                self.with_template(sources.clone(), "taxonomy_single.html", &taxonomy.lang);
            for (term_index, term) in taxonomy.terms.iter().enumerate() {
                let base = strip_root(&term.path);
                match pager_count(term.pages.len(), taxonomy.paginate_by) {
                    None => self.outputs.push(Output {
                        path: format!("{base}index.html"),
                        sources: term_sources.clone(),
                        kind: Kind::Term {
                            taxonomy: t,
                            term: term_index,
                            pager: None,
                        },
                    }),
                    Some(pagers) => {
                        for n in 1..=pagers {
                            let path = if n == 1 {
                                format!("{base}index.html")
                            } else {
                                format!("{base}{}/{n}/index.html", taxonomy.paginate_path)
                            };
                            self.outputs.push(Output {
                                path,
                                sources: term_sources.clone(),
                                kind: Kind::Term {
                                    taxonomy: t,
                                    term: term_index,
                                    pager: Some(n),
                                },
                            });
                        }
                    }
                }
                if taxonomy.feed {
                    let feed_sources =
                        self.with_template(sources.clone(), "atom.xml", &taxonomy.lang);
                    self.outputs.push(Output {
                        path: format!("{base}{}", site.config.feeds.filename),
                        sources: feed_sources,
                        kind: Kind::TermFeed {
                            taxonomy: t,
                            term: term_index,
                        },
                    });
                }
            }
        }
    }

    /// Redirects declared in the configuration. A site-path target is
    /// returned, to be checked once every output is known.
    fn plan_redirects(&mut self) -> Result<Vec<(String, String)>> {
        let site = self.site;
        let fallback = site.config.fallback().to_owned();
        let mut local_targets: Vec<(String, String)> = Vec::new();
        for redirect in &site.config.redirects {
            let mut sources = BTreeSet::new();
            sources.insert(self.config_file.clone());
            let target = if let Some(content) = redirect.to.strip_prefix("@/") {
                let (content_path, fragment) = match content.split_once('#') {
                    Some((p, f)) => (p, Some(f)),
                    None => (content, None),
                };
                let Some((permalink, rel)) = site.resolve_content_path(content_path, &fallback)
                else {
                    bail!(
                        "{}: [[redirects]] from = {:?}: no content file matches {:?}",
                        crate::config::CONFIG_FILE,
                        redirect.from,
                        redirect.to
                    );
                };
                if !rel.is_empty() {
                    sources.insert(format!("{}/{rel}", site.config.content_dir));
                }
                match fragment {
                    Some(f) => format!("{permalink}#{f}"),
                    None => permalink,
                }
            } else if redirect.to.starts_with('/') {
                local_targets.push((redirect.from.clone(), redirect.to.clone()));
                site.permalink(&redirect.to)
            } else {
                redirect.to.clone()
            };
            self.outputs.push(Output {
                path: alias_path(&redirect.from),
                sources,
                kind: Kind::Alias { target },
            });
        }
        Ok(local_targets)
    }

    /// Sitemap, robots.txt, the root redirect and the highlight stylesheet.
    fn plan_site_files(&mut self) {
        let site = self.site;
        let fallback = site.config.fallback().to_owned();
        if site.config.sitemap.enabled {
            let mut sources = self.all_content.clone();
            for taxonomy in &site.taxonomies {
                if let Some(file) = &taxonomy.translations_file {
                    sources.insert(file.clone());
                }
            }
            let sources = self.with_template(sources, "sitemap.xml", &fallback);
            self.outputs.push(Output {
                path: "sitemap.xml".to_owned(),
                sources,
                kind: Kind::Sitemap,
            });
        }
        let has_static_robots = site.static_files.iter().any(|f| f.rel == "robots.txt");
        if !has_static_robots {
            let mut sources = BTreeSet::new();
            sources.insert(self.config_file.clone());
            self.outputs.push(Output {
                path: "robots.txt".to_owned(),
                sources,
                kind: Kind::Robots,
            });
        }
        if !self.has_root_lang() {
            let mut sources = BTreeSet::new();
            sources.insert(self.config_file.clone());
            let sources = if site.templates.has("root.html") {
                self.with_template(sources, "root.html", &fallback)
            } else {
                sources
            };
            self.outputs.push(Output {
                path: "index.html".to_owned(),
                sources,
                kind: Kind::Root,
            });
        }
        if site.config.markdown.highlight.enabled && site.config.markdown.highlight.css {
            let mut sources = BTreeSet::new();
            sources.insert(self.config_file.clone());
            self.outputs.push(Output {
                path: "highlight.css".to_owned(),
                sources,
                kind: Kind::HighlightCss,
            });
        }
    }

    /// Static files (the site's and the theme's), published directories, Sass, content assets.
    fn plan_static_files(&mut self) {
        let site = self.site;
        // Sass, content assets.
        for file in &site.static_files {
            let mut sources = BTreeSet::new();
            sources.insert(site.rel(&file.path));
            self.outputs.push(Output {
                path: file.rel.clone(),
                sources,
                kind: Kind::Static(file.path.clone()),
            });
        }
        for file in &site.published {
            let mut sources = BTreeSet::new();
            sources.insert(file.source.clone());
            self.outputs.push(Output {
                path: file.out.clone(),
                sources,
                kind: Kind::Static(file.path.clone()),
            });
        }
        let sass_sources: BTreeSet<String> =
            site.sass_files.iter().map(|f| site.rel(&f.path)).collect();
        for file in &site.sass_files {
            let Some(css) = crate::site::sass_output(&file.rel) else {
                continue;
            };
            self.outputs.push(Output {
                path: css,
                sources: sass_sources.clone(),
                kind: Kind::Sass(file.path.clone()),
            });
        }
        for asset in &site.assets {
            for language in &site.config.languages {
                // A draft section takes its assets with it, like its pages.
                if site.under_drafted_section(&asset.components, &language.code) {
                    continue;
                }
                let Ok(dir) = site_section_path(site, &language.code, &asset.components) else {
                    continue;
                };
                let mut sources = BTreeSet::new();
                sources.insert(site.rel(&asset.path));
                self.outputs.push(Output {
                    path: format!("{}{}", strip_root(&dir), asset.name),
                    sources,
                    kind: Kind::Asset(asset.path.clone()),
                });
            }
        }
    }

    /// Image variants: each depends on its image and the [images] settings, nothing else.
    fn plan_images(&mut self) {
        let site = self.site;
        // settings, nothing else.
        for (path, variant) in &site.images {
            let mut sources = BTreeSet::new();
            sources.insert(site.rel(&variant.source));
            sources.insert(self.config_file.clone());
            self.outputs.push(Output {
                path: path.clone(),
                sources,
                kind: Kind::Image(path.clone()),
            });
        }
    }

    /// Every output has one path, and no path is both a file and a directory.
    fn check(&mut self, local_targets: &[(String, String)]) -> Result<()> {
        let site = self.site;
        self.outputs.sort_by(|a, b| a.path.cmp(&b.path));
        for (from, to) in local_targets {
            let path = to.split(['#', '?']).next().unwrap_or(to);
            let file = if path.ends_with('/') {
                format!("{}index.html", strip_root(path))
            } else {
                strip_root(path)
            };
            let found = |p: &str| {
                self.outputs
                    .binary_search_by(|o| o.path.as_str().cmp(p))
                    .is_ok()
            };
            if !found(&file) && !found(&format!("{file}/index.html")) {
                bail!(
                    "{}: [[redirects]] from = {from:?} points at {to:?}, which this build does not write",
                    crate::config::CONFIG_FILE
                );
            }
        }
        for pair in self.outputs.windows(2) {
            if pair[0].path == pair[1].path {
                bail!(
                    "two outputs would be written to {}: {} and {}",
                    pair[0].path,
                    describe(site, &pair[0].kind),
                    describe(site, &pair[1].kind)
                );
            }
        }
        // A path cannot be a file and a directory: the alias `/v1.2` next
        // to the page `/v1.2/` would fail in the middle of the writes.
        let by_path: std::collections::BTreeMap<&str, &Output> =
            self.outputs.iter().map(|o| (o.path.as_str(), o)).collect();
        for output in &self.outputs {
            let mut dir = output.path.as_str();
            while let Some((parent, _)) = dir.rsplit_once('/') {
                if let Some(file) = by_path.get(parent) {
                    bail!(
                        "{parent} would be both a file ({}) and a directory holding {} ({})",
                        describe(site, &file.kind),
                        output.path,
                        describe(site, &output.kind)
                    );
                }
                dir = parent;
            }
        }
        Ok(())
    }

    /// What a section's listing depends on: the content of its language, its
    /// translations and body, whatever its shortcodes and Tera body read, and
    /// its template.
    fn section_sources(&self, i: usize, section: &crate::site::Section) -> BTreeSet<String> {
        let site = self.site;
        let mut sources = self.content_of_lang(&section.lang);
        for &t in &section.translations {
            if let Some(file) = &site.sections[t].file {
                sources.insert(site.rel(&file.path));
            }
        }
        if let Some(source) = section.body_source
            && let Some(file) = &site.sections[source].file
        {
            sources.insert(site.rel(&file.path));
        }
        sources.extend(section.link_sources.iter().cloned());
        for call in &section.rendered.shortcodes {
            let (files, cross, data) = site.template_sources(&call.template(), &section.lang);
            sources.extend(files);
            sources.extend(data);
            if cross {
                sources.extend(self.all_content.iter().cloned());
            }
        }
        if let Some(body_file) = &site.sections[section.body_source.unwrap_or(i)].file
            && body_file.body_kind == crate::content::BodyKind::Tera
        {
            let (files, cross, data) = site.body_template_sources(&body_file.body, &section.lang);
            sources.extend(files);
            sources.extend(data);
            if cross {
                sources.extend(self.all_content.iter().cloned());
            }
        }
        self.with_template(sources, &section.section_template(), &section.lang)
    }

    /// The redirects a section's `pager_aliases` pattern asks for.
    fn plan_pager_aliases(
        &mut self,
        section: &crate::site::Section,
        pattern: &str,
        pagers: Option<usize>,
    ) -> Result<()> {
        let site = self.site;
        let rel = section.rel().unwrap_or("(root)");
        // The pager-number placeholder, `{n}`, spelled through format! so it
        // is not a bare string literal that looks like a format argument.
        let placeholder = format!("{{{}}}", 'n');
        if !pattern.contains(&placeholder) {
            bail!("section {rel}: pager_aliases = {pattern:?} has no {placeholder}");
        }
        let Some(pagers) = pagers else {
            bail!("section {rel}: pager_aliases needs paginate_by");
        };
        let mut alias_sources = BTreeSet::new();
        if let Some(file) = &section.file {
            alias_sources.insert(site.rel(&file.path));
        }
        // The pager count depends on how many pages the section has.
        alias_sources.extend(self.content_of_lang(&section.lang));
        alias_sources.insert(self.config_file.clone());
        for n in 1..=pagers {
            let target = if n == 1 {
                section.permalink.clone()
            } else {
                format!("{}{}/{n}/", section.permalink, section.front.paginate_path)
            };
            self.outputs.push(Output {
                path: alias_path(&pattern.replace(&placeholder, &n.to_string())),
                sources: alias_sources.clone(),
                kind: Kind::Alias { target },
            });
        }
        Ok(())
    }

    fn has_root_lang(&self) -> bool {
        self.site.config.languages.iter().any(|l| l.prefix() == "/")
    }
}

fn site_section_path(site: &Site, lang: &str, components: &[String]) -> Result<String> {
    let mut path = site
        .config
        .language(lang)
        .map_or_else(|| format!("/{lang}/"), |l| l.prefix().to_owned());
    for component in components {
        let slug = crate::slug::slugify(component, site.config.slugify.paths);
        if slug.is_empty() {
            bail!("content directory {component:?} has no characters usable in a URL");
        }
        path.push_str(&slug);
        path.push('/');
    }
    Ok(path)
}

/// Pagers for `count` items, `None` when the listing is not paginated.
pub fn pager_count(count: usize, paginate_by: Option<usize>) -> Option<usize> {
    let by = paginate_by?;
    Some(count.div_ceil(by).max(1))
}

/// A site path (`/en/blog/`) as an output-relative directory (`en/blog/`).
pub fn strip_root(path: &str) -> String {
    path.trim_start_matches('/').to_owned()
}

/// The file an alias is written to: a directory index for a URL ending in
/// `/` or without an extension, the file itself otherwise.
pub fn alias_path(alias: &str) -> String {
    let trimmed = alias.trim_matches('/');
    if trimmed.is_empty() {
        return "index.html".to_owned();
    }
    let last = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if last.contains('.') && !alias.ends_with('/') {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/index.html")
    }
}

fn describe(site: &Site, kind: &Kind) -> String {
    match kind {
        Kind::Page(i) => format!("page {}", site.pages[*i].file.rel),
        Kind::Section { section, .. } => format!(
            "section {}",
            site.sections[*section].rel().unwrap_or("(root)")
        ),
        Kind::TaxonomyList(t) => format!(
            "taxonomy {} ({})",
            site.taxonomies[*t].name, site.taxonomies[*t].lang
        ),
        Kind::Term { taxonomy, term, .. } => format!(
            "term {:?} of {}",
            site.taxonomies[*taxonomy].terms[*term].name, site.taxonomies[*taxonomy].name
        ),
        Kind::YearArchive { section, year } => format!(
            "the {year} archive of section {}",
            site.sections[*section].rel().unwrap_or("(root)")
        ),
        Kind::LanguageFeed(l) => format!("the {l} feed"),
        Kind::SearchIndex(l) => format!("the {l} search index"),
        Kind::SectionFeed(s) => format!(
            "the feed of section {}",
            site.sections[*s].rel().unwrap_or("(root)")
        ),
        Kind::TermFeed { taxonomy, term } => format!(
            "the feed of term {:?}",
            site.taxonomies[*taxonomy].terms[*term].name
        ),
        Kind::Sitemap => "the sitemap".to_owned(),
        Kind::Robots => "the generated robots.txt".to_owned(),
        Kind::Root => "the root page".to_owned(),
        Kind::NotFound(l) => format!("the {l} 404 page"),
        Kind::Static(p) => format!("static file {}", site.rel(p)),
        Kind::Sass(p) => format!("stylesheet {}", site.rel(p)),
        Kind::Asset(p) => format!("content asset {}", site.rel(p)),
        Kind::Image(p) => format!(
            "a variant of image {}",
            site.images
                .get(p)
                .map(|v| site.rel(&v.source))
                .unwrap_or_default()
        ),
        Kind::Alias { target } => format!("an alias of {target}"),
        Kind::HighlightCss => "the highlighting stylesheet".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagers() {
        assert_eq!(pager_count(0, None), None);
        assert_eq!(pager_count(0, Some(10)), Some(1));
        assert_eq!(pager_count(10, Some(10)), Some(1));
        assert_eq!(pager_count(11, Some(10)), Some(2));
    }

    #[test]
    fn alias_paths() {
        assert_eq!(alias_path("/2026/01/01/old/"), "2026/01/01/old/index.html");
        assert_eq!(alias_path("/old"), "old/index.html");
        assert_eq!(alias_path("/keys/public.asc"), "keys/public.asc");
        assert_eq!(alias_path("/"), "index.html");
    }

    #[test]
    fn alias_paths_edge_cases() {
        assert_eq!(alias_path(""), "index.html");
        assert_eq!(alias_path("///"), "index.html");
        assert_eq!(
            alias_path("old"),
            "old/index.html",
            "no leading slash needed"
        );
        assert_eq!(alias_path("/old.html"), "old.html");
        assert_eq!(
            alias_path("/v1.2/"),
            "v1.2/index.html",
            "a dot in a directory name is still a directory when the alias ends in /"
        );
        assert_eq!(
            alias_path("/v1.2"),
            "v1.2",
            "...and a file when it does not"
        );
        assert_eq!(alias_path("/a/b.c/d/"), "a/b.c/d/index.html");
        assert_eq!(alias_path("/ישן/"), "ישן/index.html");
    }

    #[test]
    fn pagers_round_up_and_never_vanish() {
        assert_eq!(pager_count(1, Some(1)), Some(1));
        assert_eq!(pager_count(2, Some(1)), Some(2));
        assert_eq!(pager_count(9, Some(10)), Some(1));
        assert_eq!(pager_count(20, Some(10)), Some(2));
        assert_eq!(pager_count(21, Some(10)), Some(3));
        assert_eq!(pager_count(1000, None), None);
    }

    #[test]
    fn strip_root_drops_every_leading_slash() {
        assert_eq!(strip_root("/en/blog/"), "en/blog/");
        assert_eq!(strip_root("//x"), "x");
        assert_eq!(strip_root("/"), "");
        assert_eq!(strip_root(""), "");
        assert_eq!(strip_root("en/"), "en/");
    }

    #[test]
    fn the_manifest_prefixes_paths_with_the_output_directory() {
        let plan = Plan {
            outputs: vec![
                Output {
                    path: "en/index.html".into(),
                    sources: ["content/_index.en.md", "rssite.toml"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                    kind: Kind::Root,
                },
                Output {
                    path: "style.css".into(),
                    sources: BTreeSet::new(),
                    kind: Kind::Sass(PathBuf::from("/x/sass/style.scss")),
                },
            ],
        };
        let manifest = plan.manifest("public");
        assert_eq!(manifest["version"], 1);
        assert_eq!(manifest["outputs"][0]["path"], "public/en/index.html");
        assert_eq!(
            manifest["outputs"][0]["sources"],
            serde_json::json!(["content/_index.en.md", "rssite.toml"])
        );
        assert_eq!(manifest["outputs"][1]["path"], "public/style.css");
        assert_eq!(manifest["outputs"][1]["sources"], serde_json::json!([]));
        assert_eq!(
            plan.paths(),
            ["en/index.html", "style.css"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }
}
