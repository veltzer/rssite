//! The commands around the build: what the plan says each file depends on,
//! what `build` writes and leaves alone, what `clean` removes, what `query`
//! and `version` print, how `init` refuses to clobber, and the exit codes.

mod common;

use common::*;
use std::process::Command;

// ── the plan's sources ────────────────────────────────────────────────────

#[test]
fn every_output_names_its_sources() {
    let dir = site("sources");
    let plan = plan_of(&dir);
    let page = sources_of(&plan, "_site/en/blog/first/index.html");
    for needed in [
        "content/blog/first.en.md",
        "content/blog/first.he.md",
        "content/blog/second.en.md",
        "content/blog/second.he.md",
        "content/blog/_index.en.md",
        "templates/page.html",
        "templates/base.html",
        "i18n/en.toml",
        "rssite.toml",
    ] {
        assert!(
            page.contains(&needed.to_owned()),
            "{needed} missing from {page:?}"
        );
    }
    assert!(
        page.contains(&"content/blog/third.en.md".to_owned()),
        "page.html reads page.previous, so every content file is a source: {page:?}"
    );
    assert!(!page.contains(&"i18n/he.toml".to_owned()), "{page:?}");
    assert!(!page.contains(&"i18n/tags.toml".to_owned()), "{page:?}");

    let alias = sources_of(&plan, "_site/old/third/index.html");
    assert_eq!(alias, ["content/blog/third.en.md", "rssite.toml"]);

    let listing = sources_of(&plan, "_site/en/blog/index.html");
    assert!(
        listing.contains(&"content/about.en.tera".to_owned()),
        "every English content file: {listing:?}"
    );
    assert!(listing.contains(&"templates/section.html".to_owned()));
    assert!(
        listing.contains(&"content/blog/first.he.md".to_owned()),
        "section.html loops over section.pages, which makes it depend on everything: {listing:?}"
    );
    assert_eq!(
        sources_of(&plan, "_site/en/blog/page/2/index.html"),
        listing,
        "pagers share the sources"
    );

    let home = sources_of(&plan, "_site/en/index.html");
    assert!(home.contains(&"templates/index.html".to_owned()));
    assert!(
        home.contains(&"content/blog/third.en.md".to_owned()),
        "index.html reads the blog section: {home:?}"
    );

    let feed = sources_of(&plan, "_site/en/atom.xml");
    assert!(
        feed.contains(&"content/blog/third.en.md".to_owned())
            && !feed.contains(&"content/blog/first.he.md".to_owned())
    );
    assert!(
        !feed.iter().any(|s| s.starts_with("templates/")),
        "the built-in feed template is not a file: {feed:?}"
    );

    let sitemap = sources_of(&plan, "_site/sitemap.xml");
    assert!(
        sitemap.contains(&"content/blog/first.he.md".to_owned())
            && sitemap.contains(&"i18n/tags.toml".to_owned())
            && sitemap.contains(&"i18n/en.toml".to_owned())
            && sitemap.contains(&"rssite.toml".to_owned())
    );

    let archive = sources_of(&plan, "_site/en/blog/2026/index.html");
    assert!(
        archive.contains(&"content/blog/first.en.md".to_owned())
            && archive.contains(&"content/blog/first.he.md".to_owned())
            && archive.contains(&"content/blog/_index.he.md".to_owned()),
        "archive.html extends base.html, which reads translations/alternates: {archive:?}"
    );

    assert_eq!(
        sources_of(&plan, "_site/favicon.svg"),
        ["static/favicon.svg"]
    );
    assert_eq!(
        sources_of(&plan, "_site/style.css"),
        ["sass/_vars.scss", "sass/style.scss"]
    );
    assert_eq!(sources_of(&plan, "_site/highlight.css"), ["rssite.toml"]);
    assert_eq!(
        sources_of(&plan, "_site/en/blog/data.csv"),
        ["content/blog/data.csv"]
    );

    let tags = sources_of(&plan, "_site/en/tags/index.html");
    assert!(
        tags.contains(&"i18n/tags.toml".to_owned())
            && tags.contains(&"templates/taxonomy_list.html".to_owned())
    );
    let term = sources_of(&plan, "_site/he/tags/דת/index.html");
    assert!(
        term.contains(&"templates/taxonomy_single.html".to_owned())
            && term.contains(&"i18n/he.toml".to_owned())
            && term.contains(&"content/blog/first.he.md".to_owned())
    );
    assert!(
        sources_of(&plan, "_site/en/tags/religion/atom.xml").contains(&"i18n/tags.toml".to_owned())
    );

    let not_found = sources_of(&plan, "_site/he/404.html");
    assert_eq!(
        not_found,
        ["i18n/he.toml", "rssite.toml"],
        "the built-in 404 depends on the language's strings and the config"
    );
    assert_eq!(
        sources_of(&plan, "_site/404.html"),
        ["i18n/en.toml", "rssite.toml"]
    );
    assert_eq!(sources_of(&plan, "_site/robots.txt"), ["rssite.toml"]);
}

#[test]
fn body_links_and_body_from_add_sources() {
    let dir = site("sources-links");
    write(&dir, "templates/_part.html", "<p>part</p>");
    write(&dir, "data/info.toml", "k = 1\n");
    write(
        &dir,
        "content/about.en.tera",
        "+++\ntitle = \"About\"\ntemplate = \"page.html\"\ndate = 2026-01-01\n+++\n{% include \"_part.html\" %}{{ load_data(path=\"data/info.toml\") }}{{ get_section(path=\"blog/_index.md\") }}",
    );
    write(
        &dir,
        "content/about.he.tera",
        "+++\ntitle = \"אודות\"\ntemplate = \"page.html\"\ndate = 2026-01-01\nbody_from = \"en\"\n+++\n",
    );
    let plan = plan_of(&dir);
    let first = sources_of(&plan, "_site/en/blog/first/index.html");
    assert!(
        first.contains(&"content/blog/second.he.md".to_owned()),
        "a link to the Hebrew version:\n{first:?}"
    );
    let he_app = sources_of(&plan, "_site/he/apps/index.html");
    assert!(
        he_app.contains(&"content/apps/_index.en.html".to_owned()),
        "body_from:\n{he_app:?}"
    );
    assert!(he_app.contains(&"templates/app.html".to_owned()));
    let about_he = sources_of(&plan, "_site/he/about/index.html");
    for needed in [
        "content/about.en.tera",
        "content/about.he.tera",
        "templates/_part.html",
        "data/info.toml",
        "content/blog/first.en.md",
    ] {
        assert!(
            about_he.contains(&needed.to_owned()),
            "{needed} missing from {about_he:?}"
        );
    }
}

#[test]
fn a_template_that_reads_other_pages_depends_on_every_content_file() {
    let dir = site("sources-cross");
    write_probe(&dir, "{{ page.title }}");
    let plan = plan_of(&dir);
    let before = sources_of(&plan, "_site/en/blog/third/index.html");
    assert!(
        !before.contains(&"content/blog/first.en.md".to_owned()),
        "an unrelated post is not a source: {before:?}"
    );
    assert!(
        !before.contains(&"content/blog/first.he.md".to_owned()),
        "{before:?}"
    );
    for construct in [
        "{{ page.previous }}",
        "{{ page.next.title }}",
        "{{ section.pages | length }}",
        "{{ get_page(path=\"blog/first.md\") }}",
        "{{ get_taxonomy(name=\"tags\") }}",
        "{{ get_taxonomy_url(name=\"tags\", term=\"linux\") }}",
    ] {
        write_probe(&dir, construct);
        let plan = plan_of(&dir);
        let after = sources_of(&plan, "_site/en/blog/third/index.html");
        assert!(
            after.contains(&"content/blog/first.he.md".to_owned()),
            "{construct}: {after:?}"
        );
        assert!(
            after.contains(&"content/about.en.tera".to_owned()),
            "{construct}: {after:?}"
        );
    }
}

#[test]
fn the_plan_is_sorted_unique_and_prefixed_with_the_output_directory() {
    let dir = site("plan-shape");
    prepend_config(&dir, "output_dir = \"public\"");
    let plan = plan_of(&dir);
    let paths = planned_paths(&plan);
    assert!(paths.iter().all(|p| p.starts_with("public/")), "{paths:?}");
    let mut sorted = paths.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(paths, sorted);
    for output in plan["outputs"].as_array().unwrap() {
        let sources = output["sources"].as_array().unwrap();
        assert!(!sources.is_empty(), "{} has no sources", output["path"]);
        let texts: Vec<&str> = sources.iter().map(|s| s.as_str().unwrap()).collect();
        let mut sorted = texts.clone();
        sorted.sort_unstable();
        assert_eq!(texts, sorted, "sources are sorted");
        assert!(
            texts
                .iter()
                .all(|s| !s.starts_with('/') && !s.starts_with("public/")),
            "sources are site-relative: {texts:?}"
        );
    }
    assert_eq!(
        plan.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["version", "outputs"]
    );
}

#[test]
fn the_plan_does_not_touch_the_output_directory() {
    let dir = site("plan-readonly");
    let plan = run(&dir, &["plan"]);
    assert!(plan.ok);
    assert!(!dir.join("_site").exists());
    let check = run(&dir, &["check"]);
    assert!(check.ok, "{}", check.stderr);
    assert!(!dir.join("_site").exists(), "check writes nothing");
    assert!(
        check.stderr.contains("checked") && check.stderr.contains("files"),
        "{}",
        check.stderr
    );
    let query = run(&dir, &["query"]);
    assert!(query.ok);
    assert!(!dir.join("_site").exists());
}

// ── building ──────────────────────────────────────────────────────────────

#[test]
fn build_reports_counts_and_writes_only_what_changed() {
    let dir = site("incremental");
    let first = build_ok(&dir);
    let total = planned_paths(&plan_of(&dir)).len();
    assert!(
        first.stderr.contains(&format!(
            "built {total} files: {total} written, 0 unchanged"
        )),
        "{}",
        first.stderr
    );
    let again = build_ok(&dir);
    assert!(
        again.stderr.contains(&format!(
            "built {total} files: 0 written, {total} unchanged"
        )),
        "{}",
        again.stderr
    );
    write(&dir, "static/favicon.svg", "<svg>new</svg>");
    let one = build_ok(&dir);
    assert!(one.stderr.contains("1 written"), "{}", one.stderr);
    assert_eq!(read(&dir, "_site/favicon.svg"), "<svg>new</svg>");
    write(&dir, "sass/_vars.scss", "$width: 50rem;");
    let css = build_ok(&dir);
    assert!(
        css.stderr.contains("1 written"),
        "a partial change rewrites the one stylesheet:\n{}",
        css.stderr
    );
    assert!(read(&dir, "_site/style.css").contains("50rem"));
}

#[test]
fn an_unchanged_file_keeps_its_mtime() {
    let dir = site("mtime");
    build_ok(&dir);
    let path = dir.join("_site/en/blog/first/index.html");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Third post\"\ndate = 2026-03-15\n+++\n\nChanged body.\n",
    );
    let build = build_ok(&dir);
    assert!(!build.stderr.contains("0 written"), "{}", build.stderr);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "first.en.md's page was not rewritten"
    );
    let third = dir.join("_site/en/blog/third/index.html");
    assert!(std::fs::metadata(&third).unwrap().modified().unwrap() > before);
}

#[test]
fn a_changed_source_reaches_every_output_that_names_it() {
    let dir = site("propagation");
    build_ok(&dir);
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Renamed third\"\ndate = 2026-03-15\naliases = [\"/old/third/\", \"/legacy/third.html\"]\n[taxonomies]\ntags = [\"religion\"]\n+++\n\nEnglish only, with aliases.\n",
    );
    build_ok(&dir);
    for rel in [
        "_site/en/blog/third/index.html",
        "_site/en/blog/index.html",
        "_site/en/blog/2026/index.html",
        "_site/en/tags/religion/index.html",
        "_site/en/atom.xml",
        "_site/en/blog/atom.xml",
        "_site/en/index.html",
        "_site/en/blog/second/index.html",
        "_site/en/blog/renamed/index.html",
    ] {
        assert!(read(&dir, rel).contains("Renamed third"), "{rel} is stale");
    }
}

#[test]
fn build_with_plan_prints_the_manifest_and_builds() {
    let dir = site("build-plan");
    let build = run(&dir, &["build", "--plan"]);
    assert!(build.ok, "{}", build.stderr);
    let manifest: serde_json::Value = serde_json::from_str(&build.stdout).unwrap();
    assert_eq!(manifest, plan_of(&dir));
    assert!(dir.join("_site/en/index.html").is_file());
    let plain = run(&dir, &["build"]);
    assert_eq!(plain.stdout, "", "without --plan nothing goes to stdout");
}

#[test]
fn the_build_and_the_plan_agree_in_every_configuration() {
    let dir = site("agree");
    prepend_config(&dir, "theme = \"t\"");
    write(&dir, "themes/t/static/t.txt", "t");
    write(&dir, "themes/t/sass/t.scss", ".t{}");
    append_config(&dir, "[[publish]]\nfrom = \"shared\"\nto = \"s\"");
    write(&dir, "shared/a/b.css", "x");
    write(&dir, "static/robots.txt", "custom");
    write(
        &dir,
        "content/blog/old.en.md",
        "+++\ntitle = \"Old\"\ndate = 2020-01-01\naliases = [\"/o/\"]\n+++\n",
    );
    build_ok(&dir);
    let planned = planned_paths(&plan_of(&dir));
    let built: Vec<String> = files_under(&dir.join("_site"))
        .iter()
        .map(|f| format!("_site/{f}"))
        .collect();
    assert_eq!(planned, built);
}

#[test]
fn stale_files_in_the_output_directory_are_left_alone_by_build() {
    let dir = site("stale");
    write(&dir, "_site/leftover.txt", "from another tool");
    write(&dir, "_site/en/blog/gone/index.html", "old page");
    build_ok(&dir);
    assert_eq!(read(&dir, "_site/leftover.txt"), "from another tool");
    assert_eq!(read(&dir, "_site/en/blog/gone/index.html"), "old page");
}

#[test]
fn the_output_directory_is_configurable_for_every_command() {
    let dir = site("output-dir");
    prepend_config(&dir, "output_dir = \"out/www\"");
    build_ok(&dir);
    assert!(dir.join("out/www/en/index.html").is_file());
    assert!(!dir.join("_site").exists());
    assert!(planned_paths(&plan_of(&dir))[0].starts_with("out/www/"));
    let clean = run(&dir, &["clean"]);
    assert!(clean.ok);
    assert!(files_under(&dir.join("out/www")).is_empty());
    assert!(
        dir.join("out/www").is_dir(),
        "the output directory itself stays"
    );
}

#[test]
fn the_content_and_other_directories_are_configurable() {
    let dir = site("dirs");
    for (from, to) in [
        ("content", "src/pages"),
        ("templates", "src/layouts"),
        ("static", "src/files"),
        ("sass", "src/styles"),
        ("i18n", "src/strings"),
    ] {
        std::fs::create_dir_all(dir.join(to).parent().unwrap()).unwrap();
        std::fs::rename(dir.join(from), dir.join(to)).unwrap();
    }
    prepend_config(
        &dir,
        "content_dir = \"src/pages\"\ntemplates_dir = \"src/layouts\"\nstatic_dir = \"src/files\"\nsass_dir = \"src/styles\"\ni18n_dir = \"src/strings\"",
    );
    edit_config(
        &dir,
        "translations = \"i18n/tags.toml\"",
        "translations = \"src/strings/tags.toml\"",
    );
    build_ok(&dir);
    assert!(dir.join("_site/en/blog/first/index.html").is_file());
    assert!(dir.join("_site/style.css").is_file());
    assert!(dir.join("_site/favicon.svg").is_file());
    let plan = plan_of(&dir);
    let first = sources_of(&plan, "_site/en/blog/first/index.html");
    assert!(
        first.contains(&"src/pages/blog/first.en.md".to_owned()),
        "{first:?}"
    );
    assert!(
        first.contains(&"src/layouts/page.html".to_owned()),
        "{first:?}"
    );
    assert!(
        first.contains(&"src/strings/en.toml".to_owned()),
        "{first:?}"
    );
}

#[test]
fn an_empty_site_still_builds_its_skeleton() {
    let dir = temp_dir("empty");
    write(
        &dir,
        "rssite.toml",
        "base_url = \"https://e.org\"\n[[languages]]\ncode = \"en\"\nname = \"E\"\n",
    );
    write(&dir, "templates/index.html", "empty");
    build_ok(&dir);
    let built = files_under(&dir.join("_site"));
    assert_eq!(
        built,
        [
            "404.html",
            "en/404.html",
            "en/atom.xml",
            "en/index.html",
            "highlight.css",
            "index.html",
            "robots.txt",
            "sitemap.xml"
        ]
    );
    let error = {
        std::fs::remove_file(dir.join("templates/index.html")).unwrap();
        build_err(&dir)
    };
    assert!(
        error.contains("index.html"),
        "the root listing needs its template:\n{error}"
    );
}

// ── clean ─────────────────────────────────────────────────────────────────

#[test]
fn clean_removes_planned_files_and_emptied_directories_only() {
    let dir = site("clean");
    build_ok(&dir);
    write(&dir, "_site/keep.txt", "foreign");
    write(&dir, "_site/en/blog/first/extra.txt", "foreign too");
    let clean = run(&dir, &["clean"]);
    assert!(clean.ok, "{}", clean.stderr);
    let total = planned_paths(&plan_of(&dir)).len();
    assert!(
        clean.stderr.contains(&format!("removed {total} files")),
        "{}",
        clean.stderr
    );
    assert_eq!(
        files_under(&dir.join("_site")),
        ["en/blog/first/extra.txt", "keep.txt"]
    );
    assert!(!dir.join("_site/he").exists(), "emptied directories go");
    assert!(
        dir.join("_site/en/blog/first").is_dir(),
        "a directory with a foreign file stays"
    );
    let again = run(&dir, &["clean"]);
    assert!(again.ok);
    assert!(again.stderr.contains("removed 0 files"));
    std::fs::remove_dir_all(dir.join("_site")).unwrap();
    let none = run(&dir, &["clean"]);
    assert!(
        none.ok,
        "no output directory is not an error:\n{}",
        none.stderr
    );
}

#[test]
fn clean_follows_the_current_plan() {
    let dir = site("clean-plan");
    build_ok(&dir);
    std::fs::remove_file(dir.join("content/blog/third.en.md")).unwrap();
    let clean = run(&dir, &["clean"]);
    assert!(clean.ok);
    assert!(
        dir.join("_site/en/blog/third/index.html").is_file(),
        "a file the plan no longer lists is not clean's to remove"
    );
    assert!(!dir.join("_site/en/blog/first").exists());
}

// ── query ─────────────────────────────────────────────────────────────────

#[test]
fn query_prints_the_model_without_bodies() {
    let dir = site("query");
    let query = run(&dir, &["query"]);
    assert!(query.ok, "{}", query.stderr);
    let model: serde_json::Value = serde_json::from_str(&query.stdout).unwrap();
    assert_eq!(model["version"], 1);
    assert_eq!(
        model["outputs"].as_u64().unwrap() as usize,
        planned_paths(&plan_of(&dir)).len()
    );
    assert_eq!(model["config"]["base_url"], "https://example.org");
    assert_eq!(model["config"]["extra"]["months_note"], "unused");
    assert_eq!(model["config"]["fallback_language"], "en");
    assert_eq!(model["languages"][1]["code"], "he");
    assert_eq!(model["languages"][1]["direction"], "rtl");
    let pages = model["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 8);
    for page in pages {
        assert!(
            page.get("content").is_none()
                && page.get("summary").is_none()
                && page.get("toc").is_none(),
            "{page}"
        );
        assert!(
            page["permalink"]
                .as_str()
                .unwrap()
                .starts_with("https://example.org/")
        );
    }
    let second = pages
        .iter()
        .find(|p| p["relative_path"] == "blog/second.en.md")
        .unwrap();
    assert_eq!(second["word_count"], 11);
    assert_eq!(second["updated"], "2026-03-01");
    assert_eq!(second["taxonomies"]["tags"][0]["name"], "religion");
    assert_eq!(second["next"]["title"], "First post");
    let sections = model["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 6);
    let blog = sections
        .iter()
        .find(|s| s["relative_path"] == "blog/_index.en.md")
        .unwrap();
    assert_eq!(blog["page_count"], 4);
    assert_eq!(
        blog["pages"][0],
        serde_json::json!({"relative_path": "blog/renamed.en.md", "permalink": "https://example.org/en/blog/renamed/"})
    );
    assert_eq!(blog["years"][0]["count"], 4);
    assert!(blog.get("content").is_none());
    let taxonomies = model["taxonomies"].as_array().unwrap();
    assert_eq!(taxonomies.len(), 2);
    let tags = taxonomies.iter().find(|t| t["lang"] == "en").unwrap();
    let religion = tags["terms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "religion")
        .unwrap();
    assert_eq!(
        religion["pages"],
        serde_json::json!(["blog/third.en.md", "blog/second.en.md", "blog/first.en.md"])
    );
    assert_eq!(religion["translations"][1]["name"], "דת");
}

// ── version, init, exit codes ─────────────────────────────────────────────

#[test]
fn version_prints_the_build_details() {
    let dir = temp_dir("version");
    let version = run(&dir, &["version"]);
    assert!(version.ok);
    assert!(
        version.stdout.starts_with(&format!(
            "rssite {} by Mark Veltzer",
            env!("CARGO_PKG_VERSION")
        )),
        "{}",
        version.stdout
    );
    for key in [
        "GIT_DESCRIBE:",
        "GIT_SHA:",
        "GIT_BRANCH:",
        "GIT_DIRTY:",
        "RUSTC_SEMVER:",
        "RUST_EDITION:",
        "BUILD_TIMESTAMP:",
    ] {
        assert!(
            version.stdout.contains(key),
            "{key} missing from:\n{}",
            version.stdout
        );
    }
    let flag = Command::new(binary()).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&flag.stdout).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn init_writes_into_a_named_directory_and_never_over_a_site() {
    let dir = temp_dir("init-dir");
    let init = run(&dir, &["init", "site"]);
    assert!(init.ok, "{}", init.stderr);
    assert!(
        init.stderr.contains("wrote 12 files into site"),
        "{}",
        init.stderr
    );
    assert!(dir.join("site/rssite.toml").is_file());
    assert!(dir.join("site/content/blog/hello.en.md").is_file());
    let again = run(&dir, &["init", "site"]);
    assert!(!again.ok);
    assert!(
        again.stderr.contains("rssite.toml already exists in site"),
        "{}",
        again.stderr
    );
    let build = run(&dir.join("site"), &["build"]);
    assert!(build.ok, "{}", build.stderr);
    // A directory with one of the files but no config is refused too.
    let partial = temp_dir("init-partial");
    write(&partial, "templates/base.html", "mine");
    let refused = run(&partial, &["init"]);
    assert!(!refused.ok);
    assert!(
        refused
            .stderr
            .contains("templates/base.html already exists; init only writes into an empty site"),
        "{}",
        refused.stderr
    );
    assert_eq!(read(&partial, "templates/base.html"), "mine");
    assert!(
        !partial.join("rssite.toml").exists(),
        "nothing else was written"
    );
}

#[test]
fn the_root_flag_runs_against_another_directory() {
    let dir = site("root-flag");
    let elsewhere = temp_dir("root-flag-cwd");
    let build = run(&elsewhere, &["-C", dir.to_str().unwrap(), "build"]);
    assert!(build.ok, "{}", build.stderr);
    assert!(dir.join("_site/en/index.html").is_file());
    assert!(!elsewhere.join("_site").exists());
    let long = run(
        &elsewhere,
        &["--root", dir.to_str().unwrap(), "--drafts", "plan"],
    );
    assert!(long.ok, "{}", long.stderr);
    let after = run(&elsewhere, &["plan", "-C", dir.to_str().unwrap()]);
    assert!(
        after.ok,
        "global flags may follow the command:\n{}",
        after.stderr
    );
}

#[test]
fn errors_exit_with_one_and_a_prefix() {
    let dir = temp_dir("exit-codes");
    let output = Command::new(binary())
        .arg("build")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("error: "), "{stderr}");
    assert!(stderr.contains("rssite.toml"), "{stderr}");
    let unknown = Command::new(binary())
        .arg("frobnicate")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2), "clap's usage error");
    let bad_flag = Command::new(binary())
        .args(["build", "--nope"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(bad_flag.status.code(), Some(2));
    let help = Command::new(binary())
        .arg("--help")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(
        text.contains("--drafts") && text.contains("--root"),
        "{text}"
    );
    let sub_help = Command::new(binary())
        .args(["build", "--help"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&sub_help.stdout).contains("--verify-determinism"));
}

#[test]
fn a_missing_or_broken_config_is_reported_by_path() {
    let dir = temp_dir("config-errors");
    let missing = run(&dir, &["plan"]);
    assert!(
        missing.stderr.contains("reading") && missing.stderr.contains("rssite.toml"),
        "{}",
        missing.stderr
    );
    write(
        &dir,
        "rssite.toml",
        "base_url = \"https://x\"\n[[languages]]\ncode = \"en\"\n",
    );
    let invalid = run(&dir, &["plan"]);
    assert!(
        invalid.stderr.contains("parsing") && invalid.stderr.contains("missing field `name`"),
        "{}",
        invalid.stderr
    );
    write(&dir, "rssite.toml", "base_url = \"https://x\"\n");
    let none = run(&dir, &["plan"]);
    assert!(
        none.stderr.contains("at least one [[languages]] entry"),
        "{}",
        none.stderr
    );
    let nowhere = run(&dir, &["-C", "does/not/exist", "plan"]);
    assert!(
        nowhere.stderr.contains("site root does/not/exist"),
        "{}",
        nowhere.stderr
    );
}

#[test]
fn verify_determinism_passes_on_the_fixture_with_every_feature_on() {
    let dir = site("determinism");
    prepend_config(&dir, "theme = \"t\"");
    write(&dir, "themes/t/static/t.txt", "t");
    write(
        &dir,
        "content/blog/old.en.md",
        "+++\ntitle = \"Old\"\ndate = 2020-01-01\n+++\n\n```rust\nfn a() {}\n```\n\n| a |\n|---|\n| 1 |\n",
    );
    let check = run(&dir, &["check"]);
    assert!(check.ok, "{}", check.stderr);
    let build = run(&dir, &["build", "--verify-determinism", "--plan"]);
    assert!(build.ok, "{}", build.stderr);
}

#[test]
fn get_url_fragments_optional_load_data_and_markdown_filter_options_work() {
    let dir = site("low-fixes");
    let cfg = read(&dir, "rssite.toml");
    write(
        &dir,
        "rssite.toml",
        &format!("{cfg}\n[markdown]\nsmart_punctuation = true\n"),
    );
    write(
        &dir,
        "content/about.en.tera",
        "+++\ntitle = \"About\"\ntemplate = \"page.html\"\ndate = 2026-01-01\n+++\n{{ get_url(path=\"@/blog/first.md#part\") }}|{{ load_data(required=false, path=\"data/missing.toml\") }}|{{ \"a -- b\" | markdown(inline=true) | safe }}",
    );
    let plan = plan_of(&dir);
    let about_sources = sources_of(&plan, "_site/en/about/index.html");
    assert!(
        about_sources.contains(&"content/blog/first.en.md".to_owned()),
        "{about_sources:?}"
    );
    assert!(
        !about_sources.contains(&"data/missing.toml".to_owned()),
        "missing optional load_data file must not be listed in sources: {about_sources:?}"
    );
    assert!(run(&dir, &["build"]).ok);
    let html = read(&dir, "_site/en/about/index.html");
    assert!(
        html.contains("https://example.org/en/blog/first/#part"),
        "{html}"
    );
    assert!(html.contains("a \u{2013} b"), "{html}");
}

#[test]
fn math_is_a_site_setting_a_page_can_override() {
    let dir = site("math");
    write(
        &dir,
        "content/blog/formula.en.md",
        "+++\ntitle = \"Formula\"\ndate = 2026-01-02\nmath = true\n+++\n\nArea $\\pi r^2$.\n",
    );
    write(
        &dir,
        "content/blog/prices.en.md",
        "+++\ntitle = \"Prices\"\ndate = 2026-01-03\n+++\n\nFrom $5 to $10.\n",
    );
    assert!(run(&dir, &["build"]).ok);
    let formula = read(&dir, "_site/en/blog/formula/index.html");
    assert!(
        formula.contains("<span class=\"math math-inline\">\\pi r^2</span>"),
        "{formula}"
    );
    let prices = read(&dir, "_site/en/blog/prices/index.html");
    assert!(prices.contains("From $5 to $10."), "{prices}");
}

#[test]
fn the_demo_site_checks() {
    let demos = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demos");
    let check = run(&demos, &["check"]);
    assert!(check.ok, "demos/ no longer builds:\n{}", check.stderr);
}

#[test]
fn code_is_left_to_right_only_in_a_right_to_left_language() {
    let dir = site("code-direction");
    for lang in ["en", "he"] {
        write(
            &dir,
            &format!("content/blog/code.{lang}.md"),
            "+++\ntitle = \"Code\"\ndate = 2026-01-04\n+++\n\nRun `make`.\n\n```sh\nmake\n```\n",
        );
    }
    let build = run(&dir, &["build"]);
    assert!(build.ok, "{}", build.stderr);
    let he = read(&dir, "_site/he/blog/code/index.html");
    assert!(he.contains("<code dir=\"ltr\">make</code>"), "{he}");
    assert!(he.contains("<pre dir=\"ltr\" class=\"highlight\">"), "{he}");
    let en = read(&dir, "_site/en/blog/code/index.html");
    assert!(
        !en.contains("<code dir=") && !en.contains("<pre dir="),
        "{en}"
    );
}

// ── section cascades ─────────────────────────────────────────────────────

/// A blog that cascades math and extra, a nested course section that adds
/// and overrides, and pages that take, override or ignore what comes down.
fn cascade_site(name: &str) -> std::path::PathBuf {
    let dir = site(name);
    write(
        &dir,
        "content/blog/_index.en.md",
        "+++\ntitle = \"Blog\"\nsort_by = \"date\"\n[cascade]\nmath = true\n[cascade.extra]\nbanner = \"blog\"\nseries = \"none\"\n+++\n",
    );
    write(
        &dir,
        "content/blog/course/_index.en.md",
        "+++\ntitle = \"Course\"\nsort_by = \"date\"\n[cascade]\nsmart_punctuation = true\n[cascade.extra]\nseries = \"course\"\n+++\n",
    );
    write(
        &dir,
        "content/blog/course/lesson.en.md",
        "+++\ntitle = \"Lesson\"\ndate = 2026-02-01\n[extra]\nlevel = 1\n+++\n\nArea $\\pi r^2$ -- done.\n",
    );
    write(
        &dir,
        "content/blog/course/plain.en.md",
        "+++\ntitle = \"Plain\"\ndate = 2026-02-02\nmath = false\nsmart_punctuation = false\n[extra]\nbanner = \"own\"\n+++\n\nFrom $5 to $10 -- done.\n",
    );
    dir
}

#[test]
fn a_section_cascade_reaches_every_page_below_it_nearest_first() {
    let dir = cascade_site("cascade");
    let build = run(&dir, &["build"]);
    assert!(build.ok, "{}", build.stderr);
    let lesson = read(&dir, "_site/en/blog/course/lesson/index.html");
    assert!(
        lesson.contains("<span class=\"math math-inline\">\\pi r^2</span>"),
        "math from the blog, two levels up:\n{lesson}"
    );
    assert!(
        lesson.contains("\u{2013} done."),
        "smart punctuation from the course:\n{lesson}"
    );
    let plain = read(&dir, "_site/en/blog/course/plain/index.html");
    assert!(
        plain.contains("From $5 to $10 -- done."),
        "the page's own settings win:\n{plain}"
    );

    let query = run(&dir, &["query"]);
    assert!(query.ok, "{}", query.stderr);
    let site: serde_json::Value = serde_json::from_str(&query.stdout).unwrap();
    let extra_of = |permalink_end: &str| {
        site["pages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["permalink"].as_str().unwrap().ends_with(permalink_end))
            .unwrap_or_else(|| panic!("no page {permalink_end}"))["extra"]
            .clone()
    };
    let lesson = extra_of("/en/blog/course/lesson/");
    assert_eq!(lesson["banner"], "blog", "{lesson}");
    assert_eq!(
        lesson["series"], "course",
        "the nearer section wins: {lesson}"
    );
    assert_eq!(lesson["level"], 1, "{lesson}");
    assert_eq!(extra_of("/en/blog/course/plain/")["banner"], "own");
    let first = extra_of("/en/blog/first/");
    assert_eq!(first["series"], "none", "the blog's own page: {first}");
    assert!(
        extra_of("/he/blog/first/").get("banner").is_none(),
        "a cascade is per language"
    );
    let course = site["sections"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| {
            s["permalink"]
                .as_str()
                .unwrap()
                .ends_with("/en/blog/course/")
        })
        .unwrap();
    assert_eq!(
        course["extra"]["banner"], "blog",
        "subsections receive it too"
    );
}

#[test]
fn a_page_depends_on_the_sections_whose_cascade_it_takes() {
    let dir = cascade_site("cascade-sources");
    // A template that reads no other page, so the sources are exactly the
    // page's own and what its front matter came from.
    write(&dir, "templates/bare.html", "{{ page.content | safe }}");
    let lesson = read(&dir, "content/blog/course/lesson.en.md");
    write(
        &dir,
        "content/blog/course/lesson.en.md",
        &lesson.replace(
            "date = 2026-02-01\n",
            "date = 2026-02-01\ntemplate = \"bare.html\"\n",
        ),
    );
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/en/blog/course/lesson/index.html");
    assert!(
        sources.contains(&"content/blog/_index.en.md".to_owned()),
        "{sources:?}"
    );
    assert!(
        sources.contains(&"content/blog/course/_index.en.md".to_owned()),
        "{sources:?}"
    );
    assert!(
        !sources.contains(&"content/blog/first.en.md".to_owned()),
        "not everything, only the cascade's sections: {sources:?}"
    );
}

#[test]
fn a_cascade_sets_only_what_means_the_same_below() {
    for (cascade, error) in [
        (
            "template = \"x.html\"",
            "cascade can set math, smart_punctuation, extra; \"template\" is not one of them",
        ),
        (
            "math = \"yes\"",
            "cascade.math must be true or false, found a string",
        ),
        (
            "extra = 3",
            "cascade.extra must be a table, found a integer",
        ),
    ] {
        let dir = site("cascade-bad");
        write(
            &dir,
            "content/blog/_index.en.md",
            &format!("+++\ntitle = \"Blog\"\nsort_by = \"date\"\n[cascade]\n{cascade}\n+++\n"),
        );
        let check = run(&dir, &["check"]);
        assert!(!check.ok, "accepted: {cascade}");
        assert!(
            check.stderr.contains("content file blog/_index.en.md"),
            "{}",
            check.stderr
        );
        assert!(check.stderr.contains(error), "{}", check.stderr);
    }
}

// ── shortcodes ───────────────────────────────────────────────────────────

fn shortcode_site(name: &str) -> std::path::PathBuf {
    let dir = site(name);
    write(
        &dir,
        "templates/shortcodes/youtube.html",
        "<iframe class=\"yt\" data-nth=\"{{ nth }}\" src=\"https://www.youtube-nocookie.com/embed/{{ id }}{% if start %}?start={{ start }}{% endif %}\"></iframe>",
    );
    write(
        &dir,
        "templates/shortcodes/note.html",
        "<aside class=\"note note-{{ kind }}\" lang=\"{{ lang }}\">{{ body | markdown | safe }}<small>{{ page.title }}</small></aside>",
    );
    write(
        &dir,
        "content/blog/shortcodes.en.md",
        "+++\ntitle = \"Shortcodes\"\ndate = 2026-05-01\n+++\n\nIntro {{ youtube(id=\"abc\") }} inline.\n\n{{ youtube(id=\"def\", start=30) }}\n\n<!-- more -->\n\n{% note(kind=\"tip\") %}\nSome *markdown* here.\n{% end %}\n\nLiteral: `{{ youtube(id=\"x\") }}`\n",
    );
    dir
}

#[test]
fn shortcodes_render_their_templates_where_they_are_called() {
    let dir = shortcode_site("shortcodes");
    let build = run(&dir, &["build"]);
    assert!(build.ok, "{}", build.stderr);
    let html = read(&dir, "_site/en/blog/shortcodes/index.html");
    assert!(
        html.contains("<p>Intro <iframe class=\"yt\" data-nth=\"1\" src=\"https://www.youtube-nocookie.com/embed/abc\"></iframe> inline.</p>"),
        "inline call, inside its paragraph:\n{html}"
    );
    assert!(
        html.contains("<iframe class=\"yt\" data-nth=\"2\" src=\"https://www.youtube-nocookie.com/embed/def?start=30\"></iframe>"),
        "{html}"
    );
    assert!(
        !html.contains("<p><iframe class=\"yt\" data-nth=\"2\""),
        "a call alone on its line is not wrapped in a paragraph:\n{html}"
    );
    assert!(
        html.contains("<aside class=\"note note-tip\" lang=\"en\"><p>Some <em>markdown</em> here.</p>\n<small>Shortcodes</small></aside>"),
        "a body, the page and the language reach the template:\n{html}"
    );
    assert!(
        html.contains("<code>{{ youtube(id=\"x\") }}</code>"),
        "a call in code is text:\n{html}"
    );
    assert!(
        !html.contains("rssite-shortcode"),
        "no marker left:\n{html}"
    );

    // Listings and feeds carry the rendered summary, not the markers.
    let blog = read(&dir, "_site/en/blog/atom.xml");
    assert!(blog.contains("youtube-nocookie.com/embed/abc"), "{blog}");
    assert!(!blog.contains("rssite-shortcode"), "{blog}");
}

#[test]
fn a_page_depends_on_the_shortcodes_it_calls() {
    let dir = shortcode_site("shortcode-sources");
    let plan = plan_of(&dir);
    let sources = sources_of(&plan, "_site/en/blog/shortcodes/index.html");
    for template in [
        "templates/shortcodes/youtube.html",
        "templates/shortcodes/note.html",
    ] {
        assert!(
            sources.contains(&template.to_owned()),
            "{template}: {sources:?}"
        );
    }
    let other = sources_of(&plan, "_site/en/about/index.html");
    assert!(
        !other.contains(&"templates/shortcodes/youtube.html".to_owned()),
        "{other:?}"
    );
}

#[test]
fn calling_a_shortcode_that_does_not_exist_names_the_file_and_line() {
    let dir = site("shortcode-unknown");
    write(
        &dir,
        "content/blog/bad.en.md",
        "+++\ntitle = \"Bad\"\ndate = 2026-03-02\n+++\n\nText.\n\n{{ gallery(dir=\"x\") }}\n",
    );
    let check = run(&dir, &["check"]);
    assert!(!check.ok);
    assert!(
        check.stderr.contains("content file blog/bad.en.md"),
        "{}",
        check.stderr
    );
    assert!(
        check
            .stderr
            .contains("line 8: no shortcode \"gallery\"; add templates/shortcodes/gallery.html"),
        "{}",
        check.stderr
    );
}

// ── images ────────────────────────────────────────────────────────────────

/// A site whose post shows `static/img/wide.png` (1000 x 500) and
/// `static/img/small.png` (300 x 200), with `[images]` set by `config`.
fn image_site(name: &str, config: &str) -> std::path::PathBuf {
    let dir = site(name);
    std::fs::create_dir_all(dir.join("static/img")).unwrap();
    for (file, w, h) in [("wide.png", 1000, 500), ("small.png", 300, 200)] {
        image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
        })
        .save(dir.join("static/img").join(file))
        .unwrap();
    }
    write(
        &dir,
        "content/blog/pictures.en.md",
        "+++\ntitle = \"Pictures\"\ndate = 2026-05-01\n+++\n\n![A *wide* one](/img/wide.png \"Wide\")\n\n![Small](../../../img/small.png)\n\n![Elsewhere](https://example.org/x.png)\n",
    );
    append_config(&dir, config);
    dir
}

#[test]
fn markdown_images_become_pictures_with_their_variants() {
    let dir = image_site(
        "images",
        "[images]\nenabled = true\nwidths = [400, 800]\nsizes = \"50vw\"",
    );
    build_ok(&dir);
    let html = read(&dir, "_site/en/blog/pictures/index.html");
    assert!(
        html.contains("<picture><source type=\"image/webp\" srcset=\"/img/wide-400w.webp 400w, /img/wide-800w.webp 800w\" sizes=\"50vw\" /><img src=\"/img/wide-800w.png\" srcset=\"/img/wide-400w.png 400w, /img/wide-800w.png 800w\" sizes=\"50vw\" width=\"800\" height=\"400\" alt=\"A wide one\" title=\"Wide\" loading=\"lazy\" decoding=\"async\" /></picture>"),
        "an image wider than every width is never sent whole:\n{html}"
    );
    assert!(
        html.contains("<picture><source type=\"image/webp\" srcset=\"../../../img/small-300w.webp 300w\" sizes=\"50vw\" /><img src=\"../../../img/small.png\" width=\"300\" height=\"200\" alt=\"Small\" loading=\"lazy\" decoding=\"async\" /></picture>"),
        "a relative URL stays relative, and a small image keeps its own size:\n{html}"
    );
    assert!(
        html.contains("<img src=\"https://example.org/x.png\" alt=\"Elsewhere\" />"),
        "another host's image is left alone:\n{html}"
    );

    let webp = image::open(dir.join("_site/img/wide-800w.webp")).unwrap();
    assert_eq!((webp.width(), webp.height()), (800, 400));
    let png = image::open(dir.join("_site/img/wide-400w.png")).unwrap();
    assert_eq!((png.width(), png.height()), (400, 200));
    assert!(
        dir.join("_site/img/wide.png").is_file(),
        "the original is still published"
    );

    let plan = plan_of(&dir);
    assert_eq!(
        sources_of(&plan, "_site/img/wide-800w.webp"),
        ["rssite.toml", "static/img/wide.png"]
    );
    let page = sources_of(&plan, "_site/en/blog/pictures/index.html");
    assert!(
        page.contains(&"static/img/wide.png".to_owned())
            && page.contains(&"static/img/small.png".to_owned()),
        "the page's width and height come from its images: {page:?}"
    );
    let planned = planned_paths(&plan);
    for file in ["_site/img/wide-400w.png", "_site/img/small-300w.webp"] {
        assert!(planned.contains(&file.to_owned()), "{file} not planned");
    }
}

#[test]
fn images_are_left_alone_unless_enabled() {
    let dir = image_site("images-off", "");
    build_ok(&dir);
    let html = read(&dir, "_site/en/blog/pictures/index.html");
    assert!(
        html.contains("<img src=\"/img/wide.png\" alt=\"A wide one\" title=\"Wide\" />"),
        "{html}"
    );
    assert!(!dir.join("_site/img/wide-480w.png").exists());
}

#[test]
fn an_unknown_image_format_fails_the_build() {
    let dir = image_site(
        "images-bad",
        "[images]\nenabled = true\nformats = [\"jxl\"]",
    );
    let error = build_err(&dir);
    assert!(error.contains("images.formats has \"jxl\""), "{error}");
}

#[test]
fn a_variant_colliding_with_a_static_file_is_named() {
    let dir = image_site("images-clash", "[images]\nenabled = true\nwidths = [400]");
    write(&dir, "static/img/wide-400w.webp", "already here");
    let error = build_err(&dir);
    assert!(
        error.contains("two outputs would be written to img/wide-400w.webp")
            && error.contains("a variant of image static/img/wide.png"),
        "{error}"
    );
}

#[test]
fn templates_read_an_image_size_and_depend_on_the_image() {
    let dir = image_site("image-metadata", "");
    write(
        &dir,
        "templates/page.html",
        "{% set m = get_image_metadata(path=\"static/img/wide.png\") %}<meta content=\"{{ m.width }}x{{ m.height }}\">",
    );
    build_ok(&dir);
    let html = read(&dir, "_site/en/blog/pictures/index.html");
    assert!(html.contains("<meta content=\"1000x500\">"), "{html}");
    let plan = plan_of(&dir);
    assert!(
        sources_of(&plan, "_site/en/blog/pictures/index.html")
            .contains(&"static/img/wide.png".to_owned())
    );
}
