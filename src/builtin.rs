//! Templates the generator ships for the outputs a site rarely wants to
//! style by hand. A file of the same name in the templates directory
//! replaces each one. They are short on purpose: a copy is not a liability.

pub const ATOM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xml:lang="{{ lang }}">
  <title>{{ feed_title }}</title>
  <link rel="self" type="application/atom+xml" href="{{ feed_url | safe }}"/>
  <link rel="alternate" type="text/html" href="{{ feed_alternate | safe }}"/>
  <generator uri="https://github.com/veltzer/rssite">rssite</generator>
  <id>{{ feed_url | safe }}</id>
{%- if last_updated %}
  <updated>{{ last_updated }}</updated>
{%- endif %}
{%- for page in pages %}
  <entry xml:lang="{{ page.lang }}">
    <title>{{ page.title }}</title>
    <published>{{ page.datetime }}</published>
    <updated>{{ page.updated_datetime }}</updated>
    <author><name>{{ config.author }}</name></author>
    <link rel="alternate" type="text/html" href="{{ page.permalink | safe }}"/>
    <id>{{ page.permalink | safe }}</id>
{%- if page.summary %}
    <summary type="html">{{ page.summary }}</summary>
{%- else %}
    <content type="html" xml:base="{{ page.permalink | safe }}">{{ page.content }}</content>
{%- endif %}
  </entry>
{%- endfor %}
</feed>
"#;

pub const SITEMAP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
{%- for entry in entries %}
  <url>
    <loc>{{ entry.permalink }}</loc>
{%- if entry.updated %}
    <lastmod>{{ entry.updated }}</lastmod>
{%- endif %}
{%- for alternate in entry.alternates %}
    <xhtml:link rel="alternate" hreflang="{{ alternate.lang }}" href="{{ alternate.permalink }}"/>
{%- endfor %}
  </url>
{%- endfor %}
</urlset>
"#;

pub const NOT_FOUND: &str = r#"<!DOCTYPE html>
<html lang="{{ lang }}" dir="{{ language.direction }}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>404 &middot; {{ config.title }}</title>
</head>
<body>
<h1>404</h1>
<p><a href="{{ language.url_prefix }}">{{ config.title }}</a></p>
</body>
</html>
"#;

/// A meta-refresh page: the only redirect a static host can serve.
pub fn redirect(target: &str) -> String {
    let target = crate::util::escape_html(target);
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<meta http-equiv=\"refresh\" content=\"0; url={target}\">\n<link rel=\"canonical\" href=\"{target}\">\n<meta name=\"robots\" content=\"noindex, follow\">\n<title>Redirecting</title>\n</head>\n<body>\n<p>This page has moved to <a href=\"{target}\">{target}</a>.</p>\n</body>\n</html>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redirect_names_its_target_in_every_place_a_client_looks() {
        let page = redirect("https://example.org/en/blog/x/");
        assert!(page.contains("content=\"0; url=https://example.org/en/blog/x/\""));
        assert!(page.contains("rel=\"canonical\" href=\"https://example.org/en/blog/x/\""));
        assert!(page.contains("<a href=\"https://example.org/en/blog/x/\">"));
        assert!(page.contains("noindex, follow"));
        assert!(page.starts_with("<!DOCTYPE html>\n"));
        assert!(page.ends_with("</html>\n"));
    }

    #[test]
    fn a_redirect_target_is_escaped() {
        let page = redirect("https://example.org/?a=1&b=\"<x>\"");
        assert!(page.contains("?a=1&amp;b=&quot;&lt;x&gt;&quot;"));
        assert!(!page.contains("<x>"));
    }

    #[test]
    fn the_built_in_templates_parse_as_tera() {
        let mut tera = tera::Tera::default();
        tera.add_raw_templates(vec![
            ("atom.xml", ATOM),
            ("sitemap.xml", SITEMAP),
            ("404.html", NOT_FOUND),
        ])
        .unwrap();
        let names: Vec<&str> = tera.get_template_names().collect();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn the_atom_template_renders_a_summary_or_the_content() {
        let mut tera = tera::Tera::default();
        tera.add_raw_template("atom.xml", ATOM).unwrap();
        tera.set_escape_fn(crate::util::escape_html);
        let mut context = tera::Context::new();
        context.insert("lang", "en");
        context.insert("feed_title", "T");
        context.insert("feed_url", "https://x/en/atom.xml");
        context.insert("feed_alternate", "https://x/en/");
        context.insert("last_updated", &Some("2026-01-01T00:00:00+00:00"));
        context.insert("config", &serde_json::json!({"author": "A"}));
        context.insert(
            "pages",
            &serde_json::json!([
                {"lang": "en", "title": "With summary", "datetime": "d", "updated_datetime": "u", "permalink": "https://x/en/a/", "summary": "<p>s</p>", "content": "<p>c</p>"},
                {"lang": "en", "title": "Without", "datetime": "d", "updated_datetime": "u", "permalink": "https://x/en/b/", "summary": null, "content": "<p>c</p>"}
            ]),
        );
        let out = tera.render("atom.xml", &context).unwrap();
        assert_eq!(out.matches("<entry").count(), 2);
        assert_eq!(out.matches("<summary").count(), 1);
        assert_eq!(out.matches("<content").count(), 1);
        assert!(out.contains("<updated>2026-01-01T00:00:00+00:00</updated>"));
        assert!(
            out.contains("&lt;p&gt;s&lt;/p&gt;"),
            "html inside the feed is escaped:\n{out}"
        );
    }
}
