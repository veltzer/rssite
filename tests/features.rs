//! The features that replace a site's own build steps and template logic:
//! configured redirects and pager aliases, the root feed, inferred term
//! translations, required translations, the `git` context, per-language
//! date formats and locale, `x_default` and `current`, meta descriptions,
//! related pages, the search index, JSON-LD and the generated robots.txt.

mod common;

use common::*;
use std::process::Command;

// ── redirects ─────────────────────────────────────────────────────────────

#[test]
fn configured_redirects_resolve_and_check_their_targets() {
    let dir = site("redirects");
    append_config(
        &dir,
        r#"
[[redirects]]
from = "/2010/01/01/old-first/"
to = "@/blog/first.md"

[[redirects]]
from = "/2010/01/01/old-second-he/"
to = "@/blog/second.he.md#top"

[[redirects]]
from = "blog"
to = "/en/blog/"

[[redirects]]
from = "/keys/public.asc"
to = "/en/"

[[redirects]]
from = "/elsewhere/"
to = "https://example.com/x"
"#,
    );
    build_ok(&dir);
    let first = read(&dir, "_site/2010/01/01/old-first/index.html");
    assert!(
        first.contains("url=https://example.org/en/blog/first/"),
        "{first}"
    );
    assert!(
        read(&dir, "_site/2010/01/01/old-second-he/index.html")
            .contains("https://example.org/he/blog/second/#top")
    );
    assert!(read(&dir, "_site/blog/index.html").contains("url=https://example.org/en/blog/"));
    assert!(read(&dir, "_site/keys/public.asc").contains("url=https://example.org/en/"));
    assert!(read(&dir, "_site/elsewhere/index.html").contains("url=https://example.com/x"));

    let plan = plan_of(&dir);
    assert_eq!(
        sources_of(&plan, "_site/2010/01/01/old-first/index.html"),
        ["content/blog/first.en.md", "rssite.toml"]
    );
    assert_eq!(sources_of(&plan, "_site/blog/index.html"), ["rssite.toml"]);
}

#[test]
fn a_redirect_to_nothing_is_an_error() {
    expect_error(
        "redirect-missing-content",
        |dir| {
            append_config(
                dir,
                "[[redirects]]\nfrom = \"/x/\"\nto = \"@/blog/nope.md\"\n",
            )
        },
        "no content file matches \"@/blog/nope.md\"",
    );
    expect_error(
        "redirect-missing-path",
        |dir| append_config(dir, "[[redirects]]\nfrom = \"/x/\"\nto = \"/en/nope/\"\n"),
        "points at \"/en/nope/\", which this build does not write",
    );
    expect_error(
        "redirect-bad-target",
        |dir| append_config(dir, "[[redirects]]\nfrom = \"/x/\"\nto = \"en/blog/\"\n"),
        "must be @/<content path>",
    );
    expect_error(
        "redirect-over-a-page",
        |dir| {
            append_config(
                dir,
                "[[redirects]]\nfrom = \"/en/blog/first/\"\nto = \"/en/\"\n",
            )
        },
        "two outputs would be written to en/blog/first/index.html",
    );
}

#[test]
fn pager_aliases_cover_every_pager() {
    let dir = site("pager-aliases");
    let index = dir.join("content/blog/_index.en.md");
    let text = std::fs::read_to_string(&index).unwrap();
    std::fs::write(
        &index,
        text.replacen("+++\n", "+++\npager_aliases = \"/page/{n}/\"\n", 1),
    )
    .unwrap();
    build_ok(&dir);
    assert!(read(&dir, "_site/page/1/index.html").contains("url=https://example.org/en/blog/\""));
    assert!(
        read(&dir, "_site/page/2/index.html").contains("url=https://example.org/en/blog/page/2/\"")
    );
    assert!(
        !dir.join("_site/page/3").exists(),
        "four posts, two per pager"
    );
    let sources = sources_of(&plan_of(&dir), "_site/page/2/index.html");
    assert!(
        sources.contains(&"content/blog/third.en.md".to_owned()),
        "{sources:?}"
    );
}

#[test]
fn pager_aliases_need_a_placeholder_and_pagination() {
    expect_error(
        "pager-aliases-no-n",
        |dir| {
            let index = dir.join("content/blog/_index.en.md");
            let text = std::fs::read_to_string(&index).unwrap();
            std::fs::write(
                &index,
                text.replacen("+++\n", "+++\npager_aliases = \"/page/\"\n", 1),
            )
            .unwrap();
        },
        "has no {n}",
    );
    expect_error(
        "pager-aliases-unpaginated",
        |dir| {
            let index = dir.join("content/blog/_index.en.md");
            let text = std::fs::read_to_string(&index).unwrap();
            std::fs::write(
                &index,
                text.replace("paginate_by = 2\n", "pager_aliases = \"/p/{n}/\"\n"),
            )
            .unwrap();
        },
        "pager_aliases needs paginate_by",
    );
}

// ── feeds ─────────────────────────────────────────────────────────────────

#[test]
fn the_fallback_feed_is_also_written_at_the_root() {
    let dir = site("root-feed");
    edit_config(&dir, "[feeds]\n", "[feeds]\nroot = true\n");
    build_ok(&dir);
    let root = read(&dir, "_site/atom.xml");
    let english = read(&dir, "_site/en/atom.xml");
    assert_eq!(root, english, "the same feed, self link included");
    assert!(
        root.contains("href=\"https://example.org/en/atom.xml\""),
        "{root}"
    );
    let plan = plan_of(&dir);
    assert_eq!(
        sources_of(&plan, "_site/atom.xml"),
        sources_of(&plan, "_site/en/atom.xml")
    );
}

// ── term translations and required translations ──────────────────────────

#[test]
fn term_translations_can_be_inferred_from_the_pages() {
    let dir = site("infer-terms");
    edit_config(
        &dir,
        "translations = \"i18n/tags.toml\"",
        "infer_translations = true",
    );
    std::fs::remove_file(dir.join("i18n/tags.toml")).unwrap();
    build_ok(&dir);
    let term = read(&dir, "_site/en/tags/religion/index.html");
    assert!(
        term.contains("hreflang=\"he\" href=\"https://example.org/he/tags/דת/\""),
        "{term}"
    );
    let linux = read(&dir, "_site/he/tags/לינוקס/index.html");
    assert!(
        linux.contains("https://example.org/en/tags/linux/"),
        "{linux}"
    );
    let sources = sources_of(&plan_of(&dir), "_site/en/tags/religion/index.html");
    assert!(
        sources.contains(&"content/blog/first.he.md".to_owned()),
        "the pairing is read off both languages: {sources:?}"
    );
}

#[test]
fn inferred_term_translations_must_line_up() {
    expect_error(
        "infer-terms-length",
        |dir| {
            edit_config(
                dir,
                "translations = \"i18n/tags.toml\"",
                "infer_translations = true",
            );
            let path = dir.join("content/blog/second.he.md");
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(&path, text.replace("[\"דת\"]", "[\"דת\", \"לינוקס\"]")).unwrap();
        },
        "content file blog/second.en.md lists 1 tags and its translation blog/second.he.md lists 2",
    );
    expect_error(
        "infer-terms-order",
        |dir| {
            edit_config(
                dir,
                "translations = \"i18n/tags.toml\"",
                "infer_translations = true",
            );
            let path = dir.join("content/blog/first.he.md");
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(
                &path,
                text.replace("[\"דת\", \"לינוקס\"]", "[\"לינוקס\", \"דת\"]"),
            )
            .unwrap();
        },
        "are both paired with the same term in another language",
    );
    expect_error(
        "infer-terms-and-file",
        |dir| {
            edit_config(
                dir,
                "translations = \"i18n/tags.toml\"",
                "translations = \"i18n/tags.toml\"\ninfer_translations = true",
            )
        },
        "set translations or infer_translations, not both",
    );
}

#[test]
fn a_section_can_require_every_translation() {
    expect_error(
        "require-translations",
        |dir| {
            let index = dir.join("content/blog/_index.en.md");
            let text = std::fs::read_to_string(&index).unwrap();
            std::fs::write(
                &index,
                text.replacen("+++\n", "+++\nrequire_translations = true\n", 1),
            )
            .unwrap();
        },
        "content file blog/third.en.md: section blog/_index.en.md sets require_translations, but this page has no he translation",
    );
}

// ── git ───────────────────────────────────────────────────────────────────

fn git(dir: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=T", "-c", "user.email=t@example.org"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_COMMITTER_DATE", "2026-05-04T10:00:00Z")
        .env("GIT_AUTHOR_DATE", "2026-05-04T10:00:00Z")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

#[test]
fn templates_see_the_commit_and_depend_on_it() {
    let dir = site("git");
    write_probe(
        &dir,
        "{% if git %}{{ git.short }}|{{ git.date }}|{{ git.year }}|{{ git.dirty }}{% else %}none{% endif %}",
    );
    write(&dir, ".gitignore", "_site/\n");
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    build_ok(&dir);
    let probe = extract_probe(&read(&dir, "_site/en/blog/first/index.html"));
    let parts: Vec<&str> = probe.split('|').collect();
    assert_eq!(parts.len(), 4, "{probe}");
    assert!(parts[0].len() >= 7, "{probe}");
    assert_eq!(&parts[1..], ["2026-05-04", "2026", "false"]);

    let plan = plan_of(&dir);
    let page = sources_of(&plan, "_site/en/blog/first/index.html");
    for needed in [".git/HEAD", ".git/index", ".git/refs/heads/main"] {
        assert!(page.contains(&needed.to_owned()), "{needed} in {page:?}");
    }
    let listing = sources_of(&plan, "_site/en/blog/index.html");
    assert!(
        !listing.iter().any(|s| s.starts_with(".git/")),
        "section.html does not read git: {listing:?}"
    );

    write(
        &dir,
        "content/blog/first.en.md",
        &read(&dir, "content/blog/first.en.md").replace("More text", "Other text"),
    );
    build_ok(&dir);
    let dirty = extract_probe(&read(&dir, "_site/en/blog/first/index.html"));
    assert!(dirty.ends_with("|true"), "{dirty}");
}

#[test]
fn outside_git_the_commit_is_null() {
    assert_eq!(
        probe("no-git", "{% if git %}yes{% else %}none{% endif %}"),
        "none"
    );
}

// ── languages: date formats, locale, x_default, current ──────────────────

#[test]
fn a_language_sets_its_date_formats_and_locale() {
    let dir = site("date-formats");
    edit_config(
        &dir,
        "title = \"Example site\"\n",
        "title = \"Example site\"\nlocale = \"en_US\"\ndate_format = \"%-d %b %Y\"\ndate_format_long = \"%-d %B %Y\"\n",
    );
    write_probe(
        &dir,
        "{{ page.date | date }}|{{ page.date | date(long=true) }}|{{ page.date | date(format=\"%Y\") }}|{{ page.date | date(lang=\"he\") }}|{{ language.locale }}|{{ language.is_fallback }}",
    );
    build_ok(&dir);
    let probe = extract_probe(&read(&dir, "_site/en/blog/first/index.html"));
    assert_eq!(
        probe,
        "10 Jan 2026|10 January 2026|2026|2026-01-10|en_US|true"
    );
}

#[test]
fn x_default_names_the_fallback_version_and_current_marks_this_one() {
    let out = probe(
        "x-default",
        "{{ x_default }}|{% for t in translations %}{{ t.lang }}={{ t.current }};{% endfor %}|{% for t in page.translations %}{{ t.current }};{% endfor %}",
    );
    assert_eq!(
        out,
        "https://example.org/en/blog/first/|en=true;he=false;|true;false;"
    );
    let dir = site("x-default-he");
    write(
        &dir,
        "templates/page.html",
        "{% if lang == \"he\" %}<probe>{{ x_default }}</probe>{% endif %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/he/blog/first/index.html")),
        "https://example.org/en/blog/first/"
    );
    assert_eq!(
        extract_probe(&read(&dir, "_site/he/blog/רביעי/index.html")),
        "https://example.org/en/blog/renamed/"
    );
}

// ── meta descriptions, related pages, JSON-LD ────────────────────────────

#[test]
fn every_document_has_a_meta_description() {
    let dir = site("meta-description");
    write_probe(&dir, "{{ meta_description }}|{{ page.meta_description }}");
    write(
        &dir,
        "templates/section.html",
        "{% extends \"base.html\" %}{% block content %}<probe>{{ meta_description }}</probe>{% endblock %}",
    );
    write(
        &dir,
        "templates/taxonomy_list.html",
        "<probe>{{ meta_description }}</probe>",
    );
    build_ok(&dir);
    let first = extract_probe(&read(&dir, "_site/en/blog/first/index.html"));
    assert_eq!(
        first,
        "See the second post and its Hebrew version.|See the second post and its Hebrew version.",
        "the summary's text"
    );
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/tags/index.html")),
        "An example site in two languages.",
        "no description of its own: the site's"
    );
    write(
        &dir,
        "content/blog/first.en.md",
        &read(&dir, "content/blog/first.en.md").replace(
            "title = \"First post\"",
            "title = \"First post\"\ndescription = \"Mine.\"",
        ),
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "Mine.|Mine."
    );
}

#[test]
fn related_pages_share_the_most_terms() {
    let out = probe(
        "related",
        "{% for r in page.related %}{{ r.title }};{% endfor %}",
    );
    assert_eq!(out, "Third post;Second post;", "newest first among equals");
    let dir = site("related-limit");
    append_config(&dir, "[related]\nlimit = 1\n");
    write_probe(
        &dir,
        "{% for r in page.related %}{{ r.title }};{% endfor %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "Third post;"
    );
    let sources = sources_of(&plan_of(&dir), "_site/en/blog/first/index.html");
    assert!(sources.contains(&"content/blog/third.en.md".to_owned()));
    expect_error(
        "related-unknown-taxonomy",
        |dir| append_config(dir, "[related]\ntaxonomies = [\"nope\"]\n"),
        "related.taxonomies names \"nope\"",
    );
}

#[test]
fn a_page_gets_json_ld() {
    let dir = site("json-ld");
    write_probe(&dir, "{{ json_ld | safe }}");
    build_ok(&dir);
    let text = extract_probe(&read(&dir, "_site/en/blog/first/index.html"));
    let value: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(value["@type"], "BlogPosting");
    assert_eq!(value["headline"], "First post");
    assert_eq!(value["url"], "https://example.org/en/blog/first/");
    assert_eq!(value["inLanguage"], "en");
    assert_eq!(value["author"]["name"], "Example Author");
    assert_eq!(value["keywords"], "religion, linux");
    assert_eq!(
        value["workTranslation"][0]["url"],
        "https://example.org/he/blog/first/"
    );
    assert!(
        value["datePublished"]
            .as_str()
            .unwrap()
            .starts_with("2026-01-10")
    );
}

// ── search index and robots.txt ───────────────────────────────────────────

#[test]
fn the_search_index_lists_every_page_of_a_language() {
    let dir = site("search");
    append_config(&dir, "[search]\nenabled = true\n");
    build_ok(&dir);
    let index: serde_json::Value =
        serde_json::from_str(&read(&dir, "_site/en/search.json")).unwrap();
    assert_eq!(index["lang"], "en");
    let pages = index["pages"].as_array().unwrap();
    let titles: Vec<&str> = pages.iter().map(|p| p["title"].as_str().unwrap()).collect();
    assert_eq!(titles[0], "Fourth post", "newest first");
    assert!(titles.contains(&"First post"));
    let first = pages.iter().find(|p| p["title"] == "First post").unwrap();
    assert_eq!(
        first["taxonomies"]["tags"],
        serde_json::json!(["religion", "linux"])
    );
    assert_eq!(first["permalink"], "https://example.org/en/blog/first/");
    assert!(first.get("content").is_none());
    assert!(dir.join("_site/he/search.json").is_file());
    let sources = sources_of(&plan_of(&dir), "_site/en/search.json");
    assert!(sources.contains(&"content/blog/first.en.md".to_owned()));
    assert!(!sources.contains(&"content/blog/first.he.md".to_owned()));

    edit_config(&dir, "enabled = true\n", "enabled = true\ncontent = true\n");
    build_ok(&dir);
    let index: serde_json::Value =
        serde_json::from_str(&read(&dir, "_site/en/search.json")).unwrap();
    let first = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["title"] == "First post")
        .unwrap()
        .clone();
    assert!(
        first["content"]
            .as_str()
            .unwrap()
            .contains("More text with a code span.")
    );
}

#[test]
fn robots_txt_names_the_sitemap_only_when_there_is_one() {
    let dir = site("robots");
    build_ok(&dir);
    assert!(read(&dir, "_site/robots.txt").contains("Sitemap: https://example.org/sitemap.xml"));
    append_config(&dir, "[sitemap]\nenabled = false\n");
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/robots.txt"), "User-agent: *\nAllow: /\n");
}
