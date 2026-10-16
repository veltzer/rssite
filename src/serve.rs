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
    [
        &config.content_dir,
        &config.templates_dir,
        &config.static_dir,
        &config.sass_dir,
        &config.i18n_dir,
    ]
    .iter()
    .map(|d| root.join(d))
    .collect()
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

fn resolve(dir: &Path, request_path: &str) -> Option<PathBuf> {
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
    if !bare.ends_with('/')
        && resolve(dir, bare).is_none()
        && resolve(dir, &format!("{bare}/")).is_some()
    {
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
}
