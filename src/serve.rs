//! `rssite serve`: the real build, served from the real output directory by
//! a plain static file server, rebuilt when a source changes. Nothing is
//! injected into the files: what is on disk is what would be deployed. With
//! live reload on, an HTML response carries one more `<script>`, which polls
//! the build generation and reloads the page when a rebuild succeeded.
//!
//! The build writes `base_url` into every absolute link, so a page served
//! here would send the reader to the live site on the first click. A text
//! response therefore has `base_url` replaced by the address the request came
//! to, again in the response only; `--keep-base-url` turns that off.

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

/// What a request is answered from: both follow the config when it changes.
#[derive(Clone)]
struct Served {
    dir: PathBuf,
    base_url: String,
}

impl Served {
    fn of(root: &Path, config: &Config) -> Served {
        Served {
            dir: root.join(&config.output_dir),
            base_url: config.base_url.clone(),
        }
    }
}

/// Whether the server watches the site, and what it does about a rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    /// Serve what is on disk; never rebuild.
    Off,
    /// Rebuild when a source changes.
    Rebuild,
    /// Rebuild, and have open pages reload once it succeeds.
    Reload,
}

pub fn serve(root: &Path, drafts: bool, port: u16, watch: Watch, rewrite: bool) -> Result<()> {
    let root = root
        .canonicalize()
        .with_context(|| format!("site root {}", root.display()))?;
    let config = Config::load(&root)?;
    let initial_output_dir = root.join(&config.output_dir);
    let served = Arc::new(RwLock::new(Served::of(&root, &config)));
    // Bumped by every successful rebuild; open pages reload when it moves.
    let generation = Arc::new(AtomicU64::new(0));
    build(&root, drafts);
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("binding 127.0.0.1:{port}"))?;
    println!(
        "serving {} at http://127.0.0.1:{port}/",
        initial_output_dir.display()
    );
    if watch != Watch::Off {
        let root = root.clone();
        let served = Arc::clone(&served);
        let generation = Arc::clone(&generation);
        std::thread::spawn(move || {
            let mut current_config = config;
            let mut last = site_fingerprint(&root, &current_config);
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if let Ok(reloaded) = Config::load(&root) {
                    if let Ok(mut out) = served.write() {
                        *out = Served::of(&root, &reloaded);
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
                        if let Ok(mut out) = served.write() {
                            *out = Served::of(&root, &reloaded);
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
        let served = served.read().expect("served lock").clone();
        let generation = (watch == Watch::Reload).then(|| generation.load(Ordering::SeqCst));
        std::thread::spawn(move || {
            let base_url = rewrite.then_some(served.base_url.as_str());
            let _ = handle(stream, &served.dir, generation, base_url, port);
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
            println!("built {} files ({})", report.outputs, report.counts());
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

/// Whether a response of this content type is text that can carry links.
fn is_text(kind: &str) -> bool {
    kind.starts_with("text/")
        || kind == "application/json"
        || kind == "application/xml"
        || kind == "image/svg+xml"
}

/// The origin a request came to, from its `Host` header: `http://` plus the
/// header when it is a plain host and optional port, else the address the
/// server listens on. Whatever the reader typed (`localhost`, a forwarded
/// port) is then where the links keep them.
fn local_origin(host: Option<&str>, port: u16) -> String {
    let plain = |h: &&str| {
        !h.is_empty()
            && h.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-:[]".contains(&b))
    };
    match host.map(str::trim).filter(plain) {
        Some(host) => format!("http://{host}"),
        None => format!("http://127.0.0.1:{port}"),
    }
}

/// `body` with every `base_url` replaced by `origin`. A match must end where
/// the URL's host or path segment ends: `https://x.org` is not replaced in
/// `https://x.org.uk`, `https://x.org:8443` or, for a base with a path, in
/// `https://x.org/site2`.
fn rewrite_base_url(body: &[u8], base_url: &str, origin: &str) -> Vec<u8> {
    let base = base_url.as_bytes();
    if base.is_empty() {
        return body.to_vec();
    }
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i..].starts_with(base) {
            let after = body.get(i + base.len()).copied();
            let continues = match after {
                Some(b) if b.is_ascii_alphanumeric() || b == b'.' || b == b'-' => true,
                Some(b':') => body.get(i + base.len() + 1).is_some_and(u8::is_ascii_digit),
                _ => false,
            };
            if !continues {
                out.extend_from_slice(origin.as_bytes());
                i += base.len();
                continue;
            }
        }
        out.push(body[i]);
        i += 1;
    }
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
            meta.map_or(0, |m| m.len()),
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
        // Byte by byte: slicing the text could cut a character in two.
        let hex = |at: usize| bytes.get(at).and_then(|b| char::from(*b).to_digit(16));
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (hex(i + 1), hex(i + 2))
        {
            out.push(u8::try_from(high * 16 + low).unwrap_or(u8::MAX));
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
/// reload is on: HTML responses then carry the reload script. `base_url` is
/// the site's when links are to stay on this server: text responses then have
/// it replaced by the address the request came to.
fn handle(
    mut stream: TcpStream,
    dir: &Path,
    generation: Option<u64>,
    base_url: Option<&str>,
    port: u16,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut host = None;
    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 && line != "\r\n" && line != "\n" {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("host")
        {
            host = Some(value.trim().to_owned());
        }
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
    let with_reload = |kind: &str, body: Vec<u8>| {
        let body = match base_url {
            Some(base) if is_text(kind) => {
                rewrite_base_url(&body, base, &local_origin(host.as_deref(), port))
            }
            _ => body,
        };
        match generation {
            Some(current) if kind.starts_with("text/html") => {
                inject(&body, &reload_script(current))
            }
            _ => body,
        }
    };
    // A directory URL without a trailing slash would make relative links
    // resolve against the parent; redirect like a real static host does.
    let bare = request_path.split('?').next().unwrap_or("/");
    if !bare.ends_with('/') && target_of(dir, bare).is_some_and(|t| t.is_dir()) {
        // The query goes along: `/en?x=1` is `/en/?x=1`.
        let location = match request_path.split_once('?') {
            Some((_, query)) => format!("{bare}/?{query}"),
            None => format!("{bare}/"),
        };
        let response = format!(
            "HTTP/1.1 301 Moved Permanently\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        return stream.write_all(response.as_bytes());
    }
    if let Some(path) = resolve(dir, request_path) {
        let mut body = Vec::new();
        std::fs::File::open(&path)?.read_to_end(&mut body)?;
        let kind = content_type(&path);
        let body = with_reload(kind, body);
        respond(&mut stream, "200 OK", kind, &body, method == "HEAD")
    } else {
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
    fn the_base_url_is_replaced_only_where_it_ends() {
        let to = "http://127.0.0.1:8000";
        let rewrite = |body: &str, base: &str| {
            String::from_utf8(rewrite_base_url(body.as_bytes(), base, to)).unwrap()
        };
        assert_eq!(
            rewrite(
                "<a href=\"https://x.org/en/\">https://x.org</a> https://x.org",
                "https://x.org"
            ),
            "<a href=\"http://127.0.0.1:8000/en/\">http://127.0.0.1:8000</a> http://127.0.0.1:8000"
        );
        for other in [
            "https://x.org.uk/",
            "https://x.org-b/",
            "https://x.organic/",
            "https://x.org:8443/",
            "http://x.org/",
        ] {
            assert_eq!(rewrite(other, "https://x.org"), other);
        }
        assert_eq!(
            rewrite("https://x.org: see", "https://x.org"),
            "http://127.0.0.1:8000: see",
            "a colon that starts no port ends the URL"
        );
        assert_eq!(
            rewrite(
                "https://x.org/site/a https://x.org/site2/ https://x.org/",
                "https://x.org/site"
            ),
            "http://127.0.0.1:8000/a https://x.org/site2/ https://x.org/",
            "a base with a path maps to the root, where the files are"
        );
        assert_eq!(
            rewrite("דת https://x.org/דת", "https://x.org"),
            "דת http://127.0.0.1:8000/דת"
        );
    }

    #[test]
    fn the_local_origin_follows_the_host_header() {
        assert_eq!(
            local_origin(Some("localhost:8000"), 8000),
            "http://localhost:8000"
        );
        assert_eq!(local_origin(Some(" [::1]:9 "), 8000), "http://[::1]:9");
        assert_eq!(local_origin(None, 8000), "http://127.0.0.1:8000");
        assert_eq!(local_origin(Some(""), 8000), "http://127.0.0.1:8000");
        assert_eq!(
            local_origin(Some("x\"><script>"), 8000),
            "http://127.0.0.1:8000",
            "a header that is not a host is not written into pages"
        );
    }

    #[test]
    fn text_responses_are_the_ones_rewritten() {
        for kind in ["html", "css", "js", "json", "xml", "txt", "svg"] {
            assert!(
                is_text(content_type(Path::new(&format!("a.{kind}")))),
                "{kind}"
            );
        }
        for kind in ["png", "woff2", "pdf", "bin"] {
            assert!(
                !is_text(content_type(Path::new(&format!("a.{kind}")))),
                "{kind}"
            );
        }
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
        assert_eq!(
            percent_decode("/%a\u{e9}"),
            "/%a\u{e9}",
            "a sequence running into a multi-byte character is kept, not a panic"
        );
        assert_eq!(percent_decode("%+1"), "%+1", "a sign is not a hex digit");
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
