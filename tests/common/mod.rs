//! Helpers shared by the end-to-end test binaries: running the built
//! binary in a scratch copy of a fixture, reading what it wrote, and
//! editing the fixture's config and files between runs.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rssite"))
}

pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bilingual")
}

pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rssite-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn copy_dir(from: &Path, to: &Path) {
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

pub struct Run {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(dir: &Path, args: &[&str]) -> Run {
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

pub fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_else(|e| panic!("reading {rel}: {e}"))
}

pub fn files_under(dir: &Path) -> Vec<String> {
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

pub fn walk(dir: &Path) -> Vec<PathBuf> {
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

pub fn expect_error(name: &str, mutate: impl FnOnce(&Path), needle: &str) {
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

/// Top-level keys must precede every table in TOML, so a key goes first.
pub fn prepend_config(dir: &Path, text: &str) {
    let path = dir.join("rssite.toml");
    let old = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{text}\n{old}")).unwrap();
}

/// A table goes last; a new header closes whatever table came before.
pub fn append_config(dir: &Path, text: &str) {
    let path = dir.join("rssite.toml");
    let old = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{old}\n{text}\n")).unwrap();
}

/// Replace `from` with `to` in rssite.toml; `from` must be there.
pub fn edit_config(dir: &Path, from: &str, to: &str) {
    let path = dir.join("rssite.toml");
    let old = std::fs::read_to_string(&path).unwrap();
    assert!(old.contains(from), "{from:?} is not in rssite.toml");
    std::fs::write(&path, old.replace(from, to)).unwrap();
}

pub fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

pub fn sources_of(manifest: &serde_json::Value, path: &str) -> Vec<String> {
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

/// A scratch copy of the bilingual fixture.
pub fn site(name: &str) -> PathBuf {
    let dir = temp_dir(name);
    copy_dir(&fixture(), &dir);
    dir
}

/// Build `dir` and fail the test with the generator's message if it fails.
pub fn build_ok(dir: &Path) -> Run {
    let build = run(dir, &["build"]);
    assert!(build.ok, "build failed:\n{}", build.stderr);
    build
}

/// Build `dir` expecting failure, and return the generator's message.
pub fn build_err(dir: &Path) -> String {
    let build = run(dir, &["build"]);
    assert!(!build.ok, "the build was expected to fail but passed");
    build.stderr
}

/// The parsed manifest of `rssite plan` for `dir`.
pub fn plan_of(dir: &Path) -> serde_json::Value {
    let plan = run(dir, &["plan"]);
    assert!(plan.ok, "plan failed:\n{}", plan.stderr);
    serde_json::from_str(&plan.stdout).expect("the plan is JSON")
}

/// The output paths of a manifest, in manifest order.
pub fn planned_paths(manifest: &serde_json::Value) -> Vec<String> {
    manifest["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["path"].as_str().unwrap().to_owned())
        .collect()
}

/// Replace `templates/page.html` with a probe that renders `snippet` inside
/// `<probe>` markers on the English blog posts (the other pages render
/// nothing, so a probe can read what only a post has), build, and return
/// what the markers enclose on the first post. One build per probe keeps
/// every template function and filter testable in one line each.
pub fn probe(name: &str, snippet: &str) -> String {
    let dir = site(name);
    write_probe(&dir, snippet);
    build_ok(&dir);
    extract_probe(&read(&dir, "_site/en/blog/first/index.html"))
}

/// Like `probe`, but the build is expected to fail; returns its message.
pub fn probe_error(name: &str, snippet: &str) -> String {
    let dir = site(name);
    write_probe(&dir, snippet);
    build_err(&dir)
}

/// Replace the fixture's page and section templates with ones that do not
/// format dates, for tests whose pages have none.
pub fn undated_templates(dir: &Path) {
    write(
        dir,
        "templates/page.html",
        "{% extends \"base.html\" %}{% block content %}<h1>{{ page.title }}</h1>{{ page.content | safe }}{% endblock %}",
    );
    write(
        dir,
        "templates/section.html",
        "{% extends \"base.html\" %}{% block content %}<h1>{{ section.title }}</h1>{% for p in section.pages %}<a href=\"{{ p.permalink }}\">{{ p.title }}</a>{% endfor %}{% endblock %}",
    );
}

pub fn write_probe(dir: &Path, snippet: &str) {
    write(
        dir,
        "templates/page.html",
        &format!(
            "{{% extends \"base.html\" %}}{{% block content %}}{{% if lang == \"en\" and page.relative_path is starting_with(\"blog/\") %}}<probe>{snippet}</probe>{{% endif %}}{{% endblock %}}"
        ),
    );
}

pub fn extract_probe(html: &str) -> String {
    let start = html.find("<probe>").expect("probe start") + "<probe>".len();
    let end = html.find("</probe>").expect("probe end");
    html[start..end].to_owned()
}
