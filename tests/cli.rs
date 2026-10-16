//! End-to-end tests through the binary: the scaffolded site and the
//! bilingual fixture build, the plan is a manifest, the build is
//! deterministic, and every silent failure of the previous generator is a
//! loud error here.

use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rssite"))
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bilingual")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rssite-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn run(dir: &Path, args: &[&str]) -> Run {
    let output = Command::new(binary())
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    Run {
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}"))
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in walk(dir) {
        out.push(
            entry
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn bare_invocation_lists_the_commands() {
    let dir = temp_dir("bare");
    let output = Command::new(binary()).current_dir(&dir).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    for command in [
        "plan", "build", "check", "query", "clean", "serve", "init", "version",
    ] {
        assert!(
            stderr.contains(command),
            "{command} missing from:\n{stderr}"
        );
    }
    assert!(
        !stderr.contains("--drafts"),
        "flags belong to --help only:\n{stderr}"
    );
    let with_flag = Command::new(binary())
        .arg("--drafts")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(with_flag.status.code(), Some(2));
}

#[test]
fn scaffolded_site_builds_deterministically() {
    let dir = temp_dir("init");
    let init = run(&dir, &["init"]);
    assert!(init.ok, "{}", init.stderr);
    let build = run(&dir, &["build", "--verify-determinism"]);
    assert!(build.ok, "{}", build.stderr);
    let page = read(&dir, "_site/en/blog/hello/index.html");
    assert!(page.contains("1 January 2026"), "{page}");
    assert!(page.contains("class=\"highlight\""));
    assert!(
        page.contains("href=\"https://example.org/en/tags/meta/\""),
        "{page}"
    );
    assert!(
        !page.contains("&#x2F;"),
        "slashes must not be entity-escaped:\n{page}"
    );
    assert!(dir.join("_site/en/atom.xml").is_file());
    assert!(dir.join("_site/sitemap.xml").is_file());
    assert!(dir.join("_site/style.css").is_file());
    assert!(dir.join("_site/highlight.css").is_file());
    assert!(read(&dir, "_site/index.html").contains("url=https://example.org/en/"));

    let check = run(&dir, &["check"]);
    assert!(check.ok, "{}", check.stderr);

    let plan = run(&dir, &["plan"]);
    assert!(plan.ok, "{}", plan.stderr);
    let manifest: serde_json::Value = serde_json::from_str(&plan.stdout).unwrap();
    assert_eq!(manifest["version"], 1);
    let outputs = manifest["outputs"].as_array().unwrap();
    let hello = outputs
        .iter()
        .find(|o| o["path"] == "_site/en/blog/hello/index.html")
        .expect("hello in plan");
    let sources: Vec<&str> = hello["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(sources.contains(&"content/blog/hello.en.md"), "{sources:?}");
    assert!(sources.contains(&"templates/page.html"), "{sources:?}");
    assert!(sources.contains(&"rssite.toml"), "{sources:?}");
    let planned: Vec<&str> = outputs
        .iter()
        .map(|o| o["path"].as_str().unwrap())
        .collect();
    let built: Vec<String> = files_under(&dir.join("_site"))
        .iter()
        .map(|f| format!("_site/{f}"))
        .collect();
    assert_eq!(
        planned,
        built.iter().map(String::as_str).collect::<Vec<_>>(),
        "plan and build disagree"
    );

    let again = run(&dir, &["build"]);
    assert!(again.ok);
    assert!(again.stderr.contains("0 written"), "{}", again.stderr);

    let clean = run(&dir, &["clean"]);
    assert!(clean.ok, "{}", clean.stderr);
    assert!(files_under(&dir.join("_site")).is_empty());
}

#[test]
fn bilingual_fixture_builds() {
    let dir = temp_dir("bilingual");
    copy_dir(&fixture(), &dir);
    let build = run(&dir, &["build", "--verify-determinism"]);
    assert!(build.ok, "{}", build.stderr);

    let he_first = read(&dir, "_site/he/blog/first/index.html");
    assert!(he_first.contains("<html lang=\"he\" dir=\"rtl\">"));
    assert!(
        he_first.contains("10 ינואר 2026"),
        "Hebrew month name expected:\n{he_first}"
    );
    assert!(he_first.contains("hreflang=\"en\" href=\"https://example.org/en/blog/first/\""));
    assert!(
        he_first.contains("class=\"lang\" href=\"https://example.org/en/blog/first/\""),
        "switcher:\n{he_first}"
    );
    assert!(he_first.contains("href=\"https://example.org/he/tags/דת/\""));

    let en_first = read(&dir, "_site/en/blog/first/index.html");
    assert!(en_first.contains("href=\"https://example.org/en/blog/second/\""));
    assert!(en_first.contains("href=\"https://example.org/he/blog/second/#top\""));
    assert!(en_first.contains("rel=\"prev\" href=\"https://example.org/en/blog/second/\""));

    let he_app = read(&dir, "_site/he/apps/index.html");
    assert!(he_app.contains("<h1>יישום</h1>"));
    assert!(
        he_app.contains("const DATA = [1, 2, 3];"),
        "body_from must pull the English body:\n{he_app}"
    );
    assert!(
        he_app.contains("*not emphasised*"),
        "raw HTML must not be markdown-processed"
    );

    let about = read(&dir, "_site/en/about/index.html");
    assert!(about.contains("this page is called About and the nav says Blog"));
    assert!(
        about.contains(&format!("made by rssite {}", env!("CARGO_PKG_VERSION"))),
        "the generator names itself:\n{about}"
    );

    let second = read(&dir, "_site/en/blog/second/index.html");
    assert!(second.contains("id=\"heading-one\"") && second.contains("id=\"heading-one-1\""));

    // Year archives: every dated post of a year, in both languages, linked
    // from the section's listing and from each other.
    let archive = read(&dir, "_site/en/blog/2026/index.html");
    for post in ["first", "second", "third", "renamed"] {
        assert!(
            archive.contains(&format!("href=\"https://example.org/en/blog/{post}/\"")),
            "{post} missing from the 2026 archive:\n{archive}"
        );
    }
    assert!(archive.contains("2026*"), "{archive}");
    assert!(
        archive.contains("hreflang=\"he\" href=\"https://example.org/he/blog/2026/\""),
        "{archive}"
    );
    assert!(dir.join("_site/he/blog/2026/index.html").is_file());
    let blog = read(&dir, "_site/en/blog/index.html");
    assert!(
        blog.contains("class=\"year\" href=\"https://example.org/en/blog/2026/\">2026 (4)"),
        "{blog}"
    );
    assert!(read(&dir, "_site/sitemap.xml").contains("https://example.org/en/blog/2026/"));
    assert!(dir.join("_site/en/blog/page/2/index.html").is_file());
    assert!(
        !dir.join("_site/en/blog/page/1").exists(),
        "no /page/1/ stub"
    );
    let page2 = read(&dir, "_site/en/blog/page/2/index.html");
    assert!(page2.contains("rel=\"prev\" href=\"https://example.org/en/blog/\""));

    let he_tag = read(&dir, "_site/he/tags/דת/index.html");
    assert!(
        he_tag.contains("hreflang=\"en\" href=\"https://example.org/en/tags/religion/\""),
        "term translations:\n{he_tag}"
    );
    assert!(
        dir.join("_site/en/tags/religion/page/2/index.html")
            .is_file()
    );
    assert!(dir.join("_site/en/tags/religion/atom.xml").is_file());

    assert!(
        read(&dir, "_site/legacy/third.html").contains("url=https://example.org/en/blog/third/")
    );
    assert!(
        read(&dir, "_site/old/third/index.html")
            .contains("rel=\"canonical\" href=\"https://example.org/en/blog/third/\"")
    );

    let sitemap = read(&dir, "_site/sitemap.xml");
    assert!(sitemap.contains("<loc>https://example.org/he/blog/רביעי/</loc>"));
    assert!(sitemap.contains("hreflang=\"he\" href=\"https://example.org/he/blog/רביעי/\""));
    assert!(!sitemap.contains("/page/1/"));

    assert!(read(&dir, "_site/style.css").contains("40rem"));
    assert!(
        dir.join("_site/en/blog/data.csv").is_file()
            && dir.join("_site/he/blog/data.csv").is_file()
    );
    assert!(read(&dir, "_site/index.html").contains("hreflang=\"he\""));
    assert!(read(&dir, "_site/robots.txt").contains("Sitemap: https://example.org/sitemap.xml"));

    let feed = read(&dir, "_site/en/atom.xml");
    assert_eq!(
        feed.matches("<entry").count(),
        2,
        "feeds.limit = 2:\n{feed}"
    );
    assert!(feed.contains("<updated>2026-04-01T00:00:00+00:00</updated>"));

    let query = run(&dir, &["query"]);
    assert!(query.ok, "{}", query.stderr);
    let model: serde_json::Value = serde_json::from_str(&query.stdout).unwrap();
    assert_eq!(model["languages"].as_array().unwrap().len(), 2);
    let renamed = model["pages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["relative_path"] == "blog/renamed.en.md")
        .unwrap();
    let paired: Vec<&str> = renamed["translations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["lang"].as_str().unwrap())
        .collect();
    assert_eq!(paired, vec!["en", "he"], "translation_key pairing");
}

#[test]
fn a_plan_reader_that_closes_the_pipe_early_is_not_an_error() {
    use std::io::Read;
    let dir = temp_dir("plan-pipe");
    copy_dir(&fixture(), &dir);
    let mut child = Command::new(binary())
        .arg("plan")
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Read a little, then close our end while rssite still has most of the
    // manifest to write.
    let mut stdout = child.stdout.take().unwrap();
    let mut first = [0u8; 64];
    let n = stdout.read(&mut first).unwrap();
    assert!(n > 0);
    drop(stdout);
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(output.status.success(), "{stderr}");
}

#[test]
fn drafts_are_excluded_unless_asked() {
    let dir = temp_dir("drafts");
    copy_dir(&fixture(), &dir);
    std::fs::write(
        dir.join("content/blog/draft.en.md"),
        "+++\ntitle = \"Draft\"\ndate = 2026-05-01\ndraft = true\n+++\n\nnot yet\n",
    )
    .unwrap();
    assert!(run(&dir, &["build"]).ok);
    assert!(!dir.join("_site/en/blog/draft").exists());
    assert!(run(&dir, &["build", "--drafts"]).ok);
    assert!(dir.join("_site/en/blog/draft/index.html").is_file());
}

fn expect_error(name: &str, mutate: impl FnOnce(&Path), needle: &str) {
    let dir = temp_dir(name);
    copy_dir(&fixture(), &dir);
    mutate(&dir);
    let build = run(&dir, &["check"]);
    assert!(!build.ok, "expected {name} to fail");
    assert!(
        build.stderr.contains(needle),
        "expected {needle:?} in:\n{}",
        build.stderr
    );
}

#[test]
fn unsuffixed_content_file_is_an_error() {
    expect_error(
        "nolang",
        |dir| {
            std::fs::write(
                dir.join("content/blog/nolang.md"),
                "+++\ntitle = \"x\"\ndate = 2026-01-01\n+++\n",
            )
            .unwrap()
        },
        "language suffix",
    );
}

#[test]
fn unknown_language_suffix_is_an_error() {
    expect_error(
        "frlang",
        |dir| {
            std::fs::write(
                dir.join("content/blog/post.fr.md"),
                "+++\ntitle = \"x\"\ndate = 2026-01-01\n+++\n",
            )
            .unwrap()
        },
        "not a configured language",
    );
}

#[test]
fn broken_internal_link_is_an_error() {
    expect_error(
        "brokenlink",
        |dir| {
            std::fs::write(
                dir.join("content/blog/broken.en.md"),
                "+++\ntitle = \"x\"\ndate = 2026-01-01\n+++\n[x](@/blog/missing.md)\n",
            )
            .unwrap()
        },
        "no content file matches",
    );
}

#[test]
fn missing_translation_key_is_an_error() {
    expect_error(
        "transkey",
        |dir| {
            let path = dir.join("templates/page.html");
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("{{ page.title }}", "{{ trans(key=\"nope\") }}");
            std::fs::write(path, text).unwrap();
        },
        "translation key \"nope\" is missing",
    );
}

#[test]
fn clock_is_not_available_to_templates() {
    expect_error(
        "now",
        |dir| {
            let path = dir.join("templates/base.html");
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("<main>", "<main>{{ now() }}");
            std::fs::write(path, text).unwrap();
        },
        "now() is not available",
    );
}

#[test]
fn unknown_config_key_is_an_error() {
    expect_error(
        "configkey",
        |dir| {
            let path = dir.join("rssite.toml");
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("author = ", "output_dirr = \"x\"\nauthor = ");
            std::fs::write(path, text).unwrap();
        },
        "unknown field",
    );
}

#[test]
fn undeclared_taxonomy_is_an_error() {
    expect_error(
        "taxonomy",
        |dir| {
            std::fs::write(
                dir.join("content/blog/cat.en.md"),
                "+++\ntitle = \"x\"\ndate = 2026-01-01\n[taxonomies]\ncategories = [\"a\"]\n+++\n",
            )
            .unwrap()
        },
        "not declared in rssite.toml",
    );
}

#[test]
fn undated_page_in_date_sorted_section_is_an_error() {
    expect_error(
        "undated",
        |dir| {
            std::fs::write(
                dir.join("content/blog/undated.en.md"),
                "+++\ntitle = \"x\"\n+++\n",
            )
            .unwrap()
        },
        "sorts by date but these pages have none",
    );
}

#[test]
fn colliding_urls_are_an_error() {
    expect_error(
        "collision",
        |dir| {
            std::fs::write(
                dir.join("content/blog/other.en.md"),
                "+++\ntitle = \"x\"\ndate = 2026-01-01\nslug = \"first\"\n+++\n",
            )
            .unwrap()
        },
        "both render to",
    );
}

#[test]
fn unknown_static_url_is_an_error() {
    expect_error(
        "geturl",
        |dir| {
            let path = dir.join("templates/base.html");
            let text = std::fs::read_to_string(&path).unwrap().replace(
                "get_url(path=\"style.css\")",
                "get_url(path=\"missing.css\")",
            );
            std::fs::write(path, text).unwrap();
        },
        "not a file this build produces",
    );
}

// ── themes and published directories ──────────────────────────────────────

/// Top-level keys must precede every table in TOML, so a key goes first.
fn prepend_config(dir: &Path, text: &str) {
    let path = dir.join("rssite.toml");
    let old = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{text}\n{old}")).unwrap();
}

/// A table goes last; a new header closes whatever table came before.
fn append_config(dir: &Path, text: &str) {
    let path = dir.join("rssite.toml");
    let old = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{old}\n{text}\n")).unwrap();
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn sources_of(manifest: &serde_json::Value, path: &str) -> Vec<String> {
    manifest["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["path"] == path)
        .unwrap_or_else(|| panic!("no output {path} in the manifest"))["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn theme_directory_sits_behind_the_site() {
    let dir = temp_dir("theme");
    copy_dir(&fixture(), &dir);
    prepend_config(&dir, "theme = \"fleet\"");
    // The theme supplies what the site lacks: the term template moves there.
    std::fs::rename(dir.join("templates/taxonomy_single.html"), {
        std::fs::create_dir_all(dir.join("themes/fleet/templates")).unwrap();
        dir.join("themes/fleet/templates/taxonomy_single.html")
    })
    .unwrap();
    // The site's file wins over the theme's of the same name.
    write(
        &dir,
        "themes/fleet/templates/base.html",
        "THEME BASE {% block content %}{% endblock %}",
    );
    write(&dir, "themes/fleet/static/favicon.svg", "<svg>theme</svg>");
    write(
        &dir,
        "themes/fleet/static/theme-only.txt",
        "from the theme\n",
    );
    write(
        &dir,
        "themes/fleet/sass/extra.scss",
        ".theme { color: red; }\n",
    );
    write(&dir, "themes/fleet/sass/_tokens.scss", "$accent: blue;\n");
    // The site's stylesheet can use a theme partial, and a theme stylesheet
    // the site's.
    let style = read(&dir, "sass/style.scss");
    write(
        &dir,
        "sass/style.scss",
        &format!("@use \"tokens\";\n{style}\n.site {{ color: tokens.$accent; }}\n"),
    );
    write(
        &dir,
        "themes/fleet/sass/more.scss",
        "@use \"vars\";\n.more { margin: 0; }\n",
    );
    // Translations layer: the site's key wins, the theme's extra key shows.
    write(
        &dir,
        "themes/fleet/i18n/en.toml",
        "nav_blog = \"WRONG\"\ntheme_key = \"from theme\"\n",
    );
    write(
        &dir,
        "themes/fleet/i18n/he.toml",
        "theme_key = \"מהערכה\"\n",
    );
    let base = read(&dir, "templates/base.html");
    write(
        &dir,
        "templates/base.html",
        &base.replace(
            "</body>",
            "<p id=\"theme\">{{ trans(key=\"theme_key\") }}</p></body>",
        ),
    );

    let build = run(&dir, &["build", "--verify-determinism"]);
    assert!(build.ok, "{}", build.stderr);

    let en_first = read(&dir, "_site/en/blog/first/index.html");
    assert!(
        !en_first.contains("THEME BASE"),
        "the site's base.html must win"
    );
    assert!(
        en_first.contains(">Blog<"),
        "the site's translation must win:\n{en_first}"
    );
    assert!(
        en_first.contains("from theme"),
        "the theme's extra key must show:\n{en_first}"
    );
    assert!(read(&dir, "_site/he/blog/first/index.html").contains("מהערכה"));
    assert_eq!(
        read(&dir, "_site/favicon.svg"),
        read(&dir, "static/favicon.svg")
    );
    assert_eq!(read(&dir, "_site/theme-only.txt"), "from the theme\n");
    assert!(read(&dir, "_site/extra.css").contains(".theme"));
    assert!(read(&dir, "_site/more.css").contains(".more"));
    assert!(read(&dir, "_site/style.css").contains("blue"));
    assert!(dir.join("_site/en/tags/index.html").is_file());

    let plan = run(&dir, &["plan"]);
    assert!(plan.ok, "{}", plan.stderr);
    let manifest: serde_json::Value = serde_json::from_str(&plan.stdout).unwrap();
    let first = sources_of(&manifest, "_site/en/blog/first/index.html");
    assert!(
        first.contains(&"templates/base.html".to_owned()),
        "{first:?}"
    );
    assert!(
        !first.contains(&"themes/fleet/templates/base.html".to_owned()),
        "{first:?}"
    );
    assert!(
        first.contains(&"i18n/en.toml".to_owned())
            && first.contains(&"themes/fleet/i18n/en.toml".to_owned()),
        "{first:?}"
    );
    let tags = sources_of(&manifest, "_site/en/tags/index.html");
    assert!(
        tags.contains(&"templates/taxonomy_list.html".to_owned()),
        "{tags:?}"
    );
    let term = manifest["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| {
            let path = o["path"].as_str().unwrap();
            path.starts_with("_site/en/tags/")
                && path.ends_with("/index.html")
                && path != "_site/en/tags/index.html"
                && !path.contains("/page/")
        })
        .expect("a term page");
    assert!(
        term["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "themes/fleet/templates/taxonomy_single.html"),
        "{term}"
    );
    assert_eq!(
        sources_of(&manifest, "_site/theme-only.txt"),
        ["themes/fleet/static/theme-only.txt"]
    );
    assert_eq!(
        sources_of(&manifest, "_site/favicon.svg"),
        ["static/favicon.svg"]
    );
    let extra = sources_of(&manifest, "_site/extra.css");
    assert!(
        extra.contains(&"sass/_vars.scss".to_owned())
            && extra.contains(&"themes/fleet/sass/extra.scss".to_owned()),
        "{extra:?}"
    );
}

#[test]
fn missing_theme_is_an_error() {
    expect_error(
        "theme-missing",
        |dir| prepend_config(dir, "theme = \"nope\""),
        "theme \"nope\" not found",
    );
}

#[test]
fn theme_translation_for_an_undeclared_language_is_an_error() {
    expect_error(
        "theme-fr",
        |dir| {
            prepend_config(dir, "theme = \"fleet\"");
            write(dir, "themes/fleet/i18n/fr.toml", "x = \"y\"\n");
        },
        "themes/fleet/i18n/fr.toml is for language \"fr\"",
    );
}

#[test]
fn published_directories_are_copied_as_they_are() {
    let dir = temp_dir("publish");
    copy_dir(&fixture(), &dir);
    append_config(
        &dir,
        "[[publish]]\nfrom = \"shared/tokens\"\nto = \"tokens\"\nfiles = [\"*.css\", \"fonts/*\"]",
    );
    write(&dir, "shared/tokens/a.css", ":root { --x: 1; }\n");
    write(&dir, "shared/tokens/b.txt", "not published\n");
    write(&dir, "shared/tokens/fonts/x.woff2", "font bytes\n");
    write(&dir, "shared/tokens/fonts/deep/y.woff2", "too deep\n");

    let build = run(&dir, &["build", "--verify-determinism"]);
    assert!(build.ok, "{}", build.stderr);
    assert_eq!(read(&dir, "_site/tokens/a.css"), ":root { --x: 1; }\n");
    assert_eq!(read(&dir, "_site/tokens/fonts/x.woff2"), "font bytes\n");
    assert!(!dir.join("_site/tokens/b.txt").exists());
    assert!(!dir.join("_site/tokens/fonts/deep").exists());

    let plan = run(&dir, &["plan"]);
    let manifest: serde_json::Value = serde_json::from_str(&plan.stdout).unwrap();
    assert_eq!(
        sources_of(&manifest, "_site/tokens/a.css"),
        ["shared/tokens/a.css"]
    );
    assert_eq!(
        sources_of(&manifest, "_site/tokens/fonts/x.woff2"),
        ["shared/tokens/fonts/x.woff2"]
    );
}

#[test]
fn a_publish_entry_that_matches_nothing_is_an_error() {
    expect_error(
        "publish-empty",
        |dir| {
            append_config(
                dir,
                "[[publish]]\nfrom = \"shared/tokens\"\nto = \"tokens\"\nfiles = [\"*.css\"]",
            );
            write(dir, "shared/tokens/b.txt", "x\n");
        },
        "matches no file",
    );
    expect_error(
        "publish-missing",
        |dir| append_config(dir, "[[publish]]\nfrom = \"shared/none\"\nto = \"x\""),
        "is not a directory",
    );
}
