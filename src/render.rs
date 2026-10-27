//! Producing every planned output: build (write), check (render without
//! writing), the determinism verification, the site query, and clean.

use crate::plan::{Kind, Output, Plan, pager_count};
use crate::site::Site;
use crate::templates::{self, Engine, Model};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

pub struct Report {
    pub outputs: usize,
    pub written: usize,
    pub unchanged: usize,
}

pub struct Options {
    pub print_plan: bool,
    pub verify_determinism: bool,
    pub write: bool,
}

struct Renderer<'a> {
    site: &'a Site,
    model: Arc<Model>,
    engines: BTreeMap<String, Engine>,
}

fn make_engines(site: &Site, model: &Arc<Model>) -> Result<BTreeMap<String, Engine>> {
    let mut engines = BTreeMap::new();
    for code in site.config.language_codes() {
        engines.insert(
            code.clone(),
            templates::engine(model, &site.templates, &code)?,
        );
    }
    Ok(engines)
}

fn base_context_from(site: &Site, model: &Model, lang: &str, current_path: &str) -> tera::Context {
    let mut context = tera::Context::new();
    context.insert("config", &model.config);
    context.insert("lang", lang);
    context.insert("language", &templates::language_view(site, lang));
    context.insert("languages", &model.languages);
    context.insert("current_path", current_path);
    context.insert(
        "current_url",
        &format!("{}{}", site.config.base_url, current_path),
    );
    // The generator itself, for a "made by" line. A build's output is a
    // function of its sources and the tool, and the tool's version is
    // part of that function, so naming it is deterministic.
    // The commit the site is built from, null outside git.
    context.insert("git", &site.git);
    // What the document says about itself in <meta name="description">;
    // pages and sections replace this language-wide default.
    context.insert(
        "meta_description",
        &templates::language_view(site, lang)["description"],
    );
    context.insert(
        "rssite",
        &json!({
            "name": "rssite",
            "version": env!("CARGO_PKG_VERSION"),
            "url": "https://github.com/veltzer/rssite",
        }),
    );
    context
}

/// Render every output of `plan` into memory.
fn render_all(site: &Site, plan: &Plan) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut model = Model::new(site, plan.paths());
    let has_tera_page = (0..site.pages.len()).any(|i| {
        site.pages[site.pages[i].body_source.unwrap_or(i)]
            .file
            .body_kind
            == crate::content::BodyKind::Tera
    });
    let has_tera_section = (0..site.sections.len()).any(|s| {
        site.sections[site.sections[s].body_source.unwrap_or(s)]
            .file
            .as_ref()
            .is_some_and(|f| f.body_kind == crate::content::BodyKind::Tera)
    });
    if has_tera_page || has_tera_section {
        let mut engines = make_engines(site, &Arc::new(model.clone()))?;
        if has_tera_page {
            for i in 0..site.pages.len() {
                let source = site.pages[i].body_source.unwrap_or(i);
                if site.pages[source].file.body_kind != crate::content::BodyKind::Tera {
                    continue;
                }
                let page = &site.pages[i];
                let lang = &page.file.lang;
                let body = &site.pages[source].file.body;
                let what = &page.file.rel;
                let mut context = base_context_from(site, &model, lang, &page.path);
                context.insert("page", &model.pages[i]);
                context.insert("section", &light(&model.sections[page.section]));
                context.insert("meta_description", &model.pages[i]["meta_description"]);
                insert_translations(&mut context, &model.pages[i]["translations"], site);
                let engine = engines.get_mut(lang).expect("engine per language");
                let html =
                    templates::render_str(engine, body, &context, what).with_context(|| {
                        format!(
                            "producing {}index.html",
                            crate::plan::strip_root(&page.path)
                        )
                    })?;
                let summary = html
                    .find(crate::markdown::SUMMARY_MARKER)
                    .map(|at| html[..at].trim_end().to_owned());
                let words = html.split_whitespace().count();
                model.pages[i]["content"] = Value::String(html);
                model.pages[i]["summary"] = match summary {
                    Some(s) => Value::String(s),
                    None => Value::Null,
                };
                model.pages[i]["word_count"] = json!(words);
                model.pages[i]["reading_time"] = json!(words.div_ceil(200).max(1));
                // The body only exists now, so the description derived from
                // it does too.
                if page.front.description.is_none() {
                    let text = crate::util::html_to_text(
                        model.pages[i]["summary"]
                            .as_str()
                            .or(model.pages[i]["content"].as_str())
                            .unwrap_or_default(),
                    );
                    if !text.is_empty() {
                        model.pages[i]["meta_description"] =
                            Value::String(crate::util::truncate_words(&text, 160));
                    }
                }
            }
            for (s, section) in site.sections.iter().enumerate() {
                model.sections[s]["pages"] = Value::Array(
                    section
                        .pages
                        .iter()
                        .map(|&p| model.pages[p].clone())
                        .collect(),
                );
            }
            for (t, taxonomy) in site.taxonomies.iter().enumerate() {
                for (term_idx, term) in taxonomy.terms.iter().enumerate() {
                    model.taxonomies[t]["terms"][term_idx]["pages"] =
                        Value::Array(term.pages.iter().map(|&p| model.pages[p].clone()).collect());
                }
            }
            engines = make_engines(site, &Arc::new(model.clone()))?;
        }
        if has_tera_section {
            for s in 0..site.sections.len() {
                let source = site.sections[s].body_source.unwrap_or(s);
                let Some(file) = &site.sections[source].file else {
                    continue;
                };
                if file.body_kind != crate::content::BodyKind::Tera {
                    continue;
                }
                let section = &site.sections[s];
                let lang = &section.lang;
                let body = &file.body;
                let what = section.rel().unwrap_or(&file.rel);
                let mut context = base_context_from(site, &model, lang, &section.path);
                context.insert("section", &model.sections[s]);
                context.insert("meta_description", &model.sections[s]["meta_description"]);
                insert_translations(&mut context, &model.sections[s]["translations"], site);
                let engine = engines.get_mut(lang).expect("engine per language");
                let html =
                    templates::render_str(engine, body, &context, what).with_context(|| {
                        format!(
                            "producing {}index.html",
                            crate::plan::strip_root(&section.path)
                        )
                    })?;
                let summary = html
                    .find(crate::markdown::SUMMARY_MARKER)
                    .map(|at| html[..at].trim_end().to_owned());
                let words = html.split_whitespace().count();
                model.sections[s]["content"] = Value::String(html);
                model.sections[s]["summary"] = match summary {
                    Some(s) => Value::String(s),
                    None => Value::Null,
                };
                model.sections[s]["word_count"] = json!(words);
            }
        }
    }
    let model = Arc::new(model);
    let engines = make_engines(site, &model)?;
    let mut renderer = Renderer {
        site,
        model,
        engines,
    };
    let mut files = BTreeMap::new();
    for output in &plan.outputs {
        let bytes = renderer
            .render(output)
            .with_context(|| format!("producing {}", output.path))?;
        files.insert(output.path.clone(), bytes);
    }
    Ok(files)
}

pub fn run(root: &Path, drafts: bool, options: &Options) -> Result<Report> {
    let site = Site::load(root, drafts)?;
    let plan = Plan::compute(&site)?;
    if options.print_plan {
        crate::util::emit(&serde_json::to_string_pretty(
            &plan.manifest(&site.config.output_dir),
        )?)?;
    }
    let files = render_all(&site, &plan)?;
    if options.verify_determinism {
        let again = render_all(&site, &plan)?;
        let differing: Vec<&String> = files
            .iter()
            .filter(|(path, bytes)| again.get(*path) != Some(bytes))
            .map(|(p, _)| p)
            .collect();
        if !differing.is_empty() {
            bail!(
                "the build is not deterministic: {} file(s) differ between two renders of the same sources: {}",
                differing.len(),
                differing
                    .iter()
                    .take(10)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    let mut report = Report {
        outputs: files.len(),
        written: 0,
        unchanged: 0,
    };
    if options.write {
        let output_dir = site.root.join(&site.config.output_dir);
        for (path, bytes) in &files {
            if crate::util::write_if_changed(&output_dir.join(path), bytes)? {
                report.written += 1;
            } else {
                report.unchanged += 1;
            }
        }
    }
    Ok(report)
}

pub fn print_plan(root: &Path, drafts: bool) -> Result<()> {
    let site = Site::load(root, drafts)?;
    let plan = Plan::compute(&site)?;
    crate::util::emit(&serde_json::to_string_pretty(
        &plan.manifest(&site.config.output_dir),
    )?)
}

/// The parsed site as JSON, for scripts that would otherwise re-parse it.
pub fn query(root: &Path, drafts: bool) -> Result<()> {
    let site = Site::load(root, drafts)?;
    let plan = Plan::compute(&site)?;
    let model = Model::new(&site, plan.paths());
    let strip = |mut value: Value| {
        if let Some(object) = value.as_object_mut() {
            object.remove("content");
            object.remove("summary");
            object.remove("toc");
        }
        value
    };
    let pages: Vec<Value> = model.pages.iter().cloned().map(strip).collect();
    let sections: Vec<Value> = model
        .sections
        .iter()
        .cloned()
        .map(|mut s| {
            s["pages"] = Value::Array(
                s["pages"].as_array().map(|p| p.iter().map(|x| json!({"relative_path": x["relative_path"], "permalink": x["permalink"]})).collect()).unwrap_or_default(),
            );
            strip(s)
        })
        .collect();
    let taxonomies: Vec<Value> = model
        .taxonomies
        .iter()
        .cloned()
        .map(|mut t| {
            if let Some(terms) = t["terms"].as_array_mut() {
                for term in terms {
                    term["pages"] = Value::Array(
                        term["pages"]
                            .as_array()
                            .map(|p| p.iter().map(|x| x["relative_path"].clone()).collect())
                            .unwrap_or_default(),
                    );
                }
            }
            t
        })
        .collect();
    let value = json!({
        "version": 1,
        "config": model.config,
        "languages": model.languages,
        "pages": pages,
        "sections": sections,
        "taxonomies": taxonomies,
        "outputs": plan.outputs.len(),
    });
    crate::util::emit(&serde_json::to_string_pretty(&value)?)?;
    Ok(())
}

/// Remove every planned output and the directories that empties.
pub fn clean(root: &Path, drafts: bool) -> Result<usize> {
    let site = Site::load(root, drafts)?;
    let plan = Plan::compute(&site)?;
    let output_dir = site.root.join(&site.config.output_dir);
    let mut removed = 0;
    let mut dirs: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    for output in &plan.outputs {
        let path = output_dir.join(&output.path);
        if path.is_file() {
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed += 1;
        }
        let mut parent = path.parent();
        while let Some(dir) = parent {
            if dir == output_dir || !dir.starts_with(&output_dir) {
                break;
            }
            dirs.insert(dir.to_path_buf());
            parent = dir.parent();
        }
    }
    // Deepest first, so a directory is tried after its children.
    for dir in dirs.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
    Ok(removed)
}

impl Renderer<'_> {
    fn engine(&mut self, lang: &str) -> &mut Engine {
        self.engines.get_mut(lang).expect("engine per language")
    }

    fn base_context(&self, lang: &str, current_path: &str) -> tera::Context {
        base_context_from(self.site, &self.model, lang, current_path)
    }

    fn render(&mut self, output: &Output) -> Result<Vec<u8>> {
        Ok(match &output.kind {
            Kind::Page(i) => self.render_page(*i)?.into_bytes(),
            Kind::Section { section, pager } => self.render_section(*section, *pager)?.into_bytes(),
            Kind::YearArchive { section, year } => {
                self.render_year_archive(*section, *year)?.into_bytes()
            }
            Kind::TaxonomyList(t) => self.render_taxonomy_list(*t)?.into_bytes(),
            Kind::Term {
                taxonomy,
                term,
                pager,
            } => self.render_term(*taxonomy, *term, *pager)?.into_bytes(),
            Kind::LanguageFeed(lang) => {
                let lang = lang.clone();
                let pages: Vec<usize> = (0..self.site.pages.len())
                    .filter(|&p| self.site.pages[p].file.lang == lang)
                    .collect();
                let root = self
                    .site
                    .sections
                    .iter()
                    .position(|s| s.lang == lang && s.is_root())
                    .expect("root section");
                let alternate = self.site.sections[root].permalink.clone();
                let title = self
                    .site
                    .config
                    .language(&lang)
                    .and_then(|l| l.title.clone())
                    .unwrap_or_else(|| self.site.config.title.clone());
                // The language's own feed path, also for the copy at the site
                // root (feeds.root): the two are the same feed, byte for
                // byte, and a reader following its self link moves to the
                // language's URL.
                let path = format!(
                    "{}{}",
                    crate::plan::strip_root(
                        self.site
                            .config
                            .language(&lang)
                            .map(|l| l.prefix())
                            .unwrap_or("/")
                    ),
                    self.site.config.feeds.filename
                );
                self.render_feed(&lang, pages, &path, &alternate, &title, None)?
                    .into_bytes()
            }
            Kind::SectionFeed(s) => {
                let section = &self.site.sections[*s];
                let lang = section.lang.clone();
                let title = format!("{} - {}", self.site.config.title, section.title());
                let alternate = section.permalink.clone();
                let pages = section.pages.clone();
                let view = self.model.sections[*s].clone();
                self.render_feed(
                    &lang,
                    pages,
                    &output.path,
                    &alternate,
                    &title,
                    Some(("section", view)),
                )?
                .into_bytes()
            }
            Kind::TermFeed { taxonomy, term } => {
                let model = &self.site.taxonomies[*taxonomy];
                let t = &model.terms[*term];
                let lang = model.lang.clone();
                let title = format!("{} - {}", self.site.config.title, t.name);
                let alternate = t.permalink.clone();
                let pages = t.pages.clone();
                let view = self.model.taxonomies[*taxonomy]["terms"][*term].clone();
                self.render_feed(
                    &lang,
                    pages,
                    &output.path,
                    &alternate,
                    &title,
                    Some(("term", view)),
                )?
                .into_bytes()
            }
            Kind::Sitemap => self.render_sitemap()?.into_bytes(),
            Kind::SearchIndex(lang) => self.render_search_index(lang)?.into_bytes(),
            Kind::Robots => {
                let mut robots = "User-agent: *\nAllow: /\n".to_owned();
                if self.site.config.sitemap.enabled {
                    robots.push_str(&format!(
                        "\nSitemap: {}/sitemap.xml\n",
                        self.site.config.base_url
                    ));
                }
                robots.into_bytes()
            }
            Kind::Root => {
                let fallback = self.site.config.fallback().to_owned();
                if self.site.templates.has("root.html") {
                    let mut context = self.base_context(&fallback, "/");
                    insert_translations(&mut context, &Value::Array(Vec::new()), self.site);
                    templates::render(self.engine(&fallback), "root.html", &context)?.into_bytes()
                } else {
                    let prefix = self
                        .site
                        .config
                        .language(&fallback)
                        .map(|l| l.prefix().to_owned())
                        .unwrap_or_default();
                    crate::builtin::redirect(&format!("{}{prefix}", self.site.config.base_url))
                        .into_bytes()
                }
            }
            Kind::NotFound(lang) => {
                let lang = lang.clone();
                let prefix = self
                    .site
                    .config
                    .language(&lang)
                    .map(|l| l.prefix().to_owned())
                    .unwrap_or_default();
                let mut context = self.base_context(&lang, &format!("{prefix}404.html"));
                insert_translations(&mut context, &Value::Array(Vec::new()), self.site);
                templates::render(self.engine(&lang), "404.html", &context)?.into_bytes()
            }
            Kind::Static(path) | Kind::Asset(path) => {
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?
            }
            Kind::Sass(path) => {
                // The site's sass directory first, then the theme's, so an
                // `@use "x"` finds the site's override before the theme's.
                let mut options = grass::Options::default()
                    .load_path(self.site.root.join(&self.site.config.sass_dir));
                if let Some(theme) = self.site.theme_dir() {
                    options = options.load_path(theme.join(&self.site.config.sass_dir));
                }
                grass::from_path(path, &options)
                    .map_err(|e| anyhow::anyhow!("compiling {}: {e}", self.site.rel(path)))?
                    .into_bytes()
            }
            Kind::Alias { target } => crate::builtin::redirect(target).into_bytes(),
            Kind::HighlightCss => {
                crate::markdown::highlight_css(&self.site.config.markdown.highlight_theme)?
                    .into_bytes()
            }
        })
    }

    fn render_page(&mut self, i: usize) -> Result<String> {
        let page = &self.site.pages[i];
        let lang = page.file.lang.clone();
        let template = self.site.page_template(page);
        let view = &self.model.pages[i];
        let mut context = self.base_context(&lang, &page.path);
        // Only the light view: the full section carries every page of the
        // section, and cloning that once per page is quadratic.
        let section = light(&self.model.sections[page.section]);
        context.insert("page", view);
        context.insert("section", &section);
        context.insert("meta_description", &view["meta_description"]);
        context.insert(
            "json_ld",
            &templates::page_json_ld(self.site, i, &view["meta_description"]),
        );
        insert_translations(&mut context, &view["translations"], self.site);
        templates::render(self.engine(&lang), &template, &context)
    }

    fn render_section(&mut self, s: usize, pager: Option<usize>) -> Result<String> {
        let section = &self.site.sections[s];
        let lang = section.lang.clone();
        let template = self.site.section_template(section);
        let view = &self.model.sections[s];
        let current_path = match pager {
            Some(n) if n > 1 => format!("{}{}/{n}/", section.path, section.front.paginate_path),
            _ => section.path.clone(),
        };
        let mut context = self.base_context(&lang, &current_path);
        if let Some(n) = pager {
            let paginator = paginator_view(
                &view["pages"],
                section.front.paginate_by.expect("paginated section"),
                n,
                &section.permalink,
                &section.front.paginate_path,
            );
            context.insert("paginator", &paginator);
        }
        context.insert("section", view);
        context.insert("meta_description", &view["meta_description"]);
        insert_translations(&mut context, &view["translations"], self.site);
        templates::render(self.engine(&lang), &template, &context)
    }

    /// The listing of a section's pages of one year, at `<section>/<year>/`.
    /// Its translations are the same year in the other languages the section
    /// exists in, where that language has pages in that year.
    fn render_year_archive(&mut self, s: usize, year: i32) -> Result<String> {
        let section = &self.site.sections[s];
        let lang = section.lang.clone();
        let template = self.site.archive_template(section);
        let view = self.model.sections[s].clone();
        let current_path = format!("{}{year}/", section.path);
        let pages: Vec<Value> = view["pages"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p["year"].as_i64() == Some(i64::from(year)))
            .collect();
        let translations: Vec<Value> = section
            .translations
            .iter()
            .filter_map(|&t| {
                let other = &self.site.sections[t];
                let has_year = other.front.year_archives
                    && self.site.years_of(other).iter().any(|(y, _)| *y == year);
                has_year.then(|| {
                    json!({
                        "lang": other.lang,
                        "permalink": format!("{}{year}/", other.permalink),
                        "path": format!("{}{year}/", other.path),
                        "title": other.title(),
                    })
                })
            })
            .collect();
        let translations = Value::Array(translations);
        let mut context = self.base_context(&lang, &current_path);
        context.insert("section", &light(&view));
        context.insert("meta_description", &view["meta_description"]);
        context.insert("year", &year);
        context.insert("pages", &pages);
        context.insert("years", &view["years"]);
        insert_translations(&mut context, &translations, self.site);
        templates::render(self.engine(&lang), &template, &context)
    }

    fn render_taxonomy_list(&mut self, t: usize) -> Result<String> {
        let taxonomy = &self.site.taxonomies[t];
        let lang = taxonomy.lang.clone();
        let view = self.model.taxonomies[t].clone();
        let mut context = self.base_context(&lang, &taxonomy.path);
        context.insert("taxonomy", &view);
        context.insert("terms", &view["terms"]);
        let translations: Vec<Value> = self
            .site
            .taxonomies
            .iter()
            .filter(|x| x.name == taxonomy.name)
            .map(|x| json!({"lang": x.lang, "permalink": x.permalink, "path": x.path, "title": x.name}))
            .collect();
        let translations = Value::Array(translations);
        insert_translations(&mut context, &translations, self.site);
        templates::render(self.engine(&lang), "taxonomy_list.html", &context)
    }

    fn render_term(&mut self, t: usize, term: usize, pager: Option<usize>) -> Result<String> {
        let taxonomy = &self.site.taxonomies[t];
        let lang = taxonomy.lang.clone();
        let term_model = &taxonomy.terms[term];
        let view = self.model.taxonomies[t]["terms"][term].clone();
        let current_path = match pager {
            Some(n) if n > 1 => format!("{}{}/{n}/", term_model.path, taxonomy.paginate_path),
            _ => term_model.path.clone(),
        };
        let mut context = self.base_context(&lang, &current_path);
        if let Some(n) = pager {
            let paginator = paginator_view(
                &view["pages"],
                taxonomy.paginate_by.expect("paginated taxonomy"),
                n,
                &term_model.permalink,
                &taxonomy.paginate_path,
            );
            context.insert("paginator", &paginator);
        }
        let mut taxonomy_view = self.model.taxonomies[t].clone();
        taxonomy_view.as_object_mut().map(|o| o.remove("terms"));
        context.insert("taxonomy", &taxonomy_view);
        context.insert("term", &view);
        insert_translations(&mut context, &view["translations"], self.site);
        templates::render(self.engine(&lang), "taxonomy_single.html", &context)
    }

    fn render_feed(
        &mut self,
        lang: &str,
        pages: Vec<usize>,
        output_path: &str,
        alternate: &str,
        title: &str,
        scope: Option<(&str, Value)>,
    ) -> Result<String> {
        let mut dated: Vec<usize> = pages
            .into_iter()
            .filter(|&p| self.site.pages[p].date.is_some())
            .collect();
        dated.sort_by(|&a, &b| {
            self.site.pages[b]
                .date
                .cmp(&self.site.pages[a].date)
                .then_with(|| {
                    self.site.pages[a]
                        .file
                        .rel
                        .cmp(&self.site.pages[b].file.rel)
                })
        });
        dated.truncate(self.site.config.feeds.limit);
        let entries: Vec<Value> = dated.iter().map(|&p| self.model.pages[p].clone()).collect();
        let last_updated = dated
            .iter()
            .filter_map(|&p| {
                self.site.pages[p]
                    .updated
                    .as_ref()
                    .or(self.site.pages[p].date.as_ref())
            })
            .max()
            .map(|d| d.rfc3339());
        let feed_url = format!("{}/{output_path}", self.site.config.base_url);
        let mut context = self.base_context(lang, &format!("/{output_path}"));
        context.insert("pages", &entries);
        context.insert("feed_url", &feed_url);
        context.insert("feed_alternate", alternate);
        context.insert("feed_title", title);
        context.insert("last_updated", &last_updated);
        if let Some((name, value)) = scope {
            context.insert(name, &value);
        }
        templates::render(self.engine(lang), "atom.xml", &context)
    }

    /// `<prefix>search.json`: every page of the language, newest first, with
    /// what a client-side search matches on and links to.
    fn render_search_index(&self, lang: &str) -> Result<String> {
        let site = self.site;
        let mut pages: Vec<usize> = (0..site.pages.len())
            .filter(|&p| site.pages[p].file.lang == lang)
            .collect();
        pages.sort_by(|&a, &b| {
            site.pages[b]
                .date
                .cmp(&site.pages[a].date)
                .then_with(|| site.pages[a].file.rel.cmp(&site.pages[b].file.rel))
        });
        let entries: Vec<Value> = pages
            .iter()
            .map(|&p| {
                let view = &self.model.pages[p];
                let mut terms = serde_json::Map::new();
                if let Some(taxonomies) = view["taxonomies"].as_object() {
                    for (name, list) in taxonomies {
                        let names: Vec<Value> = list
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|t| t["name"].clone())
                            .collect();
                        terms.insert(name.clone(), Value::Array(names));
                    }
                }
                let mut entry = json!({
                    "title": view["title"],
                    "permalink": view["permalink"],
                    "path": view["path"],
                    "date": view["date"],
                    "section": view["section"]["title"],
                    "description": view["meta_description"],
                    "taxonomies": terms,
                });
                if site.config.search.content {
                    entry["content"] = Value::String(crate::util::html_to_text(
                        view["content"].as_str().unwrap_or_default(),
                    ));
                }
                entry
            })
            .collect();
        let mut text = serde_json::to_string(&json!({"lang": lang, "pages": entries}))?;
        text.push('\n');
        Ok(text)
    }

    fn render_sitemap(&mut self) -> Result<String> {
        let site = self.site;
        let mut entries: Vec<(String, Value)> = Vec::new();
        entries.push((format!("{}/", site.config.base_url), json!({"permalink": format!("{}/", site.config.base_url), "updated": Value::Null, "alternates": []})));
        for page in &site.pages {
            let alternates: Vec<Value> = page.translations.iter().map(|&t| json!({"lang": site.pages[t].file.lang, "permalink": site.pages[t].permalink})).collect();
            let updated = page
                .updated
                .as_ref()
                .or(page.date.as_ref())
                .map(|d| d.ymd());
            entries.push((
                page.permalink.clone(),
                json!({"permalink": page.permalink, "updated": updated, "alternates": alternates}),
            ));
        }
        for section in &site.sections {
            let alternates: Vec<Value> = section.translations.iter().map(|&t| json!({"lang": site.sections[t].lang, "permalink": site.sections[t].permalink})).collect();
            entries.push((section.permalink.clone(), json!({"permalink": section.permalink, "updated": section.updated.as_ref().map(|d| d.ymd()), "alternates": alternates})));
            if let Some(pagers) = pager_count(section.pages.len(), section.front.paginate_by) {
                for n in 2..=pagers {
                    let permalink =
                        format!("{}{}/{n}/", section.permalink, section.front.paginate_path);
                    entries.push((
                        permalink.clone(),
                        json!({"permalink": permalink, "updated": Value::Null, "alternates": []}),
                    ));
                }
            }
            if section.front.year_archives {
                for (year, _) in site.years_of(section) {
                    let alternates: Vec<Value> = section
                        .translations
                        .iter()
                        .filter(|&&t| {
                            let other = &site.sections[t];
                            other.front.year_archives
                                && site.years_of(other).iter().any(|(y, _)| *y == year)
                        })
                        .map(|&t| json!({"lang": site.sections[t].lang, "permalink": format!("{}{year}/", site.sections[t].permalink)}))
                        .collect();
                    let permalink = format!("{}{year}/", section.permalink);
                    entries.push((
                        permalink.clone(),
                        json!({"permalink": permalink, "updated": Value::Null, "alternates": alternates}),
                    ));
                }
            }
        }
        for taxonomy in &site.taxonomies {
            let alternates: Vec<Value> = site
                .taxonomies
                .iter()
                .filter(|x| x.name == taxonomy.name)
                .map(|x| json!({"lang": x.lang, "permalink": x.permalink}))
                .collect();
            entries.push((taxonomy.permalink.clone(), json!({"permalink": taxonomy.permalink, "updated": Value::Null, "alternates": alternates})));
            for term in &taxonomy.terms {
                let alternates: Vec<Value> = term
                    .translations
                    .iter()
                    .map(|(lang, index)| {
                        let other = site
                            .taxonomies
                            .iter()
                            .find(|x| x.name == taxonomy.name && x.lang == *lang)
                            .map(|x| &x.terms[*index])
                            .expect("resolved term");
                        json!({"lang": lang, "permalink": other.permalink})
                    })
                    .collect();
                entries.push((term.permalink.clone(), json!({"permalink": term.permalink, "updated": Value::Null, "alternates": alternates})));
                if let Some(pagers) = pager_count(term.pages.len(), taxonomy.paginate_by) {
                    for n in 2..=pagers {
                        let permalink =
                            format!("{}{}/{n}/", term.permalink, taxonomy.paginate_path);
                        entries.push((permalink.clone(), json!({"permalink": permalink, "updated": Value::Null, "alternates": []})));
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let values: Vec<Value> = entries.into_iter().map(|(_, v)| v).collect();
        let fallback = site.config.fallback().to_owned();
        let mut context = self.base_context(&fallback, "/sitemap.xml");
        context.insert("entries", &values);
        templates::render(self.engine(&fallback), "sitemap.xml", &context)
    }
}

/// `translations` (each entry marked `current` when it is the language
/// being rendered), `alternates`, and `x_default`, the fallback language's
/// version for `hreflang="x-default"`, null when there is none.
fn insert_translations(context: &mut tera::Context, translations: &Value, site: &Site) {
    let lang = context
        .get("lang")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let marked: Vec<Value> = translations
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let mut t = t.clone();
            if let Some(object) = t.as_object_mut() {
                let current = object.get("lang").and_then(Value::as_str) == Some(lang.as_str());
                object.insert("current".to_owned(), Value::Bool(current));
            }
            t
        })
        .collect();
    let map = alternates(translations);
    let x_default = map
        .get(site.config.fallback())
        .cloned()
        .unwrap_or(Value::Null);
    context.insert("translations", &marked);
    context.insert("alternates", &map);
    context.insert("x_default", &x_default);
}

/// `{lang: permalink}` for a translations list, so a language switcher is
/// `alternates | get(key=entry.code, default=entry.url_prefix)` and no
/// template has to juggle nested loops and `set_global`.
fn alternates(translations: &Value) -> Value {
    let mut map = serde_json::Map::new();
    for t in translations.as_array().into_iter().flatten() {
        if let (Some(lang), Some(permalink)) = (t["lang"].as_str(), t["permalink"].as_str()) {
            map.insert(lang.to_owned(), Value::String(permalink.to_owned()));
        }
    }
    Value::Object(map)
}

fn light(section: &Value) -> Value {
    json!({
        "title": section["title"],
        "permalink": section["permalink"],
        "path": section["path"],
        "lang": section["lang"],
        "description": section["description"],
        "extra": section["extra"],
    })
}

fn paginator_view(pages: &Value, by: usize, n: usize, base: &str, paginate_path: &str) -> Value {
    let all = pages.as_array().cloned().unwrap_or_default();
    let count = pager_count(all.len(), Some(by)).unwrap_or(1);
    let start = (n - 1) * by;
    let end = (start + by).min(all.len());
    let slice: Vec<Value> = if start < all.len() {
        all[start..end].to_vec()
    } else {
        Vec::new()
    };
    let url = |k: usize| {
        if k == 1 {
            base.to_owned()
        } else {
            format!("{base}{paginate_path}/{k}/")
        }
    };
    json!({
        "paginate_by": by,
        "number_pagers": count,
        "current_index": n,
        "first": url(1),
        "last": url(count),
        "previous": (n > 1).then(|| url(n - 1)),
        "next": (n < count).then(|| url(n + 1)),
        "base_url": base,
        "pages": slice,
        "total_pages": all.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(n: usize) -> Value {
        Value::Array((1..=n).map(|i| json!({"n": i})).collect())
    }

    #[test]
    fn a_paginator_slices_pages_and_links_its_neighbours() {
        let base = "https://x/en/blog/";
        let first = paginator_view(&pages(5), 2, 1, base, "page");
        assert_eq!(first["paginate_by"], 2);
        assert_eq!(first["number_pagers"], 3);
        assert_eq!(first["current_index"], 1);
        assert_eq!(first["first"], base);
        assert_eq!(first["last"], "https://x/en/blog/page/3/");
        assert!(first["previous"].is_null());
        assert_eq!(first["next"], "https://x/en/blog/page/2/");
        assert_eq!(first["pages"], json!([{"n": 1}, {"n": 2}]));
        assert_eq!(first["total_pages"], 5);
        assert_eq!(first["base_url"], base);

        let middle = paginator_view(&pages(5), 2, 2, base, "page");
        assert_eq!(middle["previous"], base, "page 1 is the bare section URL");
        assert_eq!(middle["next"], "https://x/en/blog/page/3/");
        assert_eq!(middle["pages"], json!([{"n": 3}, {"n": 4}]));

        let last = paginator_view(&pages(5), 2, 3, base, "page");
        assert_eq!(last["previous"], "https://x/en/blog/page/2/");
        assert!(last["next"].is_null());
        assert_eq!(last["pages"], json!([{"n": 5}]), "a short last page");
    }

    #[test]
    fn a_paginator_over_nothing_is_one_empty_page() {
        let view = paginator_view(&pages(0), 10, 1, "https://x/en/", "p");
        assert_eq!(view["number_pagers"], 1);
        assert_eq!(view["pages"], json!([]));
        assert!(view["next"].is_null() && view["previous"].is_null());
        assert_eq!(view["last"], "https://x/en/");
        let beyond = paginator_view(&pages(3), 2, 5, "https://x/en/", "p");
        assert_eq!(
            beyond["pages"],
            json!([]),
            "past the end is empty, not a panic"
        );
        let custom = paginator_view(&pages(3), 1, 2, "https://x/en/", "seite");
        assert_eq!(custom["next"], "https://x/en/seite/3/");
    }

    #[test]
    fn alternates_map_language_to_permalink() {
        let translations = json!([
            {"lang": "en", "permalink": "https://x/en/a/", "title": "A"},
            {"lang": "he", "permalink": "https://x/he/a/"},
            {"lang": "fr"},
            {"permalink": "https://x/nolang/"},
        ]);
        let map = alternates(&translations);
        assert_eq!(
            map,
            json!({"en": "https://x/en/a/", "he": "https://x/he/a/"})
        );
        assert_eq!(alternates(&json!([])), json!({}));
        assert_eq!(
            alternates(&Value::Null),
            json!({}),
            "not a list: empty, not a panic"
        );
    }

    #[test]
    fn the_light_section_view_keeps_only_what_a_page_needs() {
        let full = json!({
            "title": "Blog",
            "permalink": "https://x/en/blog/",
            "path": "/en/blog/",
            "lang": "en",
            "description": "d",
            "extra": {"k": 1},
            "pages": [1, 2, 3],
            "content": "<p>heavy</p>",
        });
        let view = light(&full);
        assert_eq!(view["title"], "Blog");
        assert_eq!(view["extra"]["k"], 1);
        assert!(view.get("pages").is_none());
        assert!(view.get("content").is_none());
        assert_eq!(view.as_object().unwrap().len(), 6);
    }
}
