//! Tera engines, one per language, so every function knows which language
//! it is rendering without being told; the JSON views of the site model they
//! read; and the functions and filters themselves. Anything
//! non-deterministic (`now`, `get_random`, `get_env`) is an error.

use crate::i18n::{self, Translations};
use crate::site::{Page, Section, Site, Templates};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use tera::Tera;

#[derive(Clone)]
pub struct Model {
    pub config: Value,
    pub languages: Vec<Value>,
    pub pages: Vec<Value>,
    pub sections: Vec<Value>,
    pub taxonomies: Vec<Value>,
    pub page_lookup: BTreeMap<(String, String), usize>,
    pub section_lookup: BTreeMap<(String, String), usize>,
    pub asset_lookup: BTreeMap<(String, String), String>,
    pub taxonomy_lookup: BTreeMap<(String, String), usize>,
    /// (lang, taxonomy, term name) -> permalink
    pub term_lookup: BTreeMap<(String, String, String), String>,
    pub i18n: BTreeMap<String, Translations>,
    pub base_url: String,
    pub root: PathBuf,
    pub planned: BTreeSet<String>,
    pub language_codes: Vec<String>,
    pub markdown_options: crate::markdown::RenderOptions,
}

fn light_page(page: &Page) -> Value {
    json!({
        "title": page.front.title,
        "permalink": page.permalink,
        "path": page.path,
        "lang": page.file.lang,
        "date": page.date.as_ref().map(|d| d.ymd()),
    })
}

fn light_section(section: &Section) -> Value {
    json!({
        "title": section.title(),
        "permalink": section.permalink,
        "path": section.path,
        "lang": section.lang,
    })
}

fn translations_view(site: &Site, indexes: &[usize], lang: &str) -> Vec<Value> {
    indexes
        .iter()
        .map(|&t| {
            let page = &site.pages[t];
            json!({"lang": page.file.lang, "permalink": page.permalink, "path": page.path, "title": page.front.title, "current": page.file.lang == lang})
        })
        .collect()
}

fn section_translations_view(site: &Site, indexes: &[usize], lang: &str) -> Vec<Value> {
    indexes
        .iter()
        .map(|&t| {
            let section = &site.sections[t];
            json!({"lang": section.lang, "permalink": section.permalink, "path": section.path, "title": section.title(), "current": section.lang == lang})
        })
        .collect()
}

/// The language's description, or the site's.
fn language_description(site: &Site, lang: &str) -> String {
    site.config
        .language(lang)
        .and_then(|l| l.description.clone())
        .unwrap_or_else(|| site.config.description.clone())
}

/// What `<meta name="description">` says about a page: its description,
/// else the start of its summary or text, else the language's description.
pub fn page_meta_description(site: &Site, page: &Page) -> String {
    if let Some(description) = page
        .front
        .description
        .as_deref()
        .filter(|d| !d.trim().is_empty())
    {
        return description.to_owned();
    }
    let source = page
        .rendered
        .summary
        .as_deref()
        .unwrap_or(&page.rendered.html);
    let text = crate::util::html_to_text(source);
    if text.is_empty() {
        language_description(site, &page.file.lang)
    } else {
        crate::util::truncate_words(&text, 160)
    }
}

/// Schema.org JSON-LD for a page, as a string ready for a
/// `<script type="application/ld+json">`: a `BlogPosting` for a dated page,
/// a `WebPage` otherwise, with its translations as `workTranslation`.
pub fn page_json_ld(site: &Site, i: usize, meta_description: &Value) -> String {
    let page = &site.pages[i];
    let lang = &page.file.lang;
    let language = site.config.language(lang).expect("configured language");
    let dated = page.date.is_some();
    let mut value = json!({
        "@context": "https://schema.org",
        "@type": if dated { "BlogPosting" } else { "WebPage" },
        "headline": page.front.title,
        "name": page.front.title,
        "description": meta_description,
        "url": page.permalink,
        "mainEntityOfPage": page.permalink,
        "inLanguage": lang,
        "isPartOf": {
            "@type": "WebSite",
            "name": language.title.clone().unwrap_or_else(|| site.config.title.clone()),
            "url": site.permalink(language.prefix()),
        },
    });
    if !site.config.author.is_empty() {
        value["author"] = json!({"@type": "Person", "name": site.config.author});
    }
    if let Some(date) = &page.date {
        value["datePublished"] = json!(date.rfc3339());
        value["dateModified"] = json!(page.updated.as_ref().unwrap_or(date).rfc3339());
        value["wordCount"] = json!(page.rendered.word_count);
    }
    let keywords: Vec<&str> = page
        .front
        .taxonomies
        .values()
        .flatten()
        .map(String::as_str)
        .collect();
    if !keywords.is_empty() {
        value["keywords"] = json!(keywords.join(", "));
    }
    let others: Vec<Value> = page
        .translations
        .iter()
        .filter(|&&t| t != i)
        .map(|&t| {
            json!({
                "@type": if dated { "BlogPosting" } else { "WebPage" },
                "url": site.pages[t].permalink,
                "inLanguage": site.pages[t].file.lang,
            })
        })
        .collect();
    if !others.is_empty() {
        value["workTranslation"] = Value::Array(others);
    }
    // `</script>` inside a string would end the element early.
    serde_json::to_string_pretty(&value)
        .expect("JSON-LD serializes")
        .replace('<', "\\u003c")
}

pub fn section_meta_description(site: &Site, section: &Section) -> String {
    section
        .front
        .description
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| language_description(site, &section.lang))
}

pub fn page_view(site: &Site, i: usize) -> Value {
    let page = &site.pages[i];
    let section = &site.sections[page.section];
    let date = page.date.as_ref();
    let updated = page.updated.as_ref().or(date);
    // Every configured taxonomy is present on every page, empty when the
    // page has no terms, so `page.taxonomies.tags` is always safe to loop.
    let mut taxonomies = serde_json::Map::new();
    for taxonomy in &site.config.taxonomies {
        taxonomies.insert(taxonomy.name.clone(), Value::Array(Vec::new()));
    }
    for (name, terms) in &page.front.taxonomies {
        let list: Vec<Value> = terms
            .iter()
            .map(|term| {
                let found = site
                    .taxonomies
                    .iter()
                    .find(|t| t.name == *name && t.lang == page.file.lang)
                    .and_then(|t| t.terms.iter().find(|x| x.name == *term));
                match found {
                    Some(t) => json!({"name": t.name, "slug": t.slug, "permalink": t.permalink, "path": t.path}),
                    None => json!({"name": term}),
                }
            })
            .collect();
        taxonomies.insert(name.clone(), Value::Array(list));
    }
    let words = page.rendered.word_count;
    json!({
        "title": page.front.title,
        "description": page.front.description,
        "date": date.map(|d| d.ymd()),
        "datetime": date.map(|d| d.rfc3339()),
        "year": date.map(|d| chrono::Datelike::year(&d.datetime)),
        "month": date.map(|d| chrono::Datelike::month(&d.datetime)),
        "day": date.map(|d| chrono::Datelike::day(&d.datetime)),
        "updated": page.updated.as_ref().map(|d| d.ymd()),
        "updated_datetime": updated.map(|d| d.rfc3339()),
        "slug": page.slug,
        "path": page.path,
        "permalink": page.permalink,
        "content": page.rendered.html,
        "summary": page.rendered.summary,
        "toc": page.rendered.toc,
        "word_count": words,
        "reading_time": words.div_ceil(200).max(1),
        "taxonomies": taxonomies,
        "extra": crate::util::toml_to_json(&toml::Value::Table(page.front.extra.clone())),
        "lang": page.file.lang,
        "translations": translations_view(site, &page.translations, &page.file.lang),
        "meta_description": page_meta_description(site, page),
        "related": page.related.iter().map(|&p| light_page(&site.pages[p])).collect::<Vec<_>>(),
        "previous": page.previous.map(|p| light_page(&site.pages[p])),
        "next": page.next.map(|p| light_page(&site.pages[p])),
        "draft": page.front.draft,
        "weight": page.front.weight,
        "relative_path": page.file.rel,
        "components": page.file.components,
        "body_kind": page.file.body_kind,
        "section": light_section(section),
    })
}

pub fn section_view(site: &Site, i: usize, pages: &[Value]) -> Value {
    let section = &site.sections[i];
    json!({
        "title": section.title(),
        "description": section.front.description,
        "path": section.path,
        "permalink": section.permalink,
        "lang": section.lang,
        "content": section.rendered.html,
        "summary": section.rendered.summary,
        "toc": section.rendered.toc,
        "word_count": section.rendered.word_count,
        "extra": crate::util::toml_to_json(&toml::Value::Table(section.front.extra.clone())),
        "pages": section.pages.iter().map(|&p| pages[p].clone()).collect::<Vec<_>>(),
        "page_count": section.pages.len(),
        "subsections": section.subsections.iter().map(|&s| light_section(&site.sections[s])).collect::<Vec<_>>(),
        "parent": section.parent.map(|p| light_section(&site.sections[p])),
        "translations": section_translations_view(site, &section.translations, &section.lang),
        "meta_description": section_meta_description(site, section),
        "components": section.components,
        "relative_path": section.rel(),
        "updated": section.updated.as_ref().map(|d| d.ymd()),
        "paginate_by": section.front.paginate_by,
        "sort_by": section.front.sort_by,
        "is_root": section.is_root(),
        "year_archives": section.front.year_archives,
        "years": years_view(site, section),
    })
}

/// The year archives of a section (`year_archives = true`): newest first,
/// each with its page count and the URL of its listing. Empty otherwise.
fn years_view(site: &Site, section: &crate::site::Section) -> Vec<Value> {
    if !section.front.year_archives {
        return Vec::new();
    }
    site.years_of(section)
        .into_iter()
        .map(|(year, pages)| {
            json!({
                "year": year,
                "count": pages.len(),
                "permalink": format!("{}{year}/", section.permalink),
                "path": format!("{}{year}/", section.path),
            })
        })
        .collect()
}

pub fn language_view(site: &Site, code: &str) -> Value {
    let language = site.config.language(code).expect("configured language");
    json!({
        "code": language.code,
        "name": language.name,
        "direction": language.direction.as_str(),
        "url_prefix": language.prefix(),
        "title": language.title.clone().unwrap_or_else(|| site.config.title.clone()),
        "description": language.description.clone().unwrap_or_else(|| site.config.description.clone()),
        "generate_feeds": site.language_feeds(code),
        "locale": language.locale,
        "date_format": language.date_format.clone().unwrap_or_else(|| "%Y-%m-%d".to_owned()),
        "date_format_long": language.date_format_long.clone().or_else(|| language.date_format.clone()).unwrap_or_else(|| "%Y-%m-%d".to_owned()),
        "is_fallback": site.config.fallback() == code,
    })
}

impl Model {
    pub fn new(site: &Site, planned: BTreeSet<String>) -> Model {
        let pages: Vec<Value> = (0..site.pages.len()).map(|i| page_view(site, i)).collect();
        let sections: Vec<Value> = (0..site.sections.len())
            .map(|i| section_view(site, i, &pages))
            .collect();
        let mut taxonomy_lookup = BTreeMap::new();
        let mut term_lookup = BTreeMap::new();
        let mut taxonomies = Vec::new();
        for (t, taxonomy) in site.taxonomies.iter().enumerate() {
            taxonomy_lookup.insert((taxonomy.lang.clone(), taxonomy.name.clone()), t);
            let terms: Vec<Value> = taxonomy
                .terms
                .iter()
                .map(|term| {
                    term_lookup.insert((taxonomy.lang.clone(), taxonomy.name.clone(), term.name.clone()), term.permalink.clone());
                    let translations: Vec<Value> = term
                        .translations
                        .iter()
                        .map(|(lang, index)| {
                            let other = site
                                .taxonomies
                                .iter()
                                .find(|x| x.name == taxonomy.name && x.lang == *lang)
                                .map(|x| &x.terms[*index])
                                .expect("resolved term translation");
                            json!({"lang": lang, "name": other.name, "permalink": other.permalink, "path": other.path})
                        })
                        .collect();
                    json!({
                        "name": term.name,
                        "slug": term.slug,
                        "path": term.path,
                        "permalink": term.permalink,
                        "page_count": term.pages.len(),
                        "pages": term.pages.iter().map(|&p| pages[p].clone()).collect::<Vec<_>>(),
                        "translations": translations,
                    })
                })
                .collect();
            taxonomies.push(json!({
                "name": taxonomy.name,
                "lang": taxonomy.lang,
                "path": taxonomy.path,
                "permalink": taxonomy.permalink,
                "terms": terms,
            }));
        }
        let mut asset_lookup = BTreeMap::new();
        let language_codes = site.config.language_codes();
        for asset in &site.assets {
            let rel = asset.rel();
            for lang in &language_codes {
                if !site.under_drafted_section(&asset.components, lang)
                    && let Ok(dir) = site.section_path(lang, &asset.components)
                {
                    let permalink = site.permalink(&format!("{dir}{}", asset.name));
                    asset_lookup.insert((lang.clone(), rel.clone()), permalink);
                }
            }
        }
        let mut config = serde_json::to_value(&site.config).expect("config serializes");
        config["extra"] = crate::util::toml_to_json(&toml::Value::Table(site.config.extra.clone()));
        Model {
            config,
            languages: site
                .config
                .languages
                .iter()
                .map(|l| language_view(site, &l.code))
                .collect(),
            pages,
            sections,
            taxonomies,
            page_lookup: site.page_lookup.clone(),
            section_lookup: site.section_lookup.clone(),
            asset_lookup,
            taxonomy_lookup,
            term_lookup,
            i18n: site.i18n.clone(),
            base_url: site.config.base_url.clone(),
            root: site.root.clone(),
            planned,
            language_codes,
            markdown_options: site.config.render_options(),
        }
    }

    /// `@/blog/post.md` (or with an explicit language suffix) to a permalink.
    fn resolve(&self, path: &str, lang: &str) -> Option<(String, &Value)> {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        let without_ext = file_name
            .rsplit_once('.')
            .map(|(s, _)| s)
            .unwrap_or(file_name);
        let (lang, key) = match without_ext.rsplit_once('.') {
            Some((_, suffix)) if self.language_codes.iter().any(|c| c == suffix) => {
                (suffix.to_owned(), crate::site::strip_lang(path, suffix))
            }
            _ => (lang.to_owned(), path.to_owned()),
        };
        if let Some(&p) = self.page_lookup.get(&(lang.clone(), key.clone())) {
            return Some((
                self.pages[p]["permalink"].as_str().unwrap_or("").to_owned(),
                &self.pages[p],
            ));
        }
        if let Some(&s) = self.section_lookup.get(&(lang.clone(), key.clone())) {
            return Some((
                self.sections[s]["permalink"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
                &self.sections[s],
            ));
        }
        if let Some(permalink) = self
            .asset_lookup
            .get(&(lang.clone(), path.to_owned()))
            .or_else(|| self.asset_lookup.get(&(lang, key)))
        {
            return Some((permalink.clone(), &Value::Null));
        }
        None
    }
}

pub struct Engine {
    pub tera: Tera,
    pub lang: String,
}

fn err(message: String) -> tera::Error {
    tera::Error::msg(message)
}

fn arg_string(
    args: &HashMap<String, Value>,
    name: &str,
    function: &str,
) -> tera::Result<Option<String>> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(err(format!(
            "{function}: argument `{name}` must be a string, got {other}"
        ))),
    }
}

fn require_string(
    args: &HashMap<String, Value>,
    name: &str,
    function: &str,
) -> tera::Result<String> {
    arg_string(args, name, function)?
        .ok_or_else(|| err(format!("{function}: argument `{name}` is required")))
}

fn arg_bool(
    args: &HashMap<String, Value>,
    name: &str,
    function: &str,
    default: bool,
) -> tera::Result<bool> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => Err(err(format!(
            "{function}: argument `{name}` must be a boolean, got {other}"
        ))),
    }
}

fn lang_arg(
    args: &HashMap<String, Value>,
    function: &str,
    default: &str,
    model: &Model,
) -> tera::Result<String> {
    let lang = arg_string(args, "lang", function)?.unwrap_or_else(|| default.to_owned());
    if !model.language_codes.contains(&lang) {
        return Err(err(format!(
            "{function}: {lang:?} is not a configured language"
        )));
    }
    Ok(lang)
}

/// Build the engine for `lang`. User templates first, then the built-in
/// ones for any name the user did not supply.
pub fn engine(model: &Arc<Model>, templates: &Templates, lang: &str) -> Result<Engine> {
    let mut tera = Tera::default();
    let mut raw: Vec<(String, String)> = templates
        .files
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (name, text) in [
        ("atom.xml", crate::builtin::ATOM),
        ("sitemap.xml", crate::builtin::SITEMAP),
        ("404.html", crate::builtin::NOT_FOUND),
    ] {
        if !templates.has(name) {
            raw.push((name.to_owned(), text.to_owned()));
        }
    }
    tera.add_raw_templates(raw)
        .map_err(|e| anyhow::anyhow!("{}", describe(&e)))?;
    // Tera's default escaper also rewrites `/` as `&#x2F;`, which turns every
    // permalink into an entity soup. The five characters that matter are
    // escaped; a slash is not one of them.
    tera.set_escape_fn(crate::util::escape_html);
    let lang = lang.to_owned();

    for forbidden in ["now", "get_random", "get_env"] {
        tera.register_function(forbidden, move |_: &HashMap<String, Value>| -> tera::Result<Value> {
            Err(err(format!(
                "{forbidden}() is not available: its result would differ between builds of the same sources"
            )))
        });
    }

    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_function(
            "trans",
            move |args: &HashMap<String, Value>| -> tera::Result<Value> {
                let key = require_string(args, "key", "trans")?;
                let lang = lang_arg(args, "trans", &lang, &model)?;
                let translations = model
                    .i18n
                    .get(&lang)
                    .ok_or_else(|| err(format!("no translations for {lang}")))?;
                translations
                    .get(&key)
                    .map(|s| Value::String(s.to_owned()))
                    .map_err(|e| err(format!("trans: {e}")))
            },
        );
    }
    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_function("get_url", move |args: &HashMap<String, Value>| -> tera::Result<Value> {
            let path = require_string(args, "path", "get_url")?;
            let lang = lang_arg(args, "get_url", &lang, &model)?;
            let check = arg_bool(args, "check", "get_url", true)?;
            if let Some(content) = path.strip_prefix("@/") {
                let (content_path, fragment) = match content.split_once('#') {
                    Some((p, f)) => (p, Some(f)),
                    None => (content, None),
                };
                return match model.resolve(content_path, &lang) {
                    Some((permalink, _)) => Ok(Value::String(match fragment {
                        Some(f) => format!("{permalink}#{f}"),
                        None => permalink,
                    })),
                    None => Err(err(format!("get_url: no content file matches {path:?} in language {lang}"))),
                };
            }
            let relative = path.trim_start_matches('/');
            let known = model.planned.contains(relative)
                || model.planned.contains(&format!("{}index.html", if relative.is_empty() || relative.ends_with('/') { relative.to_owned() } else { format!("{relative}/") }));
            if check && !known {
                return Err(err(format!(
                    "get_url: {path:?} is not a file this build produces; pass check=false for a file another tool writes"
                )));
            }
            Ok(Value::String(format!("{}/{relative}", model.base_url)))
        });
    }
    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_function(
            "get_page",
            move |args: &HashMap<String, Value>| -> tera::Result<Value> {
                let path = require_string(args, "path", "get_page")?;
                let lang = lang_arg(args, "get_page", &lang, &model)?;
                match model.resolve(path.trim_start_matches("@/"), &lang) {
                    Some((_, value)) if value.get("slug").is_some() => Ok(value.clone()),
                    _ => Err(err(format!(
                        "get_page: no page matches {path:?} in language {lang}"
                    ))),
                }
            },
        );
    }
    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_function(
            "get_section",
            move |args: &HashMap<String, Value>| -> tera::Result<Value> {
                let path = require_string(args, "path", "get_section")?;
                let lang = lang_arg(args, "get_section", &lang, &model)?;
                match model.resolve(path.trim_start_matches("@/"), &lang) {
                    Some((_, value)) if value.get("is_root").is_some() => Ok(value.clone()),
                    _ => Err(err(format!(
                        "get_section: no section matches {path:?} in language {lang}"
                    ))),
                }
            },
        );
    }
    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_function(
            "get_taxonomy",
            move |args: &HashMap<String, Value>| -> tera::Result<Value> {
                let name = require_string(args, "name", "get_taxonomy")?;
                let lang = lang_arg(args, "get_taxonomy", &lang, &model)?;
                match model.taxonomy_lookup.get(&(lang.clone(), name.clone())) {
                    Some(&t) => Ok(model.taxonomies[t].clone()),
                    None => Err(err(format!(
                        "get_taxonomy: {name:?} is not a configured taxonomy"
                    ))),
                }
            },
        );
    }
    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_function("get_taxonomy_url", move |args: &HashMap<String, Value>| -> tera::Result<Value> {
            let name = require_string(args, "name", "get_taxonomy_url")?;
            let term = require_string(args, "term", "get_taxonomy_url")?;
            let lang = lang_arg(args, "get_taxonomy_url", &lang, &model)?;
            if !model.taxonomy_lookup.contains_key(&(lang.clone(), name.clone())) {
                return Err(err(format!("get_taxonomy_url: {name:?} is not a configured taxonomy")));
            }
            match model.term_lookup.get(&(lang.clone(), name.clone(), term.clone())) {
                Some(permalink) => Ok(Value::String(permalink.clone())),
                None => Err(err(format!("get_taxonomy_url: taxonomy {name:?} has no term {term:?} in language {lang}"))),
            }
        });
    }
    {
        let model = Arc::clone(model);
        tera.register_function(
            "load_data",
            move |args: &HashMap<String, Value>| -> tera::Result<Value> {
                let path = require_string(args, "path", "load_data")?;
                let required = arg_bool(args, "required", "load_data", true)?;
                let format = arg_string(args, "format", "load_data")?;
                if std::path::Path::new(&path).is_absolute() || path.split('/').any(|p| p == "..") {
                    return Err(err(format!(
                        "load_data: {path:?} must be a path inside the site"
                    )));
                }
                let full = model.root.join(&path);
                let text = match std::fs::read_to_string(&full) {
                    Ok(text) => text,
                    Err(_) if !required => return Ok(Value::Null),
                    Err(e) => return Err(err(format!("load_data: reading {path:?}: {e}"))),
                };
                let format =
                    format.unwrap_or_else(|| path.rsplit('.').next().unwrap_or("").to_owned());
                match format.as_str() {
                    "toml" => {
                        let table: toml::Table = toml::from_str(&text).map_err(|e| {
                            err(format!("load_data: {path} is not valid TOML: {e}"))
                        })?;
                        Ok(crate::util::toml_to_json(&toml::Value::Table(table)))
                    }
                    "json" => serde_json::from_str(&text)
                        .map_err(|e| err(format!("load_data: {path} is not valid JSON: {e}"))),
                    _ => Ok(Value::String(text)),
                }
            },
        );
    }
    {
        let model = Arc::clone(model);
        tera.register_function(
            "get_image_metadata",
            move |args: &HashMap<String, Value>| -> tera::Result<Value> {
                let path = require_string(args, "path", "get_image_metadata")?;
                if std::path::Path::new(&path).is_absolute() || path.split('/').any(|p| p == "..") {
                    return Err(err(format!(
                        "get_image_metadata: {path:?} must be a path inside the site"
                    )));
                }
                let info = crate::images::info(&model.root.join(&path))
                    .map_err(|e| err(format!("get_image_metadata: {e:#}")))?;
                Ok(serde_json::json!({ "width": info.width, "height": info.height }))
            },
        );
    }
    {
        let model = Arc::clone(model);
        let lang = lang.clone();
        tera.register_filter(
            "date",
            move |value: &Value, args: &HashMap<String, Value>| -> tera::Result<Value> {
                let text = match value {
                    Value::String(s) => s.clone(),
                    Value::Null => return Err(err("date filter: the value is null".to_owned())),
                    other => other.to_string(),
                };
                let lang = lang_arg(args, "date", &lang, &model)?;
                let long = arg_bool(args, "long", "date", false)?;
                let format = match arg_string(args, "format", "date")? {
                    Some(format) => format,
                    None => {
                        let language = model
                            .languages
                            .iter()
                            .find(|l| l["code"] == lang.as_str())
                            .ok_or_else(|| err(format!("no language {lang}")))?;
                        let key = if long {
                            "date_format_long"
                        } else {
                            "date_format"
                        };
                        language[key].as_str().unwrap_or("%Y-%m-%d").to_owned()
                    }
                };
                let names = &model
                    .i18n
                    .get(&lang)
                    .ok_or_else(|| err(format!("no translations for {lang}")))?
                    .dates;
                i18n::format_date(&text, &format, names)
                    .map(Value::String)
                    .map_err(|e| err(format!("date filter: {e}")))
            },
        );
    }
    tera.register_filter(
        "slugify",
        |value: &Value, args: &HashMap<String, Value>| -> tera::Result<Value> {
            let text = value
                .as_str()
                .ok_or_else(|| err("slugify: the value must be a string".to_owned()))?;
            let mode = match arg_string(args, "mode", "slugify")?.as_deref() {
                None | Some("safe") => crate::slug::SlugMode::Safe,
                Some("ascii") => crate::slug::SlugMode::Ascii,
                Some(other) => {
                    return Err(err(format!(
                        "slugify: mode must be safe or ascii, got {other:?}"
                    )));
                }
            };
            Ok(Value::String(crate::slug::slugify(text, mode)))
        },
    );
    {
        let model = Arc::clone(model);
        tera.register_filter(
            "markdown",
            move |value: &Value, args: &HashMap<String, Value>| -> tera::Result<Value> {
                let text = value
                    .as_str()
                    .ok_or_else(|| err("markdown: the value must be a string".to_owned()))?;
                let inline = arg_bool(args, "inline", "markdown", false)?;
                let rendered = crate::markdown::render(text, &model.markdown_options, &|_| None)
                    .map_err(|e| err(format!("markdown filter: {e}")))?;
                // `inline` unwraps a single paragraph; anything else (several
                // paragraphs, a heading, a list) is returned as it is.
                let trimmed = rendered.html.trim();
                let html = match (
                    inline,
                    trimmed
                        .strip_prefix("<p>")
                        .and_then(|s| s.strip_suffix("</p>")),
                ) {
                    (true, Some(inner)) if !inner.contains("<p>") => inner.to_owned(),
                    _ => rendered.html,
                };
                Ok(Value::String(html))
            },
        );
    }
    Ok(Engine { tera, lang })
}

/// A Tera error with its whole cause chain, which is where the useful part
/// (the line, the missing variable) lives.
pub fn describe(error: &tera::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

pub fn render(engine: &Engine, template: &str, context: &tera::Context) -> Result<String> {
    engine
        .tera
        .render(template, context)
        .map_err(|e| anyhow::anyhow!("{}", describe(&e)))
        .with_context(|| format!("rendering template {template} ({})", engine.lang))
}

pub fn render_str(
    engine: &mut Engine,
    body: &str,
    context: &tera::Context,
    what: &str,
) -> Result<String> {
    engine
        .tera
        .render_str(body, context)
        .map_err(|e| anyhow::anyhow!("{}", describe(&e)))
        .with_context(|| format!("rendering the template body of {what}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn string_arguments_are_typed() {
        let a = args(&[("s", json!("x")), ("n", json!(1)), ("z", Value::Null)]);
        assert_eq!(arg_string(&a, "s", "f").unwrap().as_deref(), Some("x"));
        assert_eq!(arg_string(&a, "missing", "f").unwrap(), None);
        assert_eq!(arg_string(&a, "z", "f").unwrap(), None, "null is absent");
        let error = arg_string(&a, "n", "f").unwrap_err().to_string();
        assert!(
            error.contains("f: argument `n` must be a string"),
            "{error}"
        );
        assert_eq!(require_string(&a, "s", "f").unwrap(), "x");
        let error = require_string(&a, "missing", "f").unwrap_err().to_string();
        assert!(error.contains("argument `missing` is required"), "{error}");
        let error = require_string(&a, "z", "f").unwrap_err().to_string();
        assert!(error.contains("is required"), "{error}");
    }

    #[test]
    fn boolean_arguments_have_defaults() {
        let a = args(&[("t", json!(true)), ("s", json!("true"))]);
        assert!(arg_bool(&a, "t", "f", false).unwrap());
        assert!(arg_bool(&a, "missing", "f", true).unwrap());
        assert!(!arg_bool(&a, "missing", "f", false).unwrap());
        let error = arg_bool(&a, "s", "f", false).unwrap_err().to_string();
        assert!(
            error.contains("must be a boolean"),
            "a string is not a boolean: {error}"
        );
    }

    #[test]
    fn describe_flattens_the_cause_chain() {
        let mut tera = Tera::default();
        tera.add_raw_template("t.html", "{{ missing.field }}")
            .unwrap();
        let error = tera.render("t.html", &tera::Context::new()).unwrap_err();
        let text = describe(&error);
        assert!(text.contains("t.html"), "{text}");
        assert!(
            text.contains("missing"),
            "the cause names the variable:\n{text}"
        );
        assert!(
            text.contains(": "),
            "causes are joined, not dropped:\n{text}"
        );
        let plain = describe(&tera::Error::msg("just this"));
        assert_eq!(plain, "just this");
    }

    #[test]
    fn a_parse_error_names_the_template() {
        let mut tera = Tera::default();
        let error = tera
            .add_raw_template("broken.html", "{% if %}")
            .unwrap_err();
        let text = describe(&error);
        assert!(text.contains("broken.html"), "{text}");
    }
}
