//! `rssite serve`: the real build, served from the real output directory by
//! a plain static file server, rebuilt when a source changes. Nothing is
//! injected into the files: what is on disk is what would be deployed. With
//! live reload on, an HTML response carries one more `<script>`, which polls
//! the build generation and reloads the page when a rebuild succeeded.

use crate::config::Config;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Where the reload script asks for the build generation. Not a path any
/// site writes.
const GENERATION_PATH: &str = "/__rssite/generation";

pub fn serve(root: &Path, drafts: bool, port: u16, watch: bool, reload: bool) -> Result<()> {
    let root = root
        .canonicalize()
        .with_context(|| format!("site root {}", root.display()))?;
    let config = Config::load(&root)?;
    let initial_output_dir = root.join(&config.output_dir);
    let output_dir = Arc::new(RwLock::new(initial_output_dir.clone()));
    // Bumped by every successful rebuild; open pages reload when it moves.
    let generation = Arc::new(AtomicU64::new(0));
    build(&root, drafts);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    println!(
        "serving {} at http://127.0.0.1:{port}/",
        initial_output_dir.display()
    );
    if watch {
        let root = root.clone();
        let output_dir = Arc::clone(&output_dir);
        let generation = Arc::clone(&generation);
        std::thread::spawn(move || {
            let mut current_config = config;
            let mut last = site_fingerprint(&root, &current_config);
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if let Ok(reloaded) = Config::load(&root) {
                    if let Ok(mut out) = output_dir.write() {
                        *out = root.join(&reloaded.output_dir);
                    }
                    current_config = reloaded;
                }
                let now = site_fingerprint(&root, &current_config);
                if now != last {
                    println!("change detected, rebuilding");
                    if build(&root, drafts) {
                        generation.fetch_add(1, Ordering::SeqCst);
                    }
                    if let Ok(reloaded) = Config::load(&root) {
                        if let Ok(mut out) = output_dir.write() {
                            *out = root.join(&reloaded.output_dir);
                        }
                        current_config = reloaded;
                    }
                    last = site_fingerprint(&root, &current_config);
                }
            }
        });
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let dir = output_dir.read().expect("output_dir lock").clone();
        let generation = reload.then(|| generation.load(Ordering::SeqCst));
        std::thread::spawn(move || {
            let _ = handle(stream, &dir, generation);
        });
    }
    Ok(())
}

/// Build once; false when the build failed (the error is printed).
fn build(root: &Path, drafts: bool) -> bool {
    let options = crate::render::Options {
        print_plan: false,
        verify_determinism: false,
        write: true,
    };
    match crate::render::run(root, drafts, &options) {
        Ok(report) => {
            println!(
                "built {} files ({} written, {} unchanged)",
                report.outputs, report.written, report.unchanged
            );
            true
        }
        Err(error) => {
            eprintln!("build failed: {error:#}");
            false
        }
    }
}

/// The live reload script for a page served at `generation`: it polls
/// [`GENERATION_PATH`] and reloads once the number differs. A stopped server
/// is a failed fetch, ignored; a restarted one counts from 0 again, which
/// differs, so the page reloads when it is back.
fn reload_script(generation: u64) -> String {
    format!(
        "<script>(function(){{var g=\"{generation}\";setInterval(function(){{\
fetch(\"{GENERATION_PATH}\",{{cache:\"no-store\"}}).then(function(r){{return r.text()}})\
.then(function(t){{if(t!==g)location.reload()}}).catch(function(){{}})}},1000)}})();</script>"
    )
}

/// `body` with `script` before its last `</body>`, or at its end when it has
/// none.
fn inject(body: &[u8], script: &str) -> Vec<u8> {
    let lower = body.to_ascii_lowercase();
    let at = lower
        .windows(7)
        .rposition(|w| w == b"</body>")
        .unwrap_or(body.len());
    let mut out = Vec::with_capacity(body.len() + script.len());
    out.extend_from_slice(&body[..at]);
    out.extend_from_slice(script.as_bytes());
    out.extend_from_slice(&body[at..]);
    out
}

fn watched_dirs(root: &Path, config: &Config) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        &config.content_dir,
        &config.templates_dir,
        &config.static_dir,
        &config.sass_dir,
        &config.i18n_dir,
    ]
    .iter()
    .map(|d| root.join(d))
    .collect();
    // The theme and the published directories change the output too.
    if let Some(theme) = &config.theme {
        dirs.push(root.join(&config.themes_dir).join(theme));
    }
    for entry in &config.publish {
        dirs.push(root.join(&entry.from));
    }
    dirs
}

fn watched_files(root: &Path, config: &Config) -> Vec<PathBuf> {
    let mut files = BTreeSet::new();
    files.insert(root.join(crate::config::CONFIG_FILE));
    for taxonomy in &config.taxonomies {
        if let Some(translations) = &taxonomy.translations {
            files.insert(root.join(translations));
        }
    }
    let mut template_roots = vec![
        root.join(&config.templates_dir),
        root.join(&config.content_dir),
    ];
    if let Some(theme) = &config.theme {
        template_roots.push(
            root.join(&config.themes_dir)
                .join(theme)
                .join(&config.templates_dir),
        );
    }
    for dir in template_roots {
        for path in crate::util::walk_files(&dir).unwrap_or_default() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                for data_path in crate::site::load_data_paths(&text) {
                    files.insert(root.join(data_path));
                }
            }
        }
    }
    files.into_iter().collect()
}

fn site_fingerprint(root: &Path, config: &Config) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    let dirs = watched_dirs(root, config);
    let files = watched_files(root, config);
    fingerprint(&dirs, &files)
}

fn fingerprint(dirs: &[PathBuf], files: &[PathBuf]) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    let mut entries = Vec::new();
    let mut record = |path: PathBuf| {
        let meta = std::fs::metadata(&path).ok();
        entries.push((
            path,
            meta.as_ref().and_then(|m| m.modified().ok()),
            meta.map(|m| m.len()).unwrap_or(0),
        ));
    };
    for file in files {
        record(file.clone());
    }
    for dir in dirs {
        for path in crate::util::walk_files(dir).unwrap_or_default() {
            record(path);
        }
    }
    entries
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "pdf" => "application/pdf",
        "gz" => "application/gzip",
        _ => "application/octet-stream",
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(value) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The path under `dir` a request names, or `None` when it tries to climb
/// out of it. Existence is not checked here.
fn target_of(dir: &Path, request_path: &str) -> Option<PathBuf> {
    let path = percent_decode(request_path.split('?').next().unwrap_or("/"));
    let mut target = dir.to_path_buf();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        target.push(part);
    }
    Some(target)
}

fn resolve(dir: &Path, request_path: &str) -> Option<PathBuf> {
    let mut target = target_of(dir, request_path)?;
    if target.is_dir() {
        target.push("index.html");
    }
    if target.is_file() { Some(target) } else { None }
}

/// Serve one request. `generation` is the current build generation when live
/// reload is on: HTML responses then carry the reload script.
fn handle(mut stream: TcpStream, dir: &Path, generation: Option<u64>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 && line != "\r\n" && line != "\n" {
        line.clear();
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let request_path = parts.next().unwrap_or("/");
    if method != "GET" && method != "HEAD" {
        return respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain; charset=utf-8",
            b"method not allowed",
            method == "HEAD",
        );
    }
    if let Some(current) = generation
        && request_path.split('?').next() == Some(GENERATION_PATH)
    {
        let body = current.to_string();
        return respond(
            &mut stream,
            "200 OK",
            "text/plain; charset=utf-8",
            body.as_bytes(),
            method == "HEAD",
        );
    }
    let with_reload = |kind: &str, body: Vec<u8>| match generation {
        Some(current) if kind.starts_with("text/html") => inject(&body, &reload_script(current)),
        _ => body,
    };
    // A directory URL without a trailing slash would make relative links
    // resolve against the parent; redirect like a real static host does.
    let bare = request_path.split('?').next().unwrap_or("/");
    if !bare.ends_with('/') && target_of(dir, bare).is_some_and(|t| t.is_dir()) {
        let location = format!("{bare}/");
        let response = format!(
            "HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        return stream.write_all(response.as_bytes());
    }
    match resolve(dir, request_path) {
        Some(path) => {
            let mut body = Vec::new();
            std::fs::File::open(&path)?.read_to_end(&mut body)?;
            let kind = content_type(&path);
            let body = with_reload(kind, body);
            respond(&mut stream, "200 OK", kind, &body, method == "HEAD")
        }
        None => {
            let not_found = dir.join("404.html");
            let body = std::fs::read(&not_found).unwrap_or_else(|_| b"404 not found".to_vec());
            let kind = if not_found.is_file() {
                "text/html; charset=utf-8"
            } else {
                "text/plain; charset=utf-8"
            };
            let body = with_reload(kind, body);
            respond(&mut stream, "404 Not Found", kind, &body, method == "HEAD")
        }
    }
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    head: bool,
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !head {
        stream.write_all(body)?;
    }
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reload_script_goes_before_the_last_body_end() {
        let script = reload_script(7);
        assert!(script.contains("var g=\"7\""), "{script}");
        assert!(script.contains(GENERATION_PATH), "{script}");
        assert_eq!(
            inject(b"<p></body>x</BODY></html>", "<s>"),
            b"<p></body>x<s></BODY></html>"
        );
        assert_eq!(inject(b"<p>no end", "<s>"), b"<p>no end<s>");
    }

    #[test]
    fn decodes_percent_sequences() {
        assert_eq!(percent_decode("/he/tags/%D7%93%D7%AA/"), "/he/tags/דת/");
        assert_eq!(percent_decode("/plain/"), "/plain/");
        assert_eq!(percent_decode("/bad%"), "/bad%");
    }

    #[test]
    fn percent_decoding_edge_cases() {
        assert_eq!(percent_decode("%2"), "%2", "a truncated sequence is kept");
        assert_eq!(percent_decode("%20"), " ");
        assert_eq!(percent_decode("%20/"), " /");
        assert_eq!(percent_decode("%zz/"), "%zz/", "invalid hex is kept");
        assert_eq!(percent_decode("a%2Fb/"), "a/b/");
        assert_eq!(
            percent_decode("%FF%FE/"),
            "\u{fffd}\u{fffd}/",
            "invalid utf-8 is replaced"
        );
        assert_eq!(percent_decode(""), "");
    }

    #[test]
    fn content_types_by_extension() {
        assert_eq!(
            content_type(Path::new("a/index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("style.css")),
            "text/css; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("x.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(content_type(Path::new("sitemap.xml")), "application/xml");
        assert_eq!(content_type(Path::new("f.woff2")), "font/woff2");
        assert_eq!(content_type(Path::new("i.svg")), "image/svg+xml");
        assert_eq!(
            content_type(Path::new("i.JPG")),
            "application/octet-stream",
            "extensions are case-sensitive"
        );
        assert_eq!(content_type(Path::new("noext")), "application/octet-stream");
        assert_eq!(content_type(Path::new("a.tar.gz")), "application/gzip");
    }

    fn served_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rssite-serve-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("en/blog")).unwrap();
        std::fs::write(dir.join("index.html"), "root").unwrap();
        std::fs::write(dir.join("en/index.html"), "en").unwrap();
        std::fs::write(dir.join("en/blog/index.html"), "blog").unwrap();
        std::fs::write(dir.join("style.css"), "css").unwrap();
        std::fs::create_dir_all(dir.join("he/דת")).unwrap();
        std::fs::write(dir.join("he/דת/index.html"), "tag").unwrap();
        dir
    }

    #[test]
    fn requests_resolve_to_files_under_the_output_directory() {
        let dir = served_dir("resolve");
        assert_eq!(resolve(&dir, "/"), Some(dir.join("index.html")));
        assert_eq!(resolve(&dir, "/en/"), Some(dir.join("en/index.html")));
        assert_eq!(
            resolve(&dir, "/en"),
            Some(dir.join("en/index.html")),
            "a directory without a slash still resolves; the redirect is decided by the handler"
        );
        assert!(target_of(&dir, "/en").unwrap().is_dir());
        assert!(!target_of(&dir, "/style.css").unwrap().is_dir());
        assert!(
            target_of(&dir, "/nope").is_some(),
            "existence is not its concern"
        );
        assert_eq!(target_of(&dir, "/../x"), None);
        assert_eq!(
            resolve(&dir, "/en/blog/index.html"),
            Some(dir.join("en/blog/index.html"))
        );
        assert_eq!(
            resolve(&dir, "/style.css?v=3"),
            Some(dir.join("style.css")),
            "query strings are ignored"
        );
        assert_eq!(
            resolve(&dir, "//en///blog/"),
            Some(dir.join("en/blog/index.html")),
            "empty segments collapse"
        );
        assert_eq!(resolve(&dir, "/./en/./"), Some(dir.join("en/index.html")));
        assert_eq!(
            resolve(&dir, "/he/%D7%93%D7%AA/"),
            Some(dir.join("he/דת/index.html")),
            "percent-encoded unicode"
        );
        assert_eq!(resolve(&dir, "/missing/"), None);
        assert_eq!(
            resolve(&dir, "/en/blog"),
            Some(dir.join("en/blog/index.html"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn requests_cannot_escape_the_output_directory() {
        let dir = served_dir("escape");
        assert_eq!(resolve(&dir, "/../"), None);
        assert_eq!(resolve(&dir, "/en/../../etc/passwd"), None);
        assert_eq!(resolve(&dir, "/%2e%2e/"), None, "decoded before the check");
        assert_eq!(resolve(&dir, "/en/..%2f..%2fx"), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn watched_directories_include_the_theme_and_published_sources() {
        let text = "base_url = \"https://x\"\ntheme = \"fleet\"\n[[languages]]\ncode = \"en\"\nname = \"E\"\n[[publish]]\nfrom = \"shared/tokens\"\nto = \"tokens\"\n";
        let dir = std::env::temp_dir().join(format!("rssite-watched-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::config::CONFIG_FILE), text).unwrap();
        let config = Config::load(&dir).unwrap();
        let dirs = watched_dirs(&dir, &config);
        let rels: Vec<String> = dirs.iter().map(|d| crate::util::rel(d, &dir)).collect();
        assert_eq!(
            rels,
            [
                "content",
                "templates",
                "static",
                "sass",
                "i18n",
                "themes/fleet",
                "shared/tokens"
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_fingerprint_changes_with_content_and_tolerates_missing_directories() {
        let dir = std::env::temp_dir().join(format!("rssite-fingerprint-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("content")).unwrap();
        let config = dir.join("rssite.toml");
        std::fs::write(&config, "a").unwrap();
        let watched = vec![dir.join("content"), dir.join("nope")];
        let files = vec![config.clone()];
        let first = fingerprint(&watched, &files);
        assert_eq!(first.len(), 1, "the config file alone");
        std::fs::write(dir.join("content/x.md"), "x").unwrap();
        let second = fingerprint(&watched, &files);
        assert_eq!(second.len(), 2);
        assert_ne!(first, second);
        std::fs::write(dir.join("content/x.md"), "xy").unwrap();
        let third = fingerprint(&watched, &files);
        assert_ne!(
            second, third,
            "a size change is seen even within the mtime granularity"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn watched_files_include_taxonomy_translations_and_load_data_paths() {
        let text = "base_url = \"https://x\"\n[[languages]]\ncode = \"en\"\nname = \"E\"\n[[taxonomies]]\nname = \"tags\"\ntranslations = \"data/tags.toml\"\n";
        let dir = std::env::temp_dir().join(format!("rssite-watched-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("templates")).unwrap();
        std::fs::write(dir.join(crate::config::CONFIG_FILE), text).unwrap();
        std::fs::write(
            dir.join("templates/page.html"),
            "{{ load_data(required=false, path=\"data/info.toml\") }}",
        )
        .unwrap();
        let config = Config::load(&dir).unwrap();
        let files = watched_files(&dir, &config);
        let rels: Vec<String> = files.iter().map(|f| crate::util::rel(f, &dir)).collect();
        assert_eq!(rels, ["data/info.toml", "data/tags.toml", "rssite.toml"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
