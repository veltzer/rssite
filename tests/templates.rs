//! The template layer, one probe per function and filter: what each one
//! returns, what context every page sees, and that every misuse is a build
//! error that names the problem rather than empty output.

mod common;

use common::*;

// ── the context every page gets ───────────────────────────────────────────

#[test]
fn the_generator_names_itself_in_the_context() {
    let out = probe(
        "ctx-rssite",
        "{{ rssite.name }}|{{ rssite.version }}|{{ rssite.url }}",
    );
    assert_eq!(
        out,
        format!(
            "rssite|{}|https://github.com/veltzer/rssite",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn config_and_language_are_in_the_context() {
    let out = probe(
        "ctx-config",
        "{{ config.title }}|{{ config.base_url }}|{{ config.extra.months_note }}|{{ config.feeds.limit }}|{{ lang }}|{{ language.name }}|{{ language.direction }}|{{ language.url_prefix }}|{{ language.title }}|{{ languages | length }}",
    );
    assert_eq!(
        out,
        "Example|https://example.org|unused|2|en|English|ltr|/en/|Example site|2"
    );
}

#[test]
fn current_path_and_url_follow_the_output() {
    let out = probe("ctx-current", "{{ current_path }}|{{ current_url }}");
    assert_eq!(out, "/en/blog/first/|https://example.org/en/blog/first/");
}

#[test]
fn the_page_view_has_dates_words_and_its_section() {
    let out = probe(
        "ctx-page",
        "{{ page.title }}|{{ page.date }}|{{ page.datetime }}|{{ page.year }}-{{ page.month }}-{{ page.day }}|{{ page.updated }}|{{ page.updated_datetime }}|{{ page.slug }}|{{ page.path }}|{{ page.word_count }}|{{ page.reading_time }}|{{ page.relative_path }}|{{ page.body_kind }}|{{ page.section.title }}|{{ page.section.permalink }}|{{ page.draft }}|{{ page.components | join(sep=\"/\") }}",
    );
    assert_eq!(
        out,
        "First post|2026-01-10|2026-01-10T00:00:00+00:00|2026-1-10||2026-01-10T00:00:00+00:00|first|/en/blog/first/|15|1|blog/first.en.md|markdown|Blog|https://example.org/en/blog/|false|blog"
    );
}

#[test]
fn updated_falls_back_to_the_date_only_for_the_datetime() {
    let dir = site("ctx-updated");
    write_probe(&dir, "{{ page.updated }}|{{ page.updated_datetime }}");
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/second/index.html")),
        "2026-03-01|2026-03-01T00:00:00+00:00"
    );
}

#[test]
fn summary_toc_and_content_are_rendered_html() {
    let out = probe(
        "ctx-body",
        "{{ page.summary | safe }}##{{ page.toc | length }}##{{ page.content | safe }}",
    );
    let (summary, rest) = out.split_once("##").unwrap();
    assert!(summary.ends_with("</p>"), "{summary}");
    assert!(summary.contains("href=\"https://example.org/en/blog/second/\""));
    let (toc, content) = rest.split_once("##").unwrap();
    assert_eq!(toc, "0");
    assert!(content.contains("<code>code</code>"));
    assert!(
        content.contains("<!-- more -->"),
        "the marker stays in the content"
    );
}

#[test]
fn the_toc_lists_headings_with_ids() {
    let dir = site("ctx-toc");
    write_probe(
        &dir,
        "{% for h in page.toc %}{{ h.level }}:{{ h.id }}:{{ h.title }};{% endfor %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/second/index.html")),
        "2:heading-one:Heading one;2:heading-one-1:Heading one;"
    );
}

#[test]
fn taxonomies_on_a_page_are_resolved_terms() {
    let out = probe(
        "ctx-tax",
        "{% for t in page.taxonomies.tags %}{{ t.name }}={{ t.slug }}={{ t.permalink }};{% endfor %}",
    );
    assert_eq!(
        out,
        "religion=religion=https://example.org/en/tags/religion/;linux=linux=https://example.org/en/tags/linux/;"
    );
}

#[test]
fn every_configured_taxonomy_is_present_even_when_empty() {
    let dir = site("ctx-tax-empty");
    write_probe(&dir, "[{{ page.taxonomies.tags | length }}]");
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/renamed/index.html")),
        "[0]"
    );
}

#[test]
fn translations_and_alternates_pair_the_page_with_its_versions() {
    let out = probe(
        "ctx-trans",
        "{% for t in page.translations %}{{ t.lang }}:{{ t.permalink }}:{{ t.title }};{% endfor %}|{{ alternates | get(key=\"he\", default=\"-\") }}|{{ translations | length }}",
    );
    assert_eq!(
        out,
        "en:https://example.org/en/blog/first/:First post;he:https://example.org/he/blog/first/:פוסט ראשון;|https://example.org/he/blog/first/|2"
    );
}

#[test]
fn an_untranslated_page_has_only_itself() {
    let dir = site("ctx-untranslated");
    write_probe(
        &dir,
        "{{ page.translations | length }}|{{ alternates | get(key=\"he\", default=\"none\") }}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/third/index.html")),
        "1|none"
    );
}

#[test]
fn previous_and_next_follow_the_section_order() {
    let dir = site("ctx-prevnext");
    write_probe(
        &dir,
        "{% if page.previous %}{{ page.previous.title }}{% endif %}|{% if page.next %}{{ page.next.title }}{% endif %}",
    );
    build_ok(&dir);
    // Newest first: fourth (Apr), third (Mar), second (Feb), first (Jan).
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/renamed/index.html")),
        "|Third post"
    );
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/second/index.html")),
        "Third post|First post"
    );
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "Second post|"
    );
}

#[test]
fn page_and_section_extra_are_free_form() {
    let dir = site("ctx-extra");
    write(
        &dir,
        "content/blog/extra.en.md",
        "+++\ntitle = \"Extra\"\ndate = 2026-05-05\n[extra]\nhero = \"big.png\"\n[extra.deep]\nn = [1, 2]\n+++\n",
    );
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\n[extra]\nbanner = true\n+++\n",
    );
    write_probe(
        &dir,
        "{% if page.slug == \"extra\" %}{{ page.extra.hero }}|{{ page.extra.deep.n | length }}|{{ section.extra.banner }}{% endif %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/extra/index.html")),
        "big.png|2|true"
    );
}

#[test]
fn the_section_view_lists_pages_years_and_relations() {
    let dir = site("ctx-section");
    write(
        &dir,
        "templates/section.html",
        "{% extends \"base.html\" %}{% block content %}<probe>{{ section.title }}|{{ section.page_count }}|{% for p in section.pages %}{{ p.slug }},{% endfor %}|{{ section.is_root }}|{{ section.parent.title }}|{{ section.relative_path }}|{{ section.sort_by }}|{{ section.paginate_by }}|{{ section.year_archives }}|{% for y in section.years %}{{ y.year }}:{{ y.count }}:{{ y.path }};{% endfor %}|{{ section.translations | length }}|{{ section.components | join(sep=\",\") }}</probe>{% endblock %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/index.html")),
        "Blog|4|renamed,third,second,first,|false|Home|blog/_index.en.md|date|2|true|2026:4:/en/blog/2026/;|2|blog"
    );
}

#[test]
fn the_root_section_knows_its_subsections() {
    let dir = site("ctx-root");
    write(
        &dir,
        "templates/index.html",
        "{% extends \"base.html\" %}{% block content %}<probe>{{ section.is_root }}|{{ section.relative_path }}|{% for s in section.subsections %}{{ s.title }}@{{ s.path }};{% endfor %}|{{ section.parent }}|{{ section.content | safe }}</probe>{% endblock %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/index.html")),
        "true|_index.en.md|Application@/en/apps/;Blog@/en/blog/;||<p>Welcome.</p>\n"
    );
}

// ── functions ─────────────────────────────────────────────────────────────

#[test]
fn trans_reads_the_current_language_or_a_named_one() {
    let out = probe(
        "fn-trans",
        "{{ trans(key=\"nav_blog\") }}|{{ trans(key=\"nav_blog\", lang=\"he\") }}",
    );
    assert_eq!(out, "Blog|בלוג");
    let error = probe_error(
        "fn-trans-lang",
        "{{ trans(key=\"nav_blog\", lang=\"fr\") }}",
    );
    assert!(
        error.contains("\"fr\" is not a configured language"),
        "{error}"
    );
    let error = probe_error("fn-trans-nokey", "{{ trans() }}");
    assert!(error.contains("argument `key` is required"), "{error}");
    let error = probe_error("fn-trans-missing", "{{ trans(key=\"nope\") }}");
    assert!(
        error.contains("translation key \"nope\" is missing for language \"en\""),
        "{error}"
    );
    assert!(
        error.contains("i18n/en.toml"),
        "the file is named:\n{error}"
    );
}

#[test]
fn get_url_resolves_content_static_and_directories() {
    let out = probe(
        "fn-get-url",
        "{{ get_url(path=\"@/blog/second.md\") }}|{{ get_url(path=\"@/blog/second.md\", lang=\"he\") }}|{{ get_url(path=\"@/blog/second.he.md\") }}|{{ get_url(path=\"@/blog/_index.md\") }}|{{ get_url(path=\"style.css\") }}|{{ get_url(path=\"/style.css\") }}|{{ get_url(path=\"en/blog\") }}|{{ get_url(path=\"en/tags/\") }}|{{ get_url(path=\"sitemap.xml\") }}",
    );
    assert_eq!(
        out,
        "https://example.org/en/blog/second/|https://example.org/he/blog/second/|https://example.org/he/blog/second/|https://example.org/en/blog/|https://example.org/style.css|https://example.org/style.css|https://example.org/en/blog|https://example.org/en/tags/|https://example.org/sitemap.xml"
    );
}

#[test]
fn get_url_checks_unless_told_not_to() {
    let error = probe_error("fn-get-url-missing", "{{ get_url(path=\"missing.css\") }}");
    assert!(
        error.contains("\"missing.css\" is not a file this build produces"),
        "{error}"
    );
    assert!(error.contains("check=false"), "{error}");
    let out = probe(
        "fn-get-url-nocheck",
        "{{ get_url(path=\"missing.css\", check=false) }}|{{ get_url(path=\"other/tool/\", check=false) }}",
    );
    assert_eq!(
        out,
        "https://example.org/missing.css|https://example.org/other/tool/"
    );
    let error = probe_error(
        "fn-get-url-content",
        "{{ get_url(path=\"@/blog/nope.md\") }}",
    );
    assert!(
        error.contains("no content file matches \"@/blog/nope.md\" in language en"),
        "{error}"
    );
    let error = probe_error(
        "fn-get-url-check-type",
        "{{ get_url(path=\"style.css\", check=\"no\") }}",
    );
    assert!(
        error.contains("argument `check` must be a boolean"),
        "{error}"
    );
}

#[test]
fn get_page_and_get_section_return_full_views() {
    let out = probe(
        "fn-get-page",
        "{% set a = get_page(path=\"blog/second.md\") %}{% set b = get_page(path=\"@/blog/second.md\", lang=\"he\") %}{% set c = get_page(path=\"blog/second.he.md\") %}{% set s = get_section(path=\"blog/_index.md\") %}{% set r = get_section(path=\"_index.md\") %}{% set h = get_section(path=\"blog/_index.md\", lang=\"he\") %}{{ a.title }}|{{ b.title }}|{{ c.lang }}|{{ a.taxonomies.tags.0.name }}|{{ s.page_count }}|{{ r.title }}|{{ h.title }}",
    );
    assert_eq!(out, "Second post|פוסט שני|he|religion|4|Home|בלוג");
}

#[test]
fn get_page_and_get_section_do_not_confuse_each_other() {
    let error = probe_error(
        "fn-get-page-section",
        "{{ get_page(path=\"blog/_index.md\") }}",
    );
    assert!(
        error.contains("get_page: no page matches \"blog/_index.md\""),
        "{error}"
    );
    let error = probe_error(
        "fn-get-section-page",
        "{{ get_section(path=\"blog/first.md\") }}",
    );
    assert!(
        error.contains("get_section: no section matches \"blog/first.md\""),
        "{error}"
    );
    let error = probe_error(
        "fn-get-page-missing",
        "{{ get_page(path=\"blog/nope.md\") }}",
    );
    assert!(error.contains("no page matches"), "{error}");
    let error = probe_error(
        "fn-get-page-lang",
        "{{ get_page(path=\"blog/third.md\", lang=\"he\") }}",
    );
    assert!(
        error.contains("in language he"),
        "an English-only page has no Hebrew:\n{error}"
    );
}

#[test]
fn get_taxonomy_lists_terms_with_counts_and_translations() {
    let out = probe(
        "fn-get-taxonomy",
        "{% set t = get_taxonomy(name=\"tags\") %}{{ t.name }}|{{ t.lang }}|{{ t.permalink }}|{% for term in t.terms %}{{ term.name }}:{{ term.page_count }}:{{ term.translations | length }};{% endfor %}|{% set h = get_taxonomy(name=\"tags\", lang=\"he\") %}{{ h.terms | length }}",
    );
    assert_eq!(
        out, "tags|en|https://example.org/en/tags/|linux:1:2;religion:3:2;|2",
        "like page.translations, a term's translations include the term itself"
    );
    let error = probe_error(
        "fn-get-taxonomy-missing",
        "{{ get_taxonomy(name=\"cats\") }}",
    );
    assert!(
        error.contains("\"cats\" is not a configured taxonomy"),
        "{error}"
    );
}

#[test]
fn get_taxonomy_url_finds_a_term_or_fails() {
    let out = probe(
        "fn-get-taxonomy-url",
        "{{ get_taxonomy_url(name=\"tags\", term=\"linux\") }}|{{ get_taxonomy_url(name=\"tags\", term=\"דת\", lang=\"he\") }}",
    );
    assert_eq!(
        out,
        "https://example.org/en/tags/linux/|https://example.org/he/tags/דת/"
    );
    let error = probe_error(
        "fn-get-taxonomy-url-term",
        "{{ get_taxonomy_url(name=\"tags\", term=\"windows\") }}",
    );
    assert!(
        error.contains("taxonomy \"tags\" has no term \"windows\" in language en"),
        "{error}"
    );
    let error = probe_error(
        "fn-get-taxonomy-url-name",
        "{{ get_taxonomy_url(name=\"cats\", term=\"x\") }}",
    );
    assert!(
        error.contains("\"cats\" is not a configured taxonomy"),
        "{error}"
    );
    let error = probe_error(
        "fn-get-taxonomy-url-noterm",
        "{{ get_taxonomy_url(name=\"tags\") }}",
    );
    assert!(error.contains("argument `term` is required"), "{error}");
}

#[test]
fn load_data_reads_toml_json_and_text() {
    let dir = site("fn-load-data");
    write(
        &dir,
        "data/a.toml",
        "name = \"x\"\n[nested]\nn = 2\nwhen = 2026-01-02\n",
    );
    write(&dir, "data/b.json", "{\"list\": [1, 2, 3], \"s\": \"j\"}");
    write(&dir, "data/c.txt", "plain <text>\n");
    write(&dir, "data/d.csv", "a,b\n1,2\n");
    write_probe(
        &dir,
        "{% set a = load_data(path=\"data/a.toml\") %}{{ a.name }}|{{ a.nested.n }}|{{ a.nested.when }}|{% set b = load_data(path=\"data/b.json\") %}{{ b.list | length }}|{{ b.s }}|{{ load_data(path=\"data/c.txt\") }}|{{ load_data(path=\"data/d.csv\") | length }}|{{ load_data(path=\"data/c.txt\", format=\"txt\") | trim }}|{% set none = load_data(path=\"data/none.toml\", required=false) %}[{{ none }}]",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "x|2|2026-01-02|3|j|plain &lt;text&gt;\n|8|plain &lt;text&gt;|[]"
    );
}

#[test]
fn load_data_errors_name_the_file() {
    let error = probe_error(
        "fn-load-data-missing",
        "{{ load_data(path=\"data/none.toml\") }}",
    );
    assert!(
        error.contains("load_data: reading \"data/none.toml\""),
        "{error}"
    );
    let dir = site("fn-load-data-bad");
    write(&dir, "data/bad.toml", "not = = toml\n");
    write_probe(&dir, "{{ load_data(path=\"data/bad.toml\") }}");
    let error = build_err(&dir);
    assert!(error.contains("data/bad.toml is not valid TOML"), "{error}");
    write(&dir, "data/bad.json", "{");
    write_probe(&dir, "{{ load_data(path=\"data/bad.json\") }}");
    let error = build_err(&dir);
    assert!(error.contains("data/bad.json is not valid JSON"), "{error}");
}

#[test]
fn load_data_stays_inside_the_site() {
    let error = probe_error(
        "fn-load-data-up",
        "{{ load_data(path=\"../rssite.toml\") }}",
    );
    assert!(error.contains("must be a path inside the site"), "{error}");
    let error = probe_error(
        "fn-load-data-abs",
        "{{ load_data(path=\"/etc/hostname\") }}",
    );
    assert!(error.contains("must be a path inside the site"), "{error}");
    let error = probe_error(
        "fn-load-data-up-required-false",
        "{{ load_data(path=\"a/../../x\", required=false) }}",
    );
    assert!(
        error.contains("must be a path inside the site"),
        "required=false does not bypass the check:\n{error}"
    );
}

#[test]
fn load_data_with_an_explicit_format_overrides_the_extension() {
    let dir = site("fn-load-data-format");
    write(&dir, "data/config.txt", "k = \"v\"\n");
    write_probe(
        &dir,
        "{% set d = load_data(path=\"data/config.txt\", format=\"toml\") %}{{ d.k }}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "v"
    );
}

#[test]
fn load_data_makes_the_file_a_source_of_the_page() {
    let dir = site("fn-load-data-source");
    write(&dir, "data/a.toml", "x = 1\n");
    write_probe(&dir, "{{ load_data(path=\"data/a.toml\").x }}");
    let manifest = plan_of(&dir);
    let sources = sources_of(&manifest, "_site/en/blog/first/index.html");
    assert!(sources.contains(&"data/a.toml".to_owned()), "{sources:?}");
}

#[test]
fn non_deterministic_functions_are_refused_by_name() {
    for (name, call) in [
        ("now", "{{ now() }}"),
        ("get_random", "{{ get_random(end=10) }}"),
        ("get_env", "{{ get_env(name=\"HOME\") }}"),
    ] {
        let error = probe_error(&format!("fn-forbidden-{name}"), call);
        assert!(
            error.contains(&format!("{name}() is not available")),
            "{name}: {error}"
        );
    }
}

#[test]
fn an_unknown_function_or_variable_is_an_error_naming_the_template() {
    let error = probe_error("fn-unknown", "{{ no_such_function() }}");
    assert!(error.contains("no_such_function"), "{error}");
    assert!(error.contains("page.html"), "{error}");
    let error = probe_error("fn-unknown-var", "{{ page.nope.deeper }}");
    assert!(error.contains("page.html"), "{error}");
    assert!(error.contains("nope"), "{error}");
}

// ── filters ───────────────────────────────────────────────────────────────

#[test]
fn the_date_filter_uses_the_language_names() {
    let out = probe(
        "filter-date",
        "{{ page.date | date }}|{{ page.date | date(format=\"%-d %B %Y\") }}|{{ page.date | date(format=\"%-d %B %Y\", lang=\"he\") }}|{{ page.datetime | date(format=\"%A %H:%M\") }}|{{ \"2026-12-25\" | date(format=\"%b %a\") }}",
    );
    assert_eq!(
        out,
        "2026-01-10|10 January 2026|10 ינואר 2026|Saturday 00:00|Dec Fri"
    );
}

#[test]
fn the_date_filter_rejects_bad_input() {
    let error = probe_error("filter-date-null", "{{ page.updated | date }}");
    assert!(error.contains("date filter: the value is null"), "{error}");
    let error = probe_error(
        "filter-date-format",
        "{{ page.date | date(format=\"%Q\") }}",
    );
    assert!(error.contains("is not a valid strftime format"), "{error}");
    let error = probe_error("filter-date-value", "{{ \"soon\" | date }}");
    assert!(error.contains("is not a date"), "{error}");
    let error = probe_error("filter-date-lang", "{{ page.date | date(lang=\"xx\") }}");
    assert!(
        error.contains("\"xx\" is not a configured language"),
        "{error}"
    );
}

#[test]
fn the_slugify_filter_has_two_modes() {
    let out = probe(
        "filter-slugify",
        "{{ \"Hello, World!\" | slugify }}|{{ \"שלום עולם\" | slugify }}|{{ \"Café\" | slugify(mode=\"ascii\") }}|{{ \"Café\" | slugify(mode=\"safe\") }}",
    );
    assert_eq!(out, "hello-world|שלום-עולם|cafe|café");
    let error = probe_error(
        "filter-slugify-mode",
        "{{ \"x\" | slugify(mode=\"loose\") }}",
    );
    assert!(error.contains("mode must be safe or ascii"), "{error}");
    let error = probe_error("filter-slugify-type", "{{ 3 | slugify }}");
    assert!(
        error.contains("slugify: the value must be a string"),
        "{error}"
    );
}

#[test]
fn the_markdown_filter_renders_blocks_or_inline() {
    let out = probe(
        "filter-markdown",
        "{{ \"a *b*\" | markdown | safe }}|{{ \"a *b*\" | markdown(inline=true) | safe }}|{{ \"# H\" | markdown | safe }}|{{ \"one\n\ntwo\" | markdown(inline=true) | safe }}",
    );
    assert_eq!(
        out,
        "<p>a <em>b</em></p>\n|a <em>b</em>|<h1 id=\"h\">H</h1>\n|<p>one</p>\n<p>two</p>\n"
    );
    let error = probe_error("filter-markdown-type", "{{ 3 | markdown }}");
    assert!(
        error.contains("markdown: the value must be a string"),
        "{error}"
    );
}

#[test]
fn the_markdown_filter_does_not_resolve_content_links() {
    let error = probe_error(
        "filter-markdown-link",
        "{{ \"[x](@/blog/second.md)\" | markdown }}",
    );
    assert!(error.contains("no content file matches"), "{error}");
}

#[test]
fn autoescaping_covers_html_but_not_slashes() {
    let out = probe(
        "escape",
        "{{ '<b>&\"</b>' }}|{{ page.permalink }}|{{ \"<i>\" | safe }}",
    );
    assert_eq!(
        out,
        "&lt;b&gt;&amp;&quot;&lt;/b&gt;|https://example.org/en/blog/first/|<i>"
    );
}

// ── overriding the built-in templates ─────────────────────────────────────

#[test]
fn a_site_can_replace_every_built_in_template() {
    let dir = site("builtin-override");
    write(
        &dir,
        "templates/atom.xml",
        "custom feed {{ feed_title }} {{ pages | length }}",
    );
    write(
        &dir,
        "templates/sitemap.xml",
        "custom sitemap {{ entries | length }}",
    );
    write(
        &dir,
        "templates/404.html",
        "custom 404 {{ lang }} {{ language.url_prefix }}",
    );
    build_ok(&dir);
    assert_eq!(
        read(&dir, "_site/en/atom.xml"),
        "custom feed Example site 2"
    );
    assert_eq!(
        read(&dir, "_site/en/blog/atom.xml"),
        "custom feed Example - Blog 2"
    );
    assert_eq!(
        read(&dir, "_site/en/tags/linux/atom.xml"),
        "custom feed Example - linux 1"
    );
    assert!(read(&dir, "_site/sitemap.xml").starts_with("custom sitemap "));
    assert_eq!(read(&dir, "_site/en/404.html"), "custom 404 en /en/");
    assert_eq!(read(&dir, "_site/he/404.html"), "custom 404 he /he/");
    assert_eq!(
        read(&dir, "_site/404.html"),
        "custom 404 en /en/",
        "the global 404 is the fallback language's"
    );
    let manifest = plan_of(&dir);
    assert!(sources_of(&manifest, "_site/en/atom.xml").contains(&"templates/atom.xml".to_owned()));
    assert!(sources_of(&manifest, "_site/404.html").contains(&"templates/404.html".to_owned()));
    assert!(
        sources_of(&manifest, "_site/sitemap.xml").contains(&"templates/sitemap.xml".to_owned())
    );
}

#[test]
fn the_built_in_404_names_the_site_and_language() {
    let dir = site("builtin-404");
    build_ok(&dir);
    let he = read(&dir, "_site/he/404.html");
    assert!(he.contains("<html lang=\"he\" dir=\"rtl\">"), "{he}");
    assert!(he.contains("<a href=\"/he/\">Example</a>"), "{he}");
    assert!(he.contains("<title>404 &middot; Example</title>"), "{he}");
}

#[test]
fn the_built_in_sitemap_and_feed_have_the_expected_shape() {
    let dir = site("builtin-shape");
    build_ok(&dir);
    let sitemap = read(&dir, "_site/sitemap.xml");
    assert!(sitemap.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset"));
    assert!(
        sitemap.contains("<loc>https://example.org/</loc>"),
        "the root is listed"
    );
    assert!(
        sitemap.contains(
            "<loc>https://example.org/en/blog/second/</loc>\n    <lastmod>2026-03-01</lastmod>"
        ),
        "updated wins over date:\n{sitemap}"
    );
    assert!(sitemap.contains("<loc>https://example.org/en/tags/religion/page/2/</loc>"));
    assert!(sitemap.contains("<loc>https://example.org/en/blog/page/2/</loc>"));
    assert!(!sitemap.contains("404.html"));
    assert!(!sitemap.contains("atom.xml"));
    let feed = read(&dir, "_site/en/atom.xml");
    assert!(
        feed.contains("<generator uri=\"https://github.com/veltzer/rssite\">rssite</generator>")
    );
    assert!(feed.contains("<link rel=\"self\" type=\"application/atom+xml\" href=\"https://example.org/en/atom.xml\"/>"));
    assert!(
        feed.contains(
            "<link rel=\"alternate\" type=\"text/html\" href=\"https://example.org/en/\"/>"
        )
    );
    assert!(feed.contains("<author><name>Example Author</name></author>"));
    assert!(feed.contains("<title>Example site</title>"));
    // Newest first, limit 2: fourth (Apr) then third (Mar).
    let fourth = feed.find("Fourth post").unwrap();
    let third = feed.find("Third post").unwrap();
    assert!(fourth < third);
    assert!(!feed.contains("Second post"));
}

#[test]
fn feed_entries_carry_a_summary_when_the_page_has_one() {
    let dir = site("builtin-feed-summary");
    edit_config(&dir, "limit = 2", "limit = 10");
    build_ok(&dir);
    let feed = read(&dir, "_site/en/atom.xml");
    assert!(
        feed.contains("<summary type=\"html\">"),
        "first.en.md has a <!-- more -->:\n{feed}"
    );
    assert!(
        feed.contains("<content type=\"html\" xml:base=\"https://example.org/en/blog/third/\">"),
        "{feed}"
    );
}

#[test]
fn a_template_error_names_the_template_the_language_and_the_output() {
    let dir = site("error-naming");
    let base = read(&dir, "templates/base.html");
    write(
        &dir,
        "templates/base.html",
        &base.replace("<main>", "<main>{{ nope.x }}"),
    );
    let error = build_err(&dir);
    assert!(
        error.contains("producing "),
        "the output is named:\n{error}"
    );
    assert!(
        error.contains("rendering template page.html (en)"),
        "the template and language are named:\n{error}"
    );
    assert!(
        error.contains("`nope.x` not found"),
        "the variable is named:\n{error}"
    );
}

#[test]
fn a_tera_body_is_rendered_with_the_page_context() {
    let dir = site("tera-body");
    write(
        &dir,
        "content/blog/dyn.en.tera",
        "+++\ntitle = \"Dynamic\"\ndate = 2026-06-01\n+++\n{% set blog = get_section(path=\"blog/_index.md\") %}<p>{{ page.title }} in {{ lang }} at {{ page.permalink }}; blog has {{ blog.page_count }} pages; {{ trans(key=\"nav_tags\") }}</p>",
    );
    build_ok(&dir);
    let page = read(&dir, "_site/en/blog/dyn/index.html");
    assert!(
        page.contains(
            "<p>Dynamic in en at https://example.org/en/blog/dyn/; blog has 5 pages; Tags</p>"
        ),
        "{page}"
    );
    write(
        &dir,
        "content/blog/dyn.en.tera",
        "+++\ntitle = \"Dynamic\"\ndate = 2026-06-01\n+++\n{{ undefined_thing }}",
    );
    let error = build_err(&dir);
    assert!(
        error.contains("rendering the template body of blog/dyn.en.tera"),
        "{error}"
    );
}

#[test]
fn a_tera_section_body_is_rendered_too() {
    let dir = site("tera-section");
    write(
        &dir,
        "content/blog/_index.en.tera",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\n+++\n<p>{{ section.page_count }} posts, newest {{ section.pages.0.title }}</p>",
    );
    std::fs::remove_file(dir.join("content/blog/_index.en.md")).unwrap();
    // A content path names its file, extension included: the English index
    // is now blog/_index.tera, so base.html's link must not say .md.
    let base = read(&dir, "templates/base.html");
    write(
        &dir,
        "templates/base.html",
        &base.replace(
            "get_url(path=\"@/blog/_index.md\")",
            "get_url(path=language.url_prefix ~ \"blog/\")",
        ),
    );
    write(
        &dir,
        "templates/index.html",
        "{% extends \"base.html\" %}{% block content %}{{ section.title }}{% endblock %}",
    );
    write(
        &dir,
        "templates/section.html",
        "{% extends \"base.html\" %}{% block content %}<probe>{{ section.content | safe }}</probe>{% endblock %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/index.html")),
        "<p>4 posts, newest Fourth post</p>"
    );
}

#[test]
fn page_and_section_templates_can_be_chosen_in_front_matter() {
    let dir = site("template-choice");
    write(&dir, "templates/special.html", "SPECIAL {{ page.title }}");
    write(
        &dir,
        "templates/listing.html",
        "LISTING {{ section.title }}",
    );
    write(&dir, "templates/post.html", "POST {{ page.title }}");
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\ntemplate = \"listing.html\"\npage_template = \"post.html\"\n+++\n",
    );
    write(
        &dir,
        "content/blog/special.en.md",
        "+++\ntitle = \"S\"\ndate = 2026-07-01\ntemplate = \"special.html\"\n+++\n",
    );
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/en/blog/index.html"), "LISTING Blog");
    assert_eq!(read(&dir, "_site/en/blog/special/index.html"), "SPECIAL S");
    assert_eq!(
        read(&dir, "_site/en/blog/first/index.html"),
        "POST First post",
        "the section's page_template applies to its other pages"
    );
    assert!(
        read(&dir, "_site/en/about/index.html").contains("<article>"),
        "other sections keep page.html"
    );
    let manifest = plan_of(&dir);
    assert_eq!(
        sources_of(&manifest, "_site/en/blog/special/index.html")
            .iter()
            .filter(|s| s.starts_with("templates/"))
            .collect::<Vec<_>>(),
        vec!["templates/special.html"]
    );
    write(
        &dir,
        "content/blog/special.en.md",
        "+++\ntitle = \"S\"\ndate = 2026-07-01\ntemplate = \"nope.html\"\n+++\n",
    );
    let error = build_err(&dir);
    assert!(error.contains("nope.html"), "{error}");
}

#[test]
fn template_includes_and_macros_work_and_are_tracked() {
    let dir = site("includes");
    write(
        &dir,
        "templates/_badge.html",
        "<span class=\"badge\">{{ page.title }}</span>",
    );
    write(
        &dir,
        "templates/macros.html",
        "{% macro shout(text) %}{{ text | upper }}{% endmacro %}",
    );
    write(
        &dir,
        "templates/page.html",
        "{% import \"macros.html\" as m %}{% extends \"base.html\" %}{% block content %}{% if lang == \"en\" %}<probe>{% include \"_badge.html\" %}|{{ m::shout(text=\"hi\") }}</probe>{% endif %}{% endblock %}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "<span class=\"badge\">First post</span>|HI"
    );
    let manifest = plan_of(&dir);
    let sources = sources_of(&manifest, "_site/en/blog/first/index.html");
    for needed in [
        "templates/_badge.html",
        "templates/macros.html",
        "templates/page.html",
        "templates/base.html",
    ] {
        assert!(
            sources.contains(&needed.to_owned()),
            "{needed} missing from {sources:?}"
        );
    }
}

#[test]
fn content_assets_resolve_in_markdown_and_get_url() {
    let dir = site("asset-links");
    write(&dir, "content/blog/pic.png", "png");
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Third post\"\ndate = 2026-03-15\n+++\n\n![alt](@/blog/pic.png) and [csv](@/blog/data.csv)\n",
    );
    write_probe(
        &dir,
        "{{ get_url(path=\"@/blog/pic.png\") }}|{{ get_url(path=\"@/blog/data.csv\", lang=\"he\") }}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "https://example.org/en/blog/pic.png|https://example.org/he/blog/data.csv"
    );
    write(
        &dir,
        "templates/page.html",
        "{{ page.content | safe }}",
    );
    build_ok(&dir);
    let third = read(&dir, "_site/en/blog/third/index.html");
    assert!(
        third.contains("src=\"https://example.org/en/blog/pic.png\"")
            && third.contains("href=\"https://example.org/en/blog/data.csv\""),
        "{third}"
    );
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/en/blog/third/index.html");
    assert!(
        sources.contains(&"content/blog/pic.png".to_owned())
            && sources.contains(&"content/blog/data.csv".to_owned()),
        "{sources:?}"
    );
}

#[test]
fn tera_bodies_populate_content_in_feeds_listings_and_get_page() {
    let dir = site("tera-everywhere");
    write(
        &dir,
        "content/blog/dyn.en.tera",
        "+++\ntitle = \"Dynamic\"\ndate = 2026-06-01\n+++\n<p>Lead paragraph</p><!-- more --><p>Rest of {{ page.title }}</p>",
    );
    write_probe(
        &dir,
        "{% set p = get_page(path=\"blog/dyn.tera\") %}{{ p.summary | safe }}|{{ p.content | safe }}|{{ p.word_count }}",
    );
    build_ok(&dir);
    assert_eq!(
        extract_probe(&read(&dir, "_site/en/blog/first/index.html")),
        "<p>Lead paragraph</p>|<p>Lead paragraph</p><!-- more --><p>Rest of Dynamic</p>|5"
    );
    let feed = read(&dir, "_site/en/atom.xml");
    assert!(
        feed.contains("Rest of Dynamic"),
        "dated .tera page content appears in the feed:\n{feed}"
    );
}

