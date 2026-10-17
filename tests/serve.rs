//! `rssite serve`, end to end: it builds, serves exactly the output
//! directory, redirects directory URLs to their slash form, answers 404
//! with the site's page, refuses what is not GET or HEAD, and rebuilds when
//! a source changes.

mod common;

use common::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The server process, killed when the test ends however it ends.
struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn serve(dir: &Path, watch: bool) -> Server {
    let port = free_port();
    let mut args = vec!["serve", "--port"];
    let port_text = port.to_string();
    args.push(&port_text);
    if !watch {
        args.push("--no-watch");
    }
    let child = Command::new(binary())
        .args(&args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            Instant::now() < deadline,
            "the server did not come up on port {port}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    server
}

struct Response {
    status: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn request(server: &Server, method: &str, path: &str) -> Response {
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a header block");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.lines();
    let status = lines
        .next()
        .unwrap()
        .trim_start_matches("HTTP/1.1 ")
        .to_owned();
    let headers = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    Response {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    }
}

#[test]
fn serve_builds_and_serves_the_output_directory() {
    let dir = site("serve-basic");
    let server = serve(&dir, false);
    assert!(
        dir.join("_site/en/blog/first/index.html").is_file(),
        "serve builds first"
    );

    let root = request(&server, "GET", "/");
    assert_eq!(root.status, "200 OK");
    assert_eq!(
        root.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(root.header("Cache-Control"), Some("no-store"));
    assert_eq!(root.text(), read(&dir, "_site/index.html"));
    assert_eq!(
        root.header("Content-Length"),
        Some(root.body.len().to_string().as_str())
    );

    let page = request(&server, "GET", "/en/blog/first/");
    assert_eq!(page.status, "200 OK");
    assert!(page.text().contains("<h1>First post</h1>"));

    let css = request(&server, "GET", "/style.css?cache=bust");
    assert_eq!(css.status, "200 OK");
    assert_eq!(css.header("Content-Type"), Some("text/css; charset=utf-8"));

    let hebrew = request(&server, "GET", "/he/tags/%D7%93%D7%AA/");
    assert_eq!(hebrew.status, "200 OK");
    assert!(hebrew.text().contains("מתויג דת"), "{}", hebrew.text());

    let xml = request(&server, "GET", "/sitemap.xml");
    assert_eq!(xml.header("Content-Type"), Some("application/xml"));
    let csv = request(&server, "GET", "/en/blog/data.csv");
    assert_eq!(csv.status, "200 OK");
    assert_eq!(csv.header("Content-Type"), Some("application/octet-stream"));
}

#[test]
fn serve_redirects_directories_to_their_slash_form() {
    let dir = site("serve-redirect");
    let server = serve(&dir, false);
    let redirect = request(&server, "GET", "/en/blog");
    assert_eq!(redirect.status, "301 Moved Permanently");
    assert_eq!(redirect.header("Location"), Some("/en/blog/"));
    assert!(redirect.body.is_empty());
    let with_query = request(&server, "GET", "/en/blog?x=1");
    assert_eq!(with_query.status, "301 Moved Permanently");
    assert_eq!(with_query.header("Location"), Some("/en/blog/"));
    let file = request(&server, "GET", "/en/blog/index.html");
    assert_eq!(file.status, "200 OK", "a file is served as it is");
}

#[test]
fn serve_answers_404_with_the_site_page_and_refuses_other_methods() {
    let dir = site("serve-404");
    let server = serve(&dir, false);
    let missing = request(&server, "GET", "/nothing/here/");
    assert_eq!(missing.status, "404 Not Found");
    assert_eq!(
        missing.header("Content-Type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(missing.text(), read(&dir, "_site/404.html"));
    let traversal = request(&server, "GET", "/../rssite.toml");
    assert_eq!(traversal.status, "404 Not Found");
    assert!(!traversal.text().contains("base_url"));
    let encoded = request(&server, "GET", "/%2e%2e/rssite.toml");
    assert_eq!(encoded.status, "404 Not Found");
    let post = request(&server, "POST", "/");
    assert_eq!(post.status, "405 Method Not Allowed");
    let head = request(&server, "HEAD", "/en/");
    assert_eq!(head.status, "200 OK");
    assert!(head.body.is_empty(), "HEAD has no body");
    assert_eq!(
        head.header("Content-Length"),
        Some(read(&dir, "_site/en/index.html").len().to_string().as_str())
    );
    let head_missing = request(&server, "HEAD", "/nope/");
    assert_eq!(head_missing.status, "404 Not Found");
    assert!(head_missing.body.is_empty());
}

#[test]
fn serve_rebuilds_when_a_source_changes() {
    let dir = site("serve-watch");
    let server = serve(&dir, true);
    assert!(
        request(&server, "GET", "/en/blog/third/")
            .text()
            .contains("<h1>Third post</h1>")
    );
    // Past the watcher's first fingerprint, so the change is a change.
    std::thread::sleep(Duration::from_millis(700));
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Third, edited\"\ndate = 2026-03-15\n+++\n\nEdited.\n",
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let text = request(&server, "GET", "/en/blog/third/").text();
        if text.contains("<h1>Third, edited</h1>") {
            break;
        }
        assert!(Instant::now() < deadline, "no rebuild within 20s:\n{text}");
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(
        request(&server, "GET", "/en/blog/")
            .text()
            .contains("Third, edited"),
        "listings rebuild too"
    );
    // A broken edit keeps serving the last good build.
    write(
        &dir,
        "content/blog/third.en.md",
        "+++\ntitle = \"Broken\"\ndate = 2026-03-15\n+++\n\n[x](@/blog/missing.md)\n",
    );
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        request(&server, "GET", "/en/blog/third/")
            .text()
            .contains("<h1>Third, edited</h1>")
    );
}

#[test]
fn serve_without_a_site_fails_before_listening() {
    let dir = temp_dir("serve-nosite");
    let port = free_port();
    let output = Command::new(binary())
        .args(["serve", "--no-watch", "--port", &port.to_string()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("rssite.toml"));
}

#[test]
fn serve_with_a_broken_site_still_listens_and_reports() {
    let dir = site("serve-broken");
    write(
        &dir,
        "content/blog/bad.en.md",
        "+++\ntitle = \"x\"\ndate = 2026-01-01\n+++\n\n[x](@/nope.md)\n",
    );
    let port = free_port();
    let child = Command::new(binary())
        .args(["serve", "--no-watch", "--port", &port.to_string()])
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < deadline, "the server did not come up");
        std::thread::sleep(Duration::from_millis(50));
    }
    let server = Server { child, port };
    let missing = request(&server, "GET", "/en/");
    assert_eq!(missing.status, "404 Not Found", "nothing was built");
    assert_eq!(
        missing.header("Content-Type"),
        Some("text/plain; charset=utf-8")
    );
}
