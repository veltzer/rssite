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

    pub fn compute(site: &Site) -> Result<Plan> {
        let mut outputs: Vec<Output> = Vec::new();
        let config_file = crate::config::CONFIG_FILE.to_owned();
        let all_content: BTreeSet<String> = site.content_files.iter().cloned().collect();
        let content_of_lang = |lang: &str| -> BTreeSet<String> {
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
        };
        let common = |lang: &str| -> BTreeSet<String> {
            let mut set = BTreeSet::new();
            set.insert(config_file.clone());
            set.extend(site.i18n_sources(lang));
            set
        };
        let with_template =
            |mut sources: BTreeSet<String>, template: &str, lang: &str| -> BTreeSet<String> {
                let (files, cross, data) = site.template_sources(template);
                sources.extend(files);
                sources.extend(data);
                sources.extend(common(lang));
                if cross {
                    sources.extend(all_content.iter().cloned());
                }
                sources
            };

        // Pages and their aliases.
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
            if let Some(file) = &site.sections[page.section].file {
                sources.insert(site.rel(&file.path));
            }
            let body_file = &site.pages[page.body_source.unwrap_or(i)].file;
            if body_file.body_kind == crate::content::BodyKind::Tera {
                let (files, cross, data) = site.body_template_sources(&body_file.body);
                sources.extend(files);
                sources.extend(data);
                if cross {
                    sources.extend(all_content.iter().cloned());
                }
            }
            let sources = with_template(sources, &site.page_template(page), &page.file.lang);
            outputs.push(Output {
                path: format!("{}index.html", strip_root(&page.path)),
                sources: sources.clone(),
                kind: Kind::Page(i),
            });
            for alias in &page.front.aliases {
                let mut alias_sources = BTreeSet::new();
                alias_sources.insert(site.rel(&page.file.path));
                alias_sources.insert(config_file.clone());
                outputs.push(Output {
                    path: alias_path(alias),
                    sources: alias_sources,
                    kind: Kind::Alias {
                        target: page.permalink.clone(),
                    },
                });
            }
        }

        // Sections, their pagers, feeds and aliases.
        for (i, section) in site.sections.iter().enumerate() {
            let mut sources = content_of_lang(&section.lang);
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
            if let Some(body_file) = &site.sections[section.body_source.unwrap_or(i)].file
                && body_file.body_kind == crate::content::BodyKind::Tera
            {
                let (files, cross, data) = site.body_template_sources(&body_file.body);
                sources.extend(files);
                sources.extend(data);
                if cross {
                    sources.extend(all_content.iter().cloned());
                }
            }
            let sources = with_template(sources, &site.section_template(section), &section.lang);
            let pagers = pager_count(section.pages.len(), section.front.paginate_by);
            let base = strip_root(&section.path);
            if pagers.is_none() {
                outputs.push(Output {
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
                    outputs.push(Output {
                        path,
                        sources: sources.clone(),
                        kind: Kind::Section {
                            section: i,
                            pager: Some(n),
                        },
                    });
                }
            }
            if section.front.year_archives {
                // A year's listing depends on every content file of the
                // language, like the section listing: any post can move
                // between years or be added. When the archive template
                // reads `translations` or `alternates`, it also depends on
                // the translated sections and their languages' content.
                let archive_template = site.archive_template(section);
                let mut archive_content = content_of_lang(&section.lang);
                if site.template_reads_translations(&archive_template) {
                    for &t in &section.translations {
                        archive_content.extend(content_of_lang(&site.sections[t].lang));
                    }
                }
                let archive_sources =
                    with_template(archive_content, &archive_template, &section.lang);
                for (year, _) in site.years_of(section) {
                    outputs.push(Output {
                        path: format!("{base}{year}/index.html"),
                        sources: archive_sources.clone(),
                        kind: Kind::YearArchive { section: i, year },
                    });
                }
            }
            if site.section_feeds(section) && !section.is_root() {
                let feed_sources =
                    with_template(content_of_lang(&section.lang), "atom.xml", &section.lang);
                outputs.push(Output {
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
                alias_sources.insert(config_file.clone());
                outputs.push(Output {
                    path: alias_path(alias),
                    sources: alias_sources,
                    kind: Kind::Alias {
                        target: section.permalink.clone(),
                    },
                });
            }
        }

        // Language feeds and 404 pages.
        for language in &site.config.languages {
            let lang = &language.code;
            if site.language_feeds(lang) {
                let sources = with_template(content_of_lang(lang), "atom.xml", lang);
                outputs.push(Output {
                    path: format!(
                        "{}{}",
                        strip_root(language.prefix()),
                        site.config.feeds.filename
                    ),
                    sources,
                    kind: Kind::LanguageFeed(lang.clone()),
                });
            }
            let sources = with_template(BTreeSet::new(), "404.html", lang);
            outputs.push(Output {
                path: format!("{}404.html", strip_root(language.prefix())),
                sources,
                kind: Kind::NotFound(lang.clone()),
            });
        }
        let fallback = site.config.fallback().to_owned();
        outputs.push(Output {
            path: "404.html".to_owned(),
            sources: with_template(BTreeSet::new(), "404.html", &fallback),
            kind: Kind::NotFound(fallback.clone()),
        });

        // Taxonomies.
        for (t, taxonomy) in site.taxonomies.iter().enumerate() {
            let sources = with_template(
                content_of_lang(&taxonomy.lang),
                "taxonomy_list.html",
                &taxonomy.lang,
            );
            let mut sources = sources;
            if let Some(file) = &taxonomy.translations_file {
                sources.insert(file.clone());
            }
            outputs.push(Output {
                path: format!("{}index.html", strip_root(&taxonomy.path)),
                sources: sources.clone(),
                kind: Kind::TaxonomyList(t),
            });
            let term_sources =
                with_template(sources.clone(), "taxonomy_single.html", &taxonomy.lang);
            for (term_index, term) in taxonomy.terms.iter().enumerate() {
                let base = strip_root(&term.path);
                match pager_count(term.pages.len(), taxonomy.paginate_by) {
                    None => outputs.push(Output {
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
                            outputs.push(Output {
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
                    let feed_sources = with_template(sources.clone(), "atom.xml", &taxonomy.lang);
                    outputs.push(Output {
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

        // Site-wide files.
        if site.config.sitemap.enabled {
            let mut sources = all_content.clone();
            for taxonomy in &site.taxonomies {
                if let Some(file) = &taxonomy.translations_file {
                    sources.insert(file.clone());
                }
            }
            let sources = with_template(sources, "sitemap.xml", &fallback);
            outputs.push(Output {
                path: "sitemap.xml".to_owned(),
                sources,
                kind: Kind::Sitemap,
            });
        }
        let has_static_robots = site.static_files.iter().any(|f| f.rel == "robots.txt");
        if !has_static_robots {
            let mut sources = BTreeSet::new();
            sources.insert(config_file.clone());
            outputs.push(Output {
                path: "robots.txt".to_owned(),
                sources,
                kind: Kind::Robots,
            });
        }
        {
            let mut sources = BTreeSet::new();
            sources.insert(config_file.clone());
            let sources = if site.templates.has("root.html") {
                with_template(sources, "root.html", &fallback)
            } else {
                sources
            };
            outputs.push(Output {
                path: "index.html".to_owned(),
                sources,
                kind: Kind::Root,
            });
        }
        if site.config.markdown.highlight {
            let mut sources = BTreeSet::new();
            sources.insert(config_file.clone());
            outputs.push(Output {
                path: "highlight.css".to_owned(),
                sources,
                kind: Kind::HighlightCss,
            });
        }

        // Static files (the site's and the theme's), published directories,
        // Sass, content assets.
        for file in &site.static_files {
            let mut sources = BTreeSet::new();
            sources.insert(site.rel(&file.path));
            outputs.push(Output {
                path: file.rel.clone(),
                sources,
                kind: Kind::Static(file.path.clone()),
            });
        }
        for file in &site.published {
            let mut sources = BTreeSet::new();
            sources.insert(file.source.clone());
            outputs.push(Output {
                path: file.out.clone(),
                sources,
                kind: Kind::Static(file.path.clone()),
            });
        }
        let sass_sources: BTreeSet<String> =
            site.sass_files.iter().map(|f| site.rel(&f.path)).collect();
        for file in &site.sass_files {
            let rel = file.rel.clone();
            let path = &file.path;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let is_partial = rel.split('/').any(|part| part.starts_with('_'));
            let is_sass = name.ends_with(".scss") || name.ends_with(".sass");
            if is_partial || !is_sass {
                continue;
            }
            let css = format!(
                "{}.css",
                rel.rsplit_once('.').map(|(s, _)| s).unwrap_or(&rel)
            );
            outputs.push(Output {
                path: css,
                sources: sass_sources.clone(),
                kind: Kind::Sass(path.clone()),
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
                outputs.push(Output {
                    path: format!("{}{}", strip_root(&dir), asset.name),
                    sources,
                    kind: Kind::Asset(asset.path.clone()),
                });
            }
        }

        outputs.sort_by(|a, b| a.path.cmp(&b.path));
        for pair in outputs.windows(2) {
            if pair[0].path == pair[1].path {
                bail!(
                    "two outputs would be written to {}: {} and {}",
                    pair[0].path,
                    describe(site, &pair[0].kind),
                    describe(site, &pair[1].kind)
                );
            }
        }
        Ok(Plan { outputs })
    }
}

fn site_section_path(site: &Site, lang: &str, components: &[String]) -> Result<String> {
    let mut path = site
        .config
        .language(lang)
        .map(|l| l.prefix().to_owned())
        .unwrap_or_else(|| format!("/{lang}/"));
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
