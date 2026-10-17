//! The site model, end to end: how pages are sorted and paired, what a
//! draft takes with it, how slugs and taxonomies are formed, what feeds and
//! the sitemap contain, and that every inconsistency is an error that names
//! the file.

mod common;

use common::*;

const POST: &str = "+++\ntitle = \"T\"\ndate = 2026-01-01\n+++\n";

// ── sorting ───────────────────────────────────────────────────────────────

fn listing_slugs(dir: &std::path::Path, rel: &str) -> Vec<String> {
    let html = read(dir, rel);
    let mut slugs = Vec::new();
    for piece in html.split("href=\"https://example.org/en/docs/").skip(1) {
        let slug = piece.split('/').next().unwrap();
        slugs.push(slug.to_owned());
    }
    slugs
}

fn docs_section(dir: &std::path::Path, sort_by: &str) {
    write(
        dir,
        "content/docs/_index.en.md",
        &format!("+++\ntitle = \"Docs\"\nsort_by = \"{sort_by}\"\n+++\n"),
    );
    undated_templates(dir);
    // Standalone, so the only links in the file are the listing's.
    write(
        dir,
        "templates/section.html",
        "{% for p in section.pages %}<a href=\"{{ p.permalink }}\">{{ p.title }}</a>{% endfor %}",
    );
}

#[test]
fn sort_by_weight_orders_ascending_and_requires_every_weight() {
    let dir = site("sort-weight");
    docs_section(&dir, "weight");
    write(
        &dir,
        "content/docs/b.en.md",
        "+++\ntitle = \"B\"\nweight = 2\n+++\n",
    );
    write(
        &dir,
        "content/docs/a.en.md",
        "+++\ntitle = \"A\"\nweight = 10\n+++\n",
    );
    write(
        &dir,
        "content/docs/c.en.md",
        "+++\ntitle = \"C\"\nweight = 2\n+++\n",
    );
    write(
        &dir,
        "content/docs/d.en.md",
        "+++\ntitle = \"D\"\nweight = -1\n+++\n",
    );
    build_ok(&dir);
    assert_eq!(
        listing_slugs(&dir, "_site/en/docs/index.html"),
        ["d", "b", "c", "a"],
        "ties break by path"
    );
    write(&dir, "content/docs/e.en.md", "+++\ntitle = \"E\"\n+++\n");
    let error = build_err(&dir);
    assert!(
        error.contains(
            "section docs/_index.en.md sorts by weight but these pages have none: docs/e.en.md"
        ),
        "{error}"
    );
}

#[test]
fn sort_by_title_and_none_are_stable() {
    let dir = site("sort-title");
    docs_section(&dir, "title");
    write(&dir, "content/docs/x.en.md", "+++\ntitle = \"beta\"\n+++\n");
    write(
        &dir,
        "content/docs/y.en.md",
        "+++\ntitle = \"Alpha\"\n+++\n",
    );
    write(
        &dir,
        "content/docs/z.en.md",
        "+++\ntitle = \"alpha\"\n+++\n",
    );
    build_ok(&dir);
    assert_eq!(
        listing_slugs(&dir, "_site/en/docs/index.html"),
        ["y", "z", "x"],
        "byte order: uppercase first"
    );
    docs_section(&dir, "none");
    build_ok(&dir);
    assert_eq!(
        listing_slugs(&dir, "_site/en/docs/index.html"),
        ["x", "y", "z"],
        "none: by file path"
    );
}

#[test]
fn sort_by_date_puts_the_newest_first_and_dates_may_carry_times() {
    let dir = site("sort-date");
    docs_section(&dir, "date");
    write(
        &dir,
        "content/docs/a.en.md",
        "+++\ntitle = \"A\"\ndate = 2026-01-01T10:00:00\n+++\n",
    );
    write(
        &dir,
        "content/docs/b.en.md",
        "+++\ntitle = \"B\"\ndate = 2026-01-01T11:00:00+02:00\n+++\n",
    );
    write(
        &dir,
        "content/docs/c.en.md",
        "+++\ntitle = \"C\"\ndate = \"2026-01-01\"\n+++\n",
    );
    write(
        &dir,
        "content/docs/d.en.md",
        "+++\ntitle = \"D\"\ndate = 2026-01-01T10:00:00\n+++\n",
    );
    build_ok(&dir);
    // b is 09:00Z, a and d are 10:00Z (tie broken by path), c is midnight.
    assert_eq!(
        listing_slugs(&dir, "_site/en/docs/index.html"),
        ["a", "d", "b", "c"]
    );
}

#[test]
fn subsections_order_by_weight_then_path() {
    let dir = site("sort-subsections");
    write(
        &dir,
        "content/z/_index.en.md",
        "+++\ntitle = \"Z\"\nweight = 1\n+++\n",
    );
    write(
        &dir,
        "content/a/_index.en.md",
        "+++\ntitle = \"A\"\nweight = 5\n+++\n",
    );
    write(&dir, "content/m/_index.en.md", "+++\ntitle = \"M\"\n+++\n");
    write(
        &dir,
        "templates/index.html",
        "{% extends \"base.html\" %}{% block content %}{% for s in section.subsections %}{{ s.title }},{% endfor %}{% endblock %}",
    );
    build_ok(&dir);
    let root = read(&dir, "_site/en/index.html");
    assert!(
        root.contains("Application,Blog,M,Z,A,"),
        "weight 0 by default, then path:\n{root}"
    );
}

// ── drafts ────────────────────────────────────────────────────────────────

#[test]
fn a_drafted_section_takes_its_pages_and_assets_with_it() {
    let dir = site("draft-section");
    write(
        &dir,
        "content/wip/_index.en.md",
        "+++\ntitle = \"WIP\"\ndraft = true\n+++\n",
    );
    write(
        &dir,
        "content/wip/_index.he.md",
        "+++\ntitle = \"בעבודה\"\ndraft = true\n+++\n",
    );
    write(&dir, "content/wip/post.en.md", POST);
    write(&dir, "content/wip/data.txt", "x");
    build_ok(&dir);
    assert!(!dir.join("_site/en/wip").exists());
    assert!(
        !dir.join("_site/he/wip").exists(),
        "the asset goes with the Hebrew draft too"
    );
    assert!(!read(&dir, "_site/sitemap.xml").contains("/wip/"));
    let plan = plan_of(&dir);
    assert!(
        !planned_paths(&plan).iter().any(|p| p.contains("/wip/")),
        "nothing of the section is planned"
    );
    let drafts = run(&dir, &["--drafts", "build"]);
    assert!(drafts.ok, "{}", drafts.stderr);
    assert!(dir.join("_site/en/wip/index.html").is_file());
    assert!(dir.join("_site/en/wip/post/index.html").is_file());
    assert!(dir.join("_site/en/wip/data.txt").is_file());
}

#[test]
fn a_draft_page_is_out_of_listings_feeds_terms_and_the_sitemap() {
    let dir = site("draft-page");
    write(
        &dir,
        "content/blog/draft.en.md",
        "+++\ntitle = \"Secret\"\ndate = 2026-09-01\ndraft = true\n[taxonomies]\ntags = [\"hidden\"]\n+++\n",
    );
    build_ok(&dir);
    for rel in [
        "_site/en/blog/index.html",
        "_site/en/atom.xml",
        "_site/sitemap.xml",
        "_site/en/tags/index.html",
        "_site/en/blog/2026/index.html",
    ] {
        assert!(
            !read(&dir, rel).contains("Secret") && !read(&dir, rel).contains("hidden"),
            "{rel}"
        );
    }
    assert!(!dir.join("_site/en/tags/hidden").exists());
    let query = run(&dir, &["query"]);
    assert!(!query.stdout.contains("draft.en.md"));
    let query = run(&dir, &["--drafts", "query"]);
    assert!(query.stdout.contains("draft.en.md"));
    let model: serde_json::Value = serde_json::from_str(&query.stdout).unwrap();
    let draft = model["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["relative_path"] == "blog/draft.en.md")
        .unwrap();
    assert_eq!(draft["draft"], true);
}

// ── sections and pages ────────────────────────────────────────────────────

#[test]
fn a_page_needs_a_section_index_in_its_language() {
    let dir = site("no-index");
    write(&dir, "content/notes/note.en.md", POST);
    let error = build_err(&dir);
    assert!(error.contains("content file notes/note.en.md: its directory has no section index for language \"en\"; add notes/_index.en.md"), "{error}");
    write(
        &dir,
        "content/notes/_index.en.md",
        "+++\ntitle = \"Notes\"\n+++\n",
    );
    build_ok(&dir);
    write(&dir, "content/notes/note.he.md", POST);
    let error = build_err(&dir);
    assert!(
        error.contains("notes/note.he.md") && error.contains("add notes/_index.he.md"),
        "an index in another language does not count:\n{error}"
    );
}

#[test]
fn the_root_section_exists_without_an_index_file() {
    let dir = site("implicit-root");
    std::fs::remove_file(dir.join("content/_index.he.md")).unwrap();
    write(
        &dir,
        "templates/index.html",
        "{% extends \"base.html\" %}{% block content %}[{{ section.title }}|{{ section.description }}|{{ section.relative_path }}|{{ section.content }}]{% endblock %}",
    );
    build_ok(&dir);
    let he = read(&dir, "_site/he/index.html");
    assert!(
        he.contains("[אתר לדוגמה|An example site in two languages.||]"),
        "the language title and the site description:\n{he}"
    );
    let en = read(&dir, "_site/en/index.html");
    assert!(en.contains("[Home|"), "{en}");
}

#[test]
fn a_root_page_lives_at_the_language_root() {
    let dir = site("root-page");
    build_ok(&dir);
    assert!(dir.join("_site/en/about/index.html").is_file());
    let about = read(&dir, "_site/en/about/index.html");
    assert!(about.contains("rel=\"canonical\" href=\"https://example.org/en/about/\""));
}

#[test]
fn nested_sections_nest_their_urls_and_parents() {
    let dir = site("nested");
    undated_templates(&dir);
    write(
        &dir,
        "content/docs/_index.en.md",
        "+++\ntitle = \"Docs\"\n+++\n",
    );
    write(
        &dir,
        "content/docs/guide/_index.en.md",
        "+++\ntitle = \"Guide\"\n+++\n",
    );
    write(
        &dir,
        "content/docs/guide/start.en.md",
        "+++\ntitle = \"Start\"\n+++\n",
    );
    write(
        &dir,
        "content/docs/guide/deep/_index.en.md",
        "+++\ntitle = \"Deep\"\n+++\n",
    );
    write(
        &dir,
        "templates/section.html",
        "{% extends \"base.html\" %}{% block content %}[{{ section.path }}|{% if section.parent %}{{ section.parent.path }}{% endif %}|{% for s in section.subsections %}{{ s.path }}{% endfor %}|{{ section.components | join(sep=\"/\") }}]{% endblock %}",
    );
    build_ok(&dir);
    assert!(dir.join("_site/en/docs/guide/start/index.html").is_file());
    assert!(
        read(&dir, "_site/en/docs/guide/index.html")
            .contains("[/en/docs/guide/|/en/docs/|/en/docs/guide/deep/|docs/guide]")
    );
    assert!(
        read(&dir, "_site/en/docs/index.html").contains("[/en/docs/|/en/|/en/docs/guide/|docs]")
    );
    // A grandchild whose parent directory has no index attaches to the
    // nearest ancestor that has one.
    write(
        &dir,
        "content/docs/other/leaf/_index.en.md",
        "+++\ntitle = \"Leaf\"\n+++\n",
    );
    build_ok(&dir);
    assert!(
        read(&dir, "_site/en/docs/other/leaf/index.html")
            .contains("[/en/docs/other/leaf/|/en/docs/|")
    );
}

#[test]
fn a_markdown_page_needs_a_title_but_an_html_page_does_not() {
    let dir = site("title");
    write(
        &dir,
        "content/blog/untitled.en.md",
        "+++\ndate = 2026-01-01\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("content file blog/untitled.en.md: front matter has no title"),
        "{error}"
    );
    std::fs::remove_file(dir.join("content/blog/untitled.en.md")).unwrap();
    write(
        &dir,
        "content/raw.en.html",
        "<p>no title, no front matter</p>",
    );
    write(
        &dir,
        "templates/page.html",
        "{{ page.title }}|{{ page.content | safe }}",
    );
    build_ok(&dir);
    assert_eq!(
        read(&dir, "_site/en/raw/index.html"),
        "|<p>no title, no front matter</p>"
    );
}

#[test]
fn a_section_index_with_a_date_is_an_error() {
    let dir = site("section-date");
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\ndate = 2026-01-01\n+++\n",
    );
    let error = build_err(&dir);
    assert!(error.contains("content file blog/_index.en.md"), "{error}");
    assert!(error.contains("a section (_index) has no date"), "{error}");
}

#[test]
fn front_matter_errors_name_the_file_and_the_key() {
    let dir = site("front-errors");
    write(
        &dir,
        "content/blog/bad.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\ntags = [\"a\"]\n+++\n",
    );
    let error = build_err(&dir);
    assert!(error.contains("content file blog/bad.en.md"), "{error}");
    assert!(error.contains("unknown field `tags`"), "{error}");
    write(
        &dir,
        "content/blog/bad.en.md",
        "+++\ntitle = \"x\"\ndate = \"2026-13-45\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("blog/bad.en.md") && error.contains("is not a date"),
        "{error}"
    );
    write(
        &dir,
        "content/blog/bad.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("blog/bad.en.md") && error.contains("not closed"),
        "{error}"
    );
}

#[test]
fn crlf_and_bom_content_builds() {
    let dir = site("crlf");
    write(
        &dir,
        "content/blog/win.en.md",
        "\u{feff}+++\r\ntitle = \"Windows\"\r\ndate = 2026-08-01\r\n+++\r\n\r\nLine one.\r\n\r\nLine two.\r\n",
    );
    build_ok(&dir);
    let page = read(&dir, "_site/en/blog/win/index.html");
    assert!(page.contains("<h1>Windows</h1>"));
    assert!(page.contains("<p>Line one.</p>"));
}

#[test]
fn aliases_work_for_sections_and_in_both_languages() {
    let dir = site("aliases");
    write(
        &dir,
        "content/blog/_index.he.md",
        "+++\ntitle = \"בלוג\"\nsort_by = \"date\"\naliases = [\"/ישן/\", \"/blog.html\"]\n+++\n",
    );
    build_ok(&dir);
    assert!(read(&dir, "_site/ישן/index.html").contains("url=https://example.org/he/blog/"));
    assert!(read(&dir, "_site/blog.html").contains("url=https://example.org/he/blog/"));
    let plan = plan_of(&dir);
    assert_eq!(
        sources_of(&plan, "_site/blog.html"),
        ["content/blog/_index.he.md", "rssite.toml"]
    );
    assert!(
        !read(&dir, "_site/sitemap.xml").contains("blog.html"),
        "aliases are not sitemap entries"
    );
}

#[test]
fn an_alias_colliding_with_a_page_is_an_error() {
    let dir = site("alias-collision");
    write(
        &dir,
        "content/blog/x.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\naliases = [\"/en/blog/first/\"]\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("two outputs would be written to en/blog/first/index.html"),
        "{error}"
    );
    assert!(
        error.contains("an alias of https://example.org/en/blog/x/"),
        "{error}"
    );
    assert!(error.contains("page blog/first.en.md"), "{error}");
}

// ── translations ──────────────────────────────────────────────────────────

#[test]
fn two_files_claiming_the_same_translation_is_an_error() {
    let dir = site("dup-translation");
    write(
        &dir,
        "content/blog/other.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\ntranslation_key = \"first\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("are both the \"en\" translation of \"blog/first\""),
        "{error}"
    );
    assert!(
        error.contains("blog/first.en.md") && error.contains("blog/other.en.md"),
        "{error}"
    );
}

#[test]
fn translation_keys_are_scoped_to_the_directory() {
    let dir = site("key-scope");
    undated_templates(&dir);
    write(
        &dir,
        "content/notes/_index.en.md",
        "+++\ntitle = \"N\"\n+++\n",
    );
    write(
        &dir,
        "content/notes/first.en.md",
        "+++\ntitle = \"Another first\"\n+++\n",
    );
    build_ok(&dir);
    let query = run(&dir, &["query"]);
    let model: serde_json::Value = serde_json::from_str(&query.stdout).unwrap();
    let note = model["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["relative_path"] == "notes/first.en.md")
        .unwrap();
    assert_eq!(
        note["translations"].as_array().unwrap().len(),
        1,
        "not paired with blog/first.he.md"
    );
}

#[test]
fn body_from_misuse_is_an_error() {
    let dir = site("body-from");
    let cases = [
        (
            "+++\ntitle = \"x\"\ndate = 2026-01-01\nbody_from = \"en\"\n+++\n",
            "body_from names its own language",
        ),
        (
            "+++\ntitle = \"x\"\ndate = 2026-01-01\nbody_from = \"he\"\n+++\n",
            "body_from = \"he\" but this page has no \"he\" translation",
        ),
        (
            "+++\ntitle = \"x\"\ndate = 2026-01-01\nbody_from = \"fr\"\n+++\n",
            "no \"fr\" translation",
        ),
    ];
    for (front, needle) in cases {
        write(&dir, "content/blog/third.en.md", front);
        let error = build_err(&dir);
        assert!(error.contains("content file blog/third.en.md"), "{error}");
        assert!(error.contains(needle), "{error}");
    }
    // Both a body and body_from.
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Third post\"\ndate = 2026-03-15\n+++\n",
    );
    write(
        &dir,
        "content/blog/third.he.md",
        "+++\ntitle = \"שלישי\"\ndate = 2026-03-15\nbody_from = \"en\"\n+++\n\nיש גוף.\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("blog/third.he.md: has both a body and body_from"),
        "{error}"
    );
    // A chain.
    write(
        &dir,
        "content/blog/third.he.md",
        "+++\ntitle = \"שלישי\"\ndate = 2026-03-15\nbody_from = \"en\"\n+++\n",
    );
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Third post\"\ndate = 2026-03-15\nbody_from = \"he\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(error.contains("which itself uses body_from"), "{error}");
}

#[test]
fn body_from_renders_the_other_body_in_this_language() {
    let dir = site("body-from-ok");
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Third post\"\ndate = 2026-03-15\n+++\n\nShared [link](@/blog/first.md) and {{ lang }}.\n",
    );
    write(
        &dir,
        "content/blog/third.he.md",
        "+++\ntitle = \"שלישי\"\ndate = 2026-03-15\nbody_from = \"en\"\n+++\n",
    );
    build_ok(&dir);
    let he = read(&dir, "_site/he/blog/third/index.html");
    assert!(
        he.contains("<h1>שלישי</h1>"),
        "the Hebrew front matter:\n{he}"
    );
    assert!(
        he.contains("href=\"https://example.org/he/blog/first/\""),
        "links resolve in Hebrew:\n{he}"
    );
    assert!(he.contains("{{ lang }}"), "markdown is not a template");
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/he/blog/third/index.html");
    assert!(
        sources.contains(&"content/blog/third.en.md".to_owned()),
        "{sources:?}"
    );
    assert!(
        sources.contains(&"content/blog/first.he.md".to_owned()),
        "the Hebrew link target:\n{sources:?}"
    );
}

// ── slugs ─────────────────────────────────────────────────────────────────

#[test]
fn slugs_come_from_the_file_name_or_the_front_matter() {
    let dir = site("slugs");
    undated_templates(&dir);
    write(
        &dir,
        "content/blog/My Post (2).en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\n+++\n",
    );
    write(
        &dir,
        "content/blog/custom.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\nslug = \"Chosen Slug!\"\n+++\n",
    );
    write(
        &dir,
        "content/Some Dir/_index.en.md",
        "+++\ntitle = \"d\"\n+++\n",
    );
    write(
        &dir,
        "content/Some Dir/p.en.md",
        "+++\ntitle = \"p\"\n+++\n",
    );
    build_ok(&dir);
    assert!(dir.join("_site/en/blog/my-post-2/index.html").is_file());
    assert!(dir.join("_site/en/blog/chosen-slug/index.html").is_file());
    assert!(dir.join("_site/en/some-dir/p/index.html").is_file());
}

#[test]
fn ascii_slugs_transliterate_paths_and_terms() {
    let dir = site("slugs-ascii");
    append_config(
        &dir,
        "[slugify]\npaths = \"ascii\"\ntaxonomies = \"ascii\"\nanchors = \"ascii\"",
    );
    write(
        &dir,
        "content/blog/ascii.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\nslug = \"Café\"\n[taxonomies]\ntags = [\"Ünïcode\"]\n+++\n\n## Résumé\n",
    );
    build_ok(&dir);
    assert!(dir.join("_site/en/blog/cafe/index.html").is_file());
    assert!(dir.join("_site/en/tags/unicode/index.html").is_file());
    assert!(read(&dir, "_site/en/blog/cafe/index.html").contains("id=\"resume\""));
    // Hebrew paths transliterate too, so the fixture's Hebrew slug moves.
    assert!(!dir.join("_site/he/blog/רביעי").exists());
    let plan = plan_of(&dir);
    assert!(
        planned_paths(&plan)
            .iter()
            .any(|p| p.starts_with("_site/he/tags/") && p.is_ascii()),
        "{:?}",
        planned_paths(&plan)
    );
}

#[test]
fn a_name_with_no_url_characters_is_an_error() {
    let dir = site("slug-empty");
    write(&dir, "content/blog/---.en.md", POST);
    let error = build_err(&dir);
    assert!(
        error.contains(
            "content file blog/---.en.md: its name has no characters usable in a URL; set `slug`"
        ),
        "{error}"
    );
    std::fs::remove_file(dir.join("content/blog/---.en.md")).unwrap();
    write(
        &dir,
        "content/blog/x.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\nslug = \"!!!\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("blog/x.en.md") && error.contains("no characters usable"),
        "{error}"
    );
    std::fs::remove_file(dir.join("content/blog/x.en.md")).unwrap();
    write(
        &dir,
        "content/!!!/_index.en.md",
        "+++\ntitle = \"x\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("content directory \"!!!\" has no characters usable in a URL"),
        "{error}"
    );
}

#[test]
fn slugs_that_differ_only_in_case_collide() {
    let dir = site("slug-case");
    write(&dir, "content/blog/First.en.md", POST);
    let error = build_err(&dir);
    assert!(error.contains("both render to /en/blog/first/"), "{error}");
}

// ── taxonomies ────────────────────────────────────────────────────────────

#[test]
fn terms_colliding_after_slugification_are_an_error() {
    let dir = site("term-collision");
    write(
        &dir,
        "content/blog/x.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\n[taxonomies]\ntags = [\"Linux\"]\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains(
            "taxonomy tags: terms \"Linux\" and \"linux\" both slugify to \"linux\" in language en"
        ),
        "{error}"
    );
}

#[test]
fn an_empty_term_is_an_error() {
    let dir = site("term-empty");
    write(
        &dir,
        "content/blog/x.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\n[taxonomies]\ntags = [\" \"]\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("content file blog/x.en.md: taxonomy \"tags\" has an empty term"),
        "{error}"
    );
    write(
        &dir,
        "content/blog/x.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\n[taxonomies]\ntags = [\"!!\"]\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("taxonomy tags: term \"!!\" has no characters usable in a URL"),
        "{error}"
    );
}

#[test]
fn a_term_lists_its_pages_newest_first_once_each() {
    let dir = site("term-pages");
    write(
        &dir,
        "content/blog/x.en.md",
        "+++\ntitle = \"Twice\"\ndate = 2026-12-01\n[taxonomies]\ntags = [\"religion\", \"religion\"]\n+++\n",
    );
    append_config(&dir, "");
    edit_config(&dir, "paginate_by = 2\nfeed = true", "feed = true");
    build_ok(&dir);
    let page = read(&dir, "_site/en/tags/religion/index.html");
    assert_eq!(page.matches("Twice").count(), 1, "{page}");
    let positions: Vec<usize> = ["Twice", "Third post", "Second post", "First post"]
        .iter()
        .map(|t| page.find(t).unwrap())
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
    let list = read(&dir, "_site/en/tags/index.html");
    assert!(list.contains("religion</a> 4"), "{list}");
}

#[test]
fn term_translation_tables_are_checked() {
    let dir = site("term-translations");
    write(
        &dir,
        "i18n/tags.toml",
        "[[terms]]\nen = \"religion\"\nfr = \"religion\"\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("taxonomy tags translations: \"fr\" is not a configured language"),
        "{error}"
    );
    write(
        &dir,
        "i18n/tags.toml",
        "[[terms]]\nen = \"religion\"\nhe = \"דת\"\n[[terms]]\nen = \"religion\"\nhe = \"לינוקס\"\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("term \"religion\" appears in two rows"),
        "{error}"
    );
    write(
        &dir,
        "i18n/tags.toml",
        "[[terms]]\nen = \"religion\"\nhe = \"דת\"\nnote = \"x\"\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("\"note\" is not a configured language"),
        "every key of a row is a language:\n{error}"
    );
    write(
        &dir,
        "i18n/tags.toml",
        "[[terms]]\nen = \"religion\"\nhe = \"דת\"\n[[terms]]\nen = \"absent\"\nhe = \"גם\"\n",
    );
    build_ok(&dir);
    let linux = read(&dir, "_site/en/tags/linux/index.html");
    assert!(
        !linux.contains("rel=\"alternate\" hreflang=\"he\""),
        "an unpaired term has no alternate:\n{linux}"
    );
    write(
        &dir,
        "i18n/tags.toml",
        "[[terms]]\nen = \"religion\"\n[[nope]]\n",
    );
    let error = build_err(&dir);
    assert!(error.contains("i18n/tags.toml"), "{error}");
}

#[test]
fn a_missing_translations_file_is_an_error_naming_it() {
    let dir = site("term-translations-missing");
    std::fs::remove_file(dir.join("i18n/tags.toml")).unwrap();
    let error = build_err(&dir);
    assert!(error.contains("i18n/tags.toml"), "{error}");
}

#[test]
fn taxonomies_exist_per_language_even_when_empty() {
    let dir = site("tax-empty-lang");
    append_config(&dir, "[[taxonomies]]\nname = \"series\"");
    build_ok(&dir);
    assert!(dir.join("_site/en/series/index.html").is_file());
    assert!(dir.join("_site/he/series/index.html").is_file());
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/en/series/index.html");
    assert!(sources.contains(&"templates/taxonomy_list.html".to_owned()));
    assert!(
        sources.contains(&"content/blog/first.en.md".to_owned()),
        "every page of the language can gain a term"
    );
    assert!(!sources.contains(&"content/blog/first.he.md".to_owned()));
}

#[test]
fn zero_pagination_is_an_error() {
    let dir = site("paginate-zero");
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\npaginate_by = 0\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("section blog/_index.en.md: paginate_by must be at least 1"),
        "{error}"
    );
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\n+++\n",
    );
    edit_config(&dir, "paginate_by = 2", "paginate_by = 0");
    let error = build_err(&dir);
    assert!(
        error.contains("taxonomy tags: paginate_by must be at least 1"),
        "{error}"
    );
}

#[test]
fn a_custom_paginate_path_is_used_everywhere() {
    let dir = site("paginate-path");
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\npaginate_by = 1\npaginate_path = \"p\"\n+++\n",
    );
    edit_config(
        &dir,
        "paginate_by = 2",
        "paginate_by = 1\npaginate_path = \"seite\"",
    );
    build_ok(&dir);
    assert!(dir.join("_site/en/blog/p/4/index.html").is_file());
    assert!(!dir.join("_site/en/blog/page").exists());
    assert!(
        dir.join("_site/en/tags/religion/seite/3/index.html")
            .is_file()
    );
    let second = read(&dir, "_site/en/blog/p/2/index.html");
    assert!(
        second.contains("rel=\"canonical\" href=\"https://example.org/en/blog/p/2/\""),
        "{second}"
    );
    assert!(second.contains("rel=\"prev\" href=\"https://example.org/en/blog/\""));
    assert!(second.contains("rel=\"next\" href=\"https://example.org/en/blog/p/3/\""));
    assert!(
        read(&dir, "_site/sitemap.xml").contains("https://example.org/en/tags/religion/seite/2/")
    );
}

#[test]
fn an_empty_paginated_listing_is_one_page() {
    let dir = site("paginate-empty");
    write(
        &dir,
        "content/empty/_index.en.md",
        "+++\ntitle = \"Empty\"\npaginate_by = 5\n+++\n",
    );
    build_ok(&dir);
    let page = read(&dir, "_site/en/empty/index.html");
    assert!(page.contains("1/1"), "{page}");
    assert!(!dir.join("_site/en/empty/page").exists());
}

// ── feeds ─────────────────────────────────────────────────────────────────

#[test]
fn feeds_are_opt_out_per_language_and_opt_in_per_section() {
    let dir = site("feeds-switches");
    edit_config(
        &dir,
        "title = \"אתר לדוגמה\"",
        "title = \"אתר לדוגמה\"\ngenerate_feeds = false",
    );
    write(
        &dir,
        "content/notes/_index.en.md",
        "+++\ntitle = \"Notes\"\n+++\n",
    );
    write(&dir, "content/notes/a.en.md", POST);
    build_ok(&dir);
    assert!(dir.join("_site/en/atom.xml").is_file());
    assert!(!dir.join("_site/he/atom.xml").exists());
    assert!(
        dir.join("_site/he/blog/atom.xml").is_file(),
        "a section's own flag is independent"
    );
    assert!(
        !dir.join("_site/en/notes/atom.xml").exists(),
        "sections default to no feed"
    );
    assert!(
        !read(&dir, "_site/he/blog/first/index.html").contains("application/atom+xml"),
        "fixture base.html has no feed link"
    );
}

#[test]
fn feeds_are_a_window_of_dated_pages_newest_first() {
    let dir = site("feeds-window");
    undated_templates(&dir);
    edit_config(&dir, "limit = 2", "limit = 3\nfilename = \"feed.xml\"");
    write(
        &dir,
        "content/undated.en.md",
        "+++\ntitle = \"Undated\"\n+++\n",
    );
    build_ok(&dir);
    assert!(!dir.join("_site/en/atom.xml").exists());
    let feed = read(&dir, "_site/en/feed.xml");
    assert_eq!(feed.matches("<entry").count(), 3);
    assert!(!feed.contains("Undated"));
    let order: Vec<usize> = ["Fourth post", "Third post", "Second post"]
        .iter()
        .map(|t| feed.find(t).unwrap())
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]));
    assert!(
        feed.contains("<updated>2026-04-01T00:00:00+00:00</updated>"),
        "the newest change:\n{feed}"
    );
    assert!(dir.join("_site/en/blog/feed.xml").is_file());
    assert!(dir.join("_site/en/tags/religion/feed.xml").is_file());
    let term_feed = read(&dir, "_site/en/tags/religion/feed.xml");
    assert_eq!(term_feed.matches("<entry").count(), 3);
    assert!(term_feed.contains("<title>Example - religion</title>"));
    assert!(term_feed.contains("href=\"https://example.org/en/tags/religion/feed.xml\""));
}

#[test]
fn a_feed_of_a_section_without_dates_is_empty_not_an_error() {
    let dir = site("feeds-undated");
    undated_templates(&dir);
    write(
        &dir,
        "content/notes/_index.en.md",
        "+++\ntitle = \"Notes\"\ngenerate_feeds = true\n+++\n",
    );
    write(&dir, "content/notes/a.en.md", "+++\ntitle = \"A\"\n+++\n");
    build_ok(&dir);
    let feed = read(&dir, "_site/en/notes/atom.xml");
    assert_eq!(feed.matches("<entry").count(), 0);
    assert!(!feed.contains("<updated>"), "{feed}");
    assert!(feed.contains("<title>Example - Notes</title>"));
}

// ── sitemap and robots ────────────────────────────────────────────────────

#[test]
fn the_sitemap_can_be_disabled_and_robots_can_be_replaced() {
    let dir = site("sitemap-off");
    append_config(&dir, "[sitemap]\nenabled = false");
    write(
        &dir,
        "static/robots.txt",
        "User-agent: *\nDisallow: /private/\n",
    );
    build_ok(&dir);
    assert!(!dir.join("_site/sitemap.xml").exists());
    assert_eq!(
        read(&dir, "_site/robots.txt"),
        "User-agent: *\nDisallow: /private/\n"
    );
    let plan = plan_of(&dir);
    assert_eq!(sources_of(&plan, "_site/robots.txt"), ["static/robots.txt"]);
    assert!(!planned_paths(&plan).contains(&"_site/sitemap.xml".to_owned()));
}

#[test]
fn the_generated_robots_points_at_the_sitemap() {
    let dir = site("robots");
    build_ok(&dir);
    assert_eq!(
        read(&dir, "_site/robots.txt"),
        "User-agent: *\nAllow: /\n\nSitemap: https://example.org/sitemap.xml\n"
    );
    let plan = plan_of(&dir);
    assert_eq!(sources_of(&plan, "_site/robots.txt"), ["rssite.toml"]);
}

#[test]
fn the_sitemap_pairs_translations_and_dates_sections() {
    let dir = site("sitemap-pairs");
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\nupdated = 2026-05-05\n+++\n",
    );
    build_ok(&dir);
    let sitemap = read(&dir, "_site/sitemap.xml");
    assert!(
        sitemap
            .contains("<loc>https://example.org/en/blog/</loc>\n    <lastmod>2026-05-05</lastmod>"),
        "{sitemap}"
    );
    assert!(sitemap.contains("<loc>https://example.org/en/blog/first/</loc>\n    <lastmod>2026-01-10</lastmod>\n    <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"https://example.org/en/blog/first/\"/>\n    <xhtml:link rel=\"alternate\" hreflang=\"he\" href=\"https://example.org/he/blog/first/\"/>"), "{sitemap}");
    assert!(sitemap.contains("<loc>https://example.org/en/tags/religion/</loc>\n    <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"https://example.org/en/tags/religion/\"/>\n    <xhtml:link rel=\"alternate\" hreflang=\"he\" href=\"https://example.org/he/tags/דת/\"/>"), "{sitemap}");
    let locs: Vec<&str> = sitemap
        .split("<loc>")
        .skip(1)
        .map(|s| s.split("</loc>").next().unwrap())
        .collect();
    let mut sorted = locs.clone();
    sorted.sort_unstable();
    assert_eq!(locs, sorted, "entries are sorted");
    let unique: std::collections::BTreeSet<&&str> = locs.iter().collect();
    assert_eq!(unique.len(), locs.len(), "no duplicates");
}

// ── assets, static files, sass ────────────────────────────────────────────

#[test]
fn content_assets_are_copied_into_every_language() {
    let dir = site("assets");
    write(&dir, "content/pic.png", "root asset");
    write(&dir, "content/blog/deep/chart.svg", "<svg/>");
    build_ok(&dir);
    for lang in ["en", "he"] {
        assert_eq!(read(&dir, &format!("_site/{lang}/pic.png")), "root asset");
        assert_eq!(
            read(&dir, &format!("_site/{lang}/blog/deep/chart.svg")),
            "<svg/>"
        );
    }
    let plan = plan_of(&dir);
    assert_eq!(sources_of(&plan, "_site/he/pic.png"), ["content/pic.png"]);
}

#[test]
fn static_files_keep_their_paths_and_a_collision_is_an_error() {
    let dir = site("static");
    write(&dir, "static/js/app.js", "console.log(1)");
    write(&dir, "static/.well-known/x", "y");
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/js/app.js"), "console.log(1)");
    assert_eq!(read(&dir, "_site/.well-known/x"), "y");
    write(&dir, "static/en/index.html", "shadow");
    let error = build_err(&dir);
    assert!(
        error.contains("two outputs would be written to en/index.html"),
        "{error}"
    );
    assert!(
        error.contains("section _index.en.md")
            && error.contains("static file static/en/index.html"),
        "{error}"
    );
}

#[test]
fn sass_compiles_every_non_partial_and_reports_errors() {
    let dir = site("sass");
    write(
        &dir,
        "sass/print.scss",
        "@use \"vars\";\n@media print { body { width: vars.$width; } }\n",
    );
    write(
        &dir,
        "sass/nested/extra.scss",
        ".x { .y { color: red; } }\n",
    );
    write(&dir, "sass/_mixins.scss", "@mixin m { margin: 0; }\n");
    write(&dir, "sass/legacy.sass", ".a\n  color: blue\n");
    write(&dir, "sass/notes.txt", "ignored");
    build_ok(&dir);
    assert!(read(&dir, "_site/print.css").contains("40rem"));
    assert!(read(&dir, "_site/nested/extra.css").contains(".x .y"));
    assert!(read(&dir, "_site/legacy.css").contains("blue"));
    assert!(!dir.join("_site/_mixins.css").exists());
    assert!(!dir.join("_site/notes.txt").exists());
    assert!(!dir.join("_site/notes.css").exists());
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/print.css");
    assert!(
        sources.contains(&"sass/_vars.scss".to_owned())
            && sources.contains(&"sass/_mixins.scss".to_owned()),
        "every sass file is a source of every stylesheet:\n{sources:?}"
    );
    write(&dir, "sass/broken.scss", ".a { color: ; }\n");
    let error = build_err(&dir);
    assert!(error.contains("compiling sass/broken.scss"), "{error}");
}

// ── markdown options ──────────────────────────────────────────────────────

#[test]
fn smart_punctuation_can_be_set_per_site_page_or_section() {
    let dir = site("smart");
    write(
        &dir,
        "content/blog/q.en.md",
        "+++\ntitle = \"q\"\ndate = 2026-01-01\n+++\n\n\"quoted\"\n",
    );
    write(
        &dir,
        "content/blog/s.en.md",
        "+++\ntitle = \"s\"\ndate = 2026-01-01\nsmart_punctuation = true\n+++\n\n\"quoted\"\n",
    );
    build_ok(&dir);
    assert!(read(&dir, "_site/en/blog/q/index.html").contains("<p>\"quoted\"</p>"));
    assert!(read(&dir, "_site/en/blog/s/index.html").contains("<p>“quoted”</p>"));
    append_config(&dir, "[markdown]\nsmart_punctuation = true");
    write(
        &dir,
        "content/blog/n.en.md",
        "+++\ntitle = \"n\"\ndate = 2026-01-01\nsmart_punctuation = false\n+++\n\n\"quoted\"\n",
    );
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\nsmart_punctuation = true\n+++\n\n\"section\"\n",
    );
    build_ok(&dir);
    assert!(
        read(&dir, "_site/en/blog/q/index.html").contains("<p>“quoted”</p>"),
        "the site default"
    );
    assert!(
        read(&dir, "_site/en/blog/n/index.html").contains("<p>\"quoted\"</p>"),
        "the page override"
    );
}

#[test]
fn highlighting_can_be_turned_off_and_needs_a_known_theme() {
    let dir = site("highlight");
    write(
        &dir,
        "content/blog/code.en.md",
        "+++\ntitle = \"c\"\ndate = 2026-01-01\n+++\n\n```rust\nfn x() {}\n```\n",
    );
    build_ok(&dir);
    assert!(dir.join("_site/highlight.css").is_file());
    assert!(read(&dir, "_site/en/blog/code/index.html").contains("<pre class=\"highlight\">"));
    append_config(&dir, "[markdown]\nhighlight = false");
    build_ok(&dir);
    assert!(!planned_paths(&plan_of(&dir)).contains(&"_site/highlight.css".to_owned()));
    assert!(
        read(&dir, "_site/en/blog/code/index.html")
            .contains("<pre><code class=\"language-rust\">fn x() {}\n</code></pre>")
    );
    append_config(&dir, "");
    edit_config(
        &dir,
        "highlight = false",
        "highlight = true\nhighlight_theme = \"nope\"",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("markdown.highlight_theme \"nope\" is not a known theme"),
        "{error}"
    );
    edit_config(
        &dir,
        "highlight_theme = \"nope\"",
        "highlight_theme = \"base16-ocean.dark\"",
    );
    build_ok(&dir);
    assert!(
        read(&dir, "_site/highlight.css").contains("2b303b"),
        "the ocean theme's background"
    );
}

// ── single language and prefixes ──────────────────────────────────────────

const SINGLE: &str = "base_url = \"https://one.example\"\ntitle = \"One\"\n[[languages]]\ncode = \"en\"\nname = \"English\"\n";

fn single_language_site(name: &str) -> std::path::PathBuf {
    let dir = temp_dir(name);
    write(&dir, "rssite.toml", SINGLE);
    write(&dir, "templates/index.html", "root {{ section.title }}");
    write(
        &dir,
        "templates/section.html",
        "section {{ section.title }} {% for p in section.pages %}{{ p.title }} {{ p.date | date(format=\"%B\") }};{% endfor %}",
    );
    write(
        &dir,
        "templates/page.html",
        "page {{ page.title }} {{ trans(key=\"x\") }}",
    );
    write(&dir, "content/_index.md", "+++\ntitle = \"Home\"\n+++\n");
    write(
        &dir,
        "content/blog/_index.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\n+++\n",
    );
    write(
        &dir,
        "content/blog/a.md",
        "+++\ntitle = \"A\"\ndate = 2026-03-01\n+++\n",
    );
    write(
        &dir,
        "content/blog/b.en.md",
        "+++\ntitle = \"B\"\ndate = 2026-04-01\n+++\n",
    );
    dir
}

#[test]
fn a_single_language_site_needs_no_suffixes_and_no_i18n_directory() {
    let dir = single_language_site("single");
    write(&dir, "templates/page.html", "page {{ page.title }}");
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/en/index.html"), "root Home");
    assert_eq!(
        read(&dir, "_site/en/blog/index.html"),
        "section Blog B April;A March;"
    );
    assert_eq!(read(&dir, "_site/en/blog/a/index.html"), "page A");
    assert_eq!(
        read(&dir, "_site/en/blog/b/index.html"),
        "page B",
        "a suffix is still allowed"
    );
    assert!(
        read(&dir, "_site/index.html").contains("url=https://one.example/en/"),
        "the root redirects without a root.html"
    );
    assert!(dir.join("_site/404.html").is_file());
    assert!(dir.join("_site/en/404.html").is_file());
    assert!(dir.join("_site/en/atom.xml").is_file());
    assert!(
        !dir.join("_site/style.css").exists(),
        "no sass, no stylesheet"
    );
    assert!(!dir.join("_site/highlight.css").exists() || true);
    let error = {
        write(&dir, "templates/page.html", "page {{ trans(key=\"x\") }}");
        build_err(&dir)
    };
    assert!(
        error.contains(
            "translation key \"x\" is missing for language \"en\" in i18n/en.toml (no such file)"
        ),
        "{error}"
    );
}

#[test]
fn a_language_can_own_a_custom_prefix() {
    let dir = site("prefix");
    edit_config(
        &dir,
        "name = \"English\"",
        "name = \"English\"\nurl_prefix = \"/english/\"",
    );
    build_ok(&dir);
    assert!(dir.join("_site/english/blog/first/index.html").is_file());
    assert!(!dir.join("_site/en").exists());
    let first = read(&dir, "_site/english/blog/first/index.html");
    assert!(first.contains("href=\"https://example.org/english/blog/second/\""));
    assert!(first.contains("href=\"https://example.org/english/tags/religion/\""));
    assert!(first.contains("hreflang=\"he\" href=\"https://example.org/he/blog/first/\""));
    assert!(read(&dir, "_site/index.html").contains("href=\"/english/\""));
    assert!(read(&dir, "_site/sitemap.xml").contains("https://example.org/english/"));
    assert!(dir.join("_site/english/atom.xml").is_file());
    assert!(dir.join("_site/english/404.html").is_file());
}

#[test]
fn the_fallback_language_drives_the_root_redirect_and_global_404() {
    let dir = site("fallback");
    edit_config(
        &dir,
        "fallback_language = \"en\"",
        "fallback_language = \"he\"",
    );
    std::fs::remove_file(dir.join("templates/root.html")).unwrap();
    build_ok(&dir);
    assert!(read(&dir, "_site/index.html").contains("url=https://example.org/he/"));
    assert!(read(&dir, "_site/404.html").contains("<html lang=\"he\" dir=\"rtl\">"));
    let plan = plan_of(&dir);
    assert_eq!(sources_of(&plan, "_site/index.html"), ["rssite.toml"]);
}

#[test]
fn the_root_template_sees_the_fallback_language_context() {
    let dir = site("root-template");
    write(
        &dir,
        "templates/root.html",
        "{{ lang }}|{{ language.name }}|{{ languages | length }}|{{ current_path }}|{{ config.title }}|{{ translations | length }}",
    );
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/index.html"), "en|English|2|/|Example|0");
    let plan = plan_of(&dir);
    assert_eq!(
        sources_of(&plan, "_site/index.html"),
        ["i18n/en.toml", "rssite.toml", "templates/root.html"]
    );
}

// ── year archives ─────────────────────────────────────────────────────────

#[test]
fn year_archives_are_opt_in_and_span_years() {
    let dir = site("years");
    write(
        &dir,
        "content/blog/old.en.md",
        "+++\ntitle = \"Old\"\ndate = 2024-06-01\n+++\n",
    );
    write(
        &dir,
        "content/blog/older.en.md",
        "+++\ntitle = \"Older\"\ndate = 2024-01-01\n+++\n",
    );
    build_ok(&dir);
    let archive = read(&dir, "_site/en/blog/2024/index.html");
    assert!(
        archive.contains("Old") && archive.contains("Older") && !archive.contains("First post")
    );
    assert!(
        archive.contains("2026 2024*"),
        "years newest first, this one marked:\n{archive}"
    );
    assert!(
        !archive.contains("rel=\"alternate\" hreflang=\"he\""),
        "Hebrew has no 2024 posts, so no alternate:\n{archive}"
    );
    assert!(
        archive.contains("class=\"lang\" href=\"/he/\""),
        "the switcher falls back to the Hebrew home:\n{archive}"
    );
    let blog = read(&dir, "_site/en/blog/index.html");
    assert!(
        blog.contains(">2026 (4)</a>") && blog.contains(">2024 (2)</a>"),
        "{blog}"
    );
    let archive_2026 = read(&dir, "_site/en/blog/2026/index.html");
    assert!(archive_2026.contains("hreflang=\"he\" href=\"https://example.org/he/blog/2026/\""));
    assert!(archive_2026.contains("rel=\"canonical\" href=\"https://example.org/en/blog/2026/\""));
    let sitemap = read(&dir, "_site/sitemap.xml");
    assert!(sitemap.contains("<loc>https://example.org/en/blog/2024/</loc>"));
    assert!(sitemap.contains("<loc>https://example.org/en/blog/2026/</loc>\n    <xhtml:link rel=\"alternate\" hreflang=\"en\" href=\"https://example.org/en/blog/2026/\"/>\n    <xhtml:link rel=\"alternate\" hreflang=\"he\" href=\"https://example.org/he/blog/2026/\"/>"), "{sitemap}");
    // Off by default.
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\n+++\n",
    );
    build_ok(&dir);
    assert!(
        !planned_paths(&plan_of(&dir))
            .iter()
            .any(|p| p.starts_with("_site/en/blog/20"))
    );
    assert!(!read(&dir, "_site/en/blog/index.html").contains("class=\"year\""));
}

#[test]
fn year_archives_use_their_template_and_depend_on_the_whole_language() {
    let dir = site("years-template");
    write(
        &dir,
        "templates/by-year.html",
        "YEAR {{ year }} {{ pages | length }} {{ section.title }} {{ years | length }}",
    );
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\nyear_archives = true\narchive_template = \"by-year.html\"\n+++\n",
    );
    build_ok(&dir);
    assert_eq!(
        read(&dir, "_site/en/blog/2026/index.html"),
        "YEAR 2026 4 Blog 1"
    );
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/en/blog/2026/index.html");
    assert!(
        sources.contains(&"templates/by-year.html".to_owned()),
        "{sources:?}"
    );
    assert!(
        sources.contains(&"content/blog/third.en.md".to_owned())
            && sources.contains(&"content/about.en.tera".to_owned()),
        "{sources:?}"
    );
    assert!(
        !sources.contains(&"content/blog/first.he.md".to_owned()),
        "{sources:?}"
    );
    std::fs::remove_file(dir.join("templates/by-year.html")).unwrap();
    let error = build_err(&dir);
    assert!(error.contains("by-year.html"), "{error}");
}

#[test]
fn year_archives_without_dated_pages_are_none() {
    let dir = site("years-none");
    undated_templates(&dir);
    write(
        &dir,
        "content/notes/_index.en.md",
        "+++\ntitle = \"Notes\"\nyear_archives = true\n+++\n",
    );
    write(&dir, "content/notes/a.en.md", "+++\ntitle = \"A\"\n+++\n");
    build_ok(&dir);
    assert!(
        !planned_paths(&plan_of(&dir))
            .iter()
            .any(|p| p.starts_with("_site/en/notes/20"))
    );
}

#[test]
fn a_language_with_root_url_prefix_builds_without_colliding() {
    let dir = single_language_site("root-prefix");
    edit_config(
        &dir,
        "name = \"English\"",
        "name = \"English\"\nurl_prefix = \"/\"",
    );
    write(&dir, "templates/page.html", "page {{ page.title }}");
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/index.html"), "root Home");
    assert_eq!(read(&dir, "_site/blog/a/index.html"), "page A");
    assert!(dir.join("_site/404.html").is_file());
    assert!(dir.join("_site/atom.xml").is_file());
}

#[test]
fn section_translation_key_root_feeds_drafts_body_from_and_duplicate_indexes_are_checked() {
    // 1. Section translation_key pairs sections across directory names and rejects duplicates.
    let dir = site("section-trans-key");
    write(
        &dir,
        "content/notes/_index.en.md",
        "+++\ntitle = \"Notes\"\ntranslation_key = \"shared-notes\"\n+++\n",
    );
    write(
        &dir,
        "content/רשימות/_index.he.md",
        "+++\ntitle = \"רשימות\"\ntranslation_key = \"shared-notes\"\n+++\n",
    );
    build_ok(&dir);
    assert!(
        read(&dir, "_site/en/notes/index.html")
            .contains("hreflang=\"he\" href=\"https://example.org/he/רשימות/\"")
    );
    write(
        &dir,
        "content/other/_index.en.md",
        "+++\ntitle = \"Other\"\ntranslation_key = \"shared-notes\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("are both the \"en\" translation of section \"shared-notes\""),
        "{error}"
    );
    std::fs::remove_dir_all(dir.join("content/other")).unwrap();

    // 2. Root section generate_feeds overrides language generate_feeds.
    write(
        &dir,
        "content/_index.en.md",
        "+++\ntitle = \"Home\"\ngenerate_feeds = false\n+++\n",
    );
    build_ok(&dir);
    assert!(!planned_paths(&plan_of(&dir)).contains(&"_site/en/atom.xml".to_owned()));

    // 3. Duplicate section index files in the same directory and language are rejected.
    write(&dir, "content/blog/_index.en.html", "<p>dup</p>");
    let error = build_err(&dir);
    assert!(
        error.contains("are both the \"en\" section index of blog"),
        "{error}"
    );
    std::fs::remove_file(dir.join("content/blog/_index.en.html")).unwrap();

    // 4. Drafted HTML/Tera section index and nested subsection under drafted parent drop pages.
    write(
        &dir,
        "content/wip/_index.en.html",
        "+++\ndraft = true\n+++\n<p>wip</p>",
    );
    write(&dir, "content/wip/p.en.md", POST);
    write(
        &dir,
        "content/wip/sub/_index.en.md",
        "+++\ntitle = \"Sub\"\n+++\n",
    );
    write(&dir, "content/wip/sub/q.en.md", POST);
    build_ok(&dir);
    assert!(!dir.join("_site/en/wip").exists());

    // 5. Section body_from rejects non-empty body and implicit target root section.
    write(
        &dir,
        "content/blog/_index.he.md",
        "+++\ntitle = \"בלוג\"\nsort_by = \"date\"\nbody_from = \"en\"\n+++\n\nיש גוף.\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("blog/_index.he.md: has both a body and body_from"),
        "{error}"
    );
    write(
        &dir,
        "content/blog/_index.he.md",
        "+++\ntitle = \"בלוג\"\nsort_by = \"date\"\n+++\n",
    );
    std::fs::remove_file(dir.join("content/_index.en.md")).unwrap();
    write(
        &dir,
        "content/_index.he.md",
        "+++\ntitle = \"בית\"\nbody_from = \"en\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(
        error.contains(
            "_index.he.md: body_from = \"en\" but this section has no \"en\" translation"
        ),
        "{error}"
    );
}
