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
