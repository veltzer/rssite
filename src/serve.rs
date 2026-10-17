//! `rssite serve`: the real build, served from the real output directory by
//! a plain static file server, rebuilt when a source changes. Nothing is
//! injected into the pages: what is served is what would be deployed.

use crate::config::Config;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub fn serve(root: &Path, drafts: bool, port: u16, watch: bool) -> Result<()> {
    let root = root
        .canonicalize()
        .with_context(|| format!("site root {}", root.display()))?;
    let config = Config::load(&root)?;
    let output_dir = root.join(&config.output_dir);
    build(&root, drafts);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    println!(
        "serving {} at http://127.0.0.1:{port}/",
        output_dir.display()
    );
    if watch {
        let root = root.clone();
        let watched = watched_dirs(&root, &config);
        std::thread::spawn(move || {
            let mut last = fingerprint(&watched, &root.join(crate::config::CONFIG_FILE));
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let now = fingerprint(&watched, &root.join(crate::config::CONFIG_FILE));
                if now != last {
                    last = now;
                    println!("change detected, rebuilding");
                    build(&root, drafts);
                }
            }
        });
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let dir = output_dir.clone();
        std::thread::spawn(move || {
            let _ = handle(stream, &dir);
        });
    }
    Ok(())
}

fn build(root: &Path, drafts: bool) {
    let options = crate::render::Options {
        print_plan: false,
        verify_determinism: false,
        write: true,
    };
    match crate::render::run(root, drafts, &options) {
        Ok(report) => println!(
            "built {} files ({} written, {} unchanged)",
            report.outputs, report.written, report.unchanged
        ),
        Err(error) => eprintln!("build failed: {error:#}"),
    }
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

fn fingerprint(dirs: &[PathBuf], config_file: &Path) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    let mut entries = Vec::new();
    let mut record = |path: PathBuf| {
        let meta = std::fs::metadata(&path).ok();
        entries.push((
            path,
            meta.as_ref().and_then(|m| m.modified().ok()),
            meta.map(|m| m.len()).unwrap_or(0),
        ));
    };
    record(config_file.to_path_buf());
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

fn handle(mut stream: TcpStream, dir: &Path) -> std::io::Result<()> {
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
            respond(
                &mut stream,
                "200 OK",
                content_type(&path),
                &body,
                method == "HEAD",
            )
        }
        None => {
            let not_found = dir.join("404.html");
            let body = std::fs::read(&not_found).unwrap_or_else(|_| b"404 not found".to_vec());
            let kind = if not_found.is_file() {
                "text/html; charset=utf-8"
            } else {
                "text/plain; charset=utf-8"
            };
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
        let first = fingerprint(&watched, &config);
        assert_eq!(first.len(), 1, "the config file alone");
        std::fs::write(dir.join("content/x.md"), "x").unwrap();
        let second = fingerprint(&watched, &config);
        assert_eq!(second.len(), 2);
        assert_ne!(first, second);
        std::fs::write(dir.join("content/x.md"), "xy").unwrap();
        let third = fingerprint(&watched, &config);
        assert_ne!(
            second, third,
            "a size change is seen even within the mtime granularity"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
