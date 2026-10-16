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

/// Render every output of `plan` into memory.
fn render_all(site: &Site, plan: &Plan) -> Result<BTreeMap<String, Vec<u8>>> {
    let model = Arc::new(Model::new(site, plan.paths()));
    let mut engines = BTreeMap::new();
    for code in site.config.language_codes() {
        engines.insert(
            code.clone(),
            templates::engine(&model, &site.templates, &code)?,
        );
    }
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
        let mut context = tera::Context::new();
        context.insert("config", &self.model.config);
        context.insert("lang", lang);
        context.insert("language", &templates::language_view(self.site, lang));
        context.insert("languages", &self.model.languages);
        context.insert("current_path", current_path);
        context.insert(
            "current_url",
            &format!("{}{}", self.site.config.base_url, current_path),
        );
        // The generator itself, for a "made by" line. A build's output is a
        // function of its sources and the tool, and the tool's version is
        // part of that function, so naming it is deterministic.
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
                self.render_feed(&lang, pages, &output.path, &alternate, &title, None)?
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
            Kind::Robots => format!(
                "User-agent: *\nAllow: /\n\nSitemap: {}/sitemap.xml\n",
                self.site.config.base_url
            )
            .into_bytes(),
            Kind::Root => {
                let fallback = self.site.config.fallback().to_owned();
                if self.site.templates.has("root.html") {
                    let mut context = self.base_context(&fallback, "/");
                    context.insert("translations", &Vec::<Value>::new());
                    context.insert("alternates", &Value::Object(serde_json::Map::new()));
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
                context.insert("translations", &Vec::<Value>::new());
                context.insert("alternates", &Value::Object(serde_json::Map::new()));
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
        let mut view = self.model.pages[i].clone();
        let mut context = self.base_context(&lang, &page.path);
        let source = page.body_source.unwrap_or(i);
        if self.site.pages[source].file.body_kind == crate::content::BodyKind::Tera {
            let body = self.site.pages[source].file.body.clone();
            let what = page.file.rel.clone();
            context.insert("page", &view);
            let html = templates::render_str(self.engine(&lang), &body, &context, &what)?;
            view["content"] = Value::String(html);
        }
        // Only the light view: the full section carries every page of the
        // section, and cloning that once per page is quadratic.
        let section = light(&self.model.sections[page.section]);
        context.insert("page", &view);
        context.insert("section", &section);
        context.insert("translations", &view["translations"]);
        context.insert("alternates", &alternates(&view["translations"]));
        templates::render(self.engine(&lang), &template, &context)
    }

    fn render_section(&mut self, s: usize, pager: Option<usize>) -> Result<String> {
        let section = &self.site.sections[s];
        let lang = section.lang.clone();
        let template = self.site.section_template(section);
        let mut view = self.model.sections[s].clone();
        let current_path = match pager {
            Some(n) if n > 1 => format!("{}{}/{n}/", section.path, section.front.paginate_path),
            _ => section.path.clone(),
        };
        let mut context = self.base_context(&lang, &current_path);
        let source = section.body_source.unwrap_or(s);
        if let Some(file) = &self.site.sections[source].file
            && file.body_kind == crate::content::BodyKind::Tera
        {
            let body = file.body.clone();
            let what = file.rel.clone();
            context.insert("section", &view);
            let html = templates::render_str(self.engine(&lang), &body, &context, &what)?;
            view["content"] = Value::String(html);
        }
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
        context.insert("section", &view);
        context.insert("translations", &view["translations"]);
        context.insert("alternates", &alternates(&view["translations"]));
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
        context.insert("year", &year);
        context.insert("pages", &pages);
        context.insert("years", &view["years"]);
        context.insert("translations", &translations);
        context.insert("alternates", &alternates(&translations));
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
        context.insert("translations", &translations);
        context.insert("alternates", &alternates(&translations));
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
        context.insert("translations", &view["translations"]);
        context.insert("alternates", &alternates(&view["translations"]));
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
