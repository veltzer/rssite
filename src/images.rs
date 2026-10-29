//! `[images]`: a markdown image of a local JPEG, PNG or WebP file becomes a
//! `<picture>` offering smaller widths and modern formats, with `width`,
//! `height` and lazy loading on its `<img>`.
//!
//! Every variant is a file of its own next to the original
//! (`photo.jpg` -> `photo-480w.jpg`, `photo-480w.webp`, ...), so the plan
//! knows each one, with the original image as its source, before anything
//! is encoded. The original is still published as it is: an old link to it
//! keeps working.

use crate::config::Images;
use anyhow::{Context, Result, bail};
use image::codecs::avif::AvifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The formats `[images] formats` may name.
pub const EXTRA_FORMATS: &[&str] = &["webp", "avif"];

/// rav1e's speed, 0 (slowest) to 10. 6 is ravif's own default: most of the
/// compression of the slow settings at a fraction of their time.
const AVIF_SPEED: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Jpeg,
    Png,
    Webp,
    Avif,
    Gif,
}

impl Format {
    fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Format::Jpeg),
            "png" => Some(Format::Png),
            "webp" => Some(Format::Webp),
            "avif" => Some(Format::Avif),
            "gif" => Some(Format::Gif),
            _ => None,
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Format::Jpeg => "image/jpeg",
            Format::Png => "image/png",
            Format::Webp => "image/webp",
            Format::Avif => "image/avif",
            Format::Gif => "image/gif",
        }
    }
}

/// Fails on a setting the build could only trip over later.
pub fn check(config: &Images) -> Result<()> {
    let file = crate::config::CONFIG_FILE;
    for (i, format) in config.formats.iter().enumerate() {
        if !EXTRA_FORMATS.contains(&format.as_str()) {
            bail!(
                "{file}: images.formats has {format:?}; known: {}",
                EXTRA_FORMATS.join(", ")
            );
        }
        if config.formats[..i].contains(format) {
            bail!("{file}: images.formats names {format:?} twice");
        }
    }
    if !(1..=100).contains(&config.quality) {
        bail!(
            "{file}: images.quality is {}, not between 1 and 100",
            config.quality
        );
    }
    if config.widths.contains(&0) {
        bail!("{file}: images.widths has a width of 0");
    }
    Ok(())
}

/// One file the build encodes: `source` at `width` x `height` in `format`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    pub source: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub quality: u8,
}

/// An image's size as it is displayed, after its EXIF orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub format: Format,
}

/// Reads only as much of the file as its size and orientation take.
pub fn info(path: &Path) -> Result<Info> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("reading {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("reading {}", path.display()))?;
    let format = match reader.format() {
        Some(image::ImageFormat::Jpeg) => Format::Jpeg,
        Some(image::ImageFormat::Png) => Format::Png,
        Some(image::ImageFormat::WebP) => Format::Webp,
        Some(image::ImageFormat::Gif) => Format::Gif,
        _ => bail!("{} is not a JPEG, PNG, WebP or GIF image", path.display()),
    };
    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("decoding {}", path.display()))?;
    let (width, height) = decoder.dimensions();
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let (width, height) = if turns_sideways(orientation) {
        (height, width)
    } else {
        (width, height)
    };
    Ok(Info {
        width,
        height,
        format,
    })
}

fn turns_sideways(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90
            | Orientation::Rotate270
            | Orientation::Rotate90FlipH
            | Orientation::Rotate270FlipH
    )
}

/// Whether a file is one `[images]` handles, by its extension.
pub fn handles(path: &str) -> bool {
    path.rsplit_once('.')
        .and_then(|(_, ext)| Format::from_extension(ext))
        .is_some_and(|f| f != Format::Avif)
}

/// What a markdown image is written as, and the files that takes.
#[derive(Debug)]
pub struct Picture {
    pub html: String,
    /// Output path (relative to the output directory) and what goes there.
    pub variants: Vec<(String, Variant)>,
}

/// The `<picture>` (or, with nothing to choose between, the `<img>`) for an
/// image written as `dest` whose original is published at `out_path`.
pub fn picture(
    config: &Images,
    dest: &str,
    out_path: &str,
    source: &Path,
    info: Info,
    alt: &str,
    title: &str,
) -> Picture {
    let (dest_dir, _) = split_last(dest);
    let (out_dir, file) = split_last(out_path);
    let (stem, ext) = file.rsplit_once('.').unwrap_or((file, ""));
    let height_at = |w: u32| -> u32 {
        let h = (u64::from(info.height) * u64::from(w) + u64::from(info.width) / 2)
            / u64::from(info.width.max(1));
        (h as u32).max(1)
    };

    let mut variants = Vec::new();
    let mut sources = Vec::new();
    // A GIF is often animated, and resizing would keep only its first frame.
    let resizable = info.format != Format::Gif;
    let mut widths: Vec<u32> = if resizable {
        config
            .widths
            .iter()
            .copied()
            .filter(|&w| w < info.width)
            .collect()
    } else {
        Vec::new()
    };
    widths.sort_unstable();
    widths.dedup();
    // Too wide for every configured width: the largest of them stands in
    // for the original, which is never sent.
    let full = !resizable
        || config.widths.is_empty()
        || info.width <= config.widths.iter().copied().max().unwrap_or(0);

    let mut variant = |w: u32, format: Format, ext: &str| -> String {
        let name = format!("{stem}-{w}w.{ext}");
        variants.push((
            format!("{out_dir}{name}"),
            Variant {
                source: source.to_path_buf(),
                width: w,
                height: height_at(w),
                format,
                quality: config.quality,
            },
        ));
        format!("{dest_dir}{name}")
    };

    if resizable {
        for extra in &config.formats {
            let (format, extra_ext) = match extra.as_str() {
                "avif" => (Format::Avif, "avif"),
                _ => (Format::Webp, "webp"),
            };
            if format == info.format {
                continue;
            }
            let mut set: Vec<(String, u32)> = widths
                .iter()
                .map(|&w| (variant(w, format, extra_ext), w))
                .collect();
            if full {
                set.push((variant(info.width, format, extra_ext), info.width));
            }
            sources.push((format, set));
        }
    }
    let mut own: Vec<(String, u32)> = widths
        .iter()
        .map(|&w| (variant(w, info.format, ext), w))
        .collect();
    if full {
        own.push((dest.to_owned(), info.width));
    }
    let (src, src_width) = own.last().cloned().unwrap_or((dest.to_owned(), info.width));

    let esc = crate::util::escape_html;
    let srcset = |set: &[(String, u32)]| -> String {
        set.iter()
            .map(|(url, w)| format!("{} {w}w", esc(url)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let sizes = esc(&config.sizes);
    let mut img = format!("<img src=\"{}\"", esc(&src));
    if own.len() > 1 {
        img.push_str(&format!(" srcset=\"{}\" sizes=\"{sizes}\"", srcset(&own)));
    }
    img.push_str(&format!(
        " width=\"{src_width}\" height=\"{}\" alt=\"{}\"",
        height_at(src_width),
        esc(alt)
    ));
    if !title.is_empty() {
        img.push_str(&format!(" title=\"{}\"", esc(title)));
    }
    if config.lazy {
        img.push_str(" loading=\"lazy\"");
    }
    img.push_str(" decoding=\"async\" />");
    let html = if sources.is_empty() {
        img
    } else {
        let mut html = String::from("<picture>");
        for (format, set) in &sources {
            html.push_str(&format!(
                "<source type=\"{}\" srcset=\"{}\" sizes=\"{sizes}\" />",
                format.mime(),
                srcset(set)
            ));
        }
        html.push_str(&img);
        html.push_str("</picture>");
        html
    };
    Picture { html, variants }
}

/// `a/b/c.jpg` -> (`a/b/`, `c.jpg`); a path without `/` has an empty
/// directory.
fn split_last(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(at) => (&path[..=at], &path[at + 1..]),
        None => ("", path),
    }
}

/// Where encoded variants are kept between runs: AVIF takes seconds an
/// image, and a build re-renders every output. `$XDG_CACHE_HOME/rssite/
/// images` (`~/.cache/rssite/images`); `None` without either variable.
fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("rssite").join("images"))
}

/// The cache file of a variant of an image whose bytes are `source`: the
/// key covers everything the encoded bytes depend on, rssite's version
/// standing in for the encoders'.
fn cache_name(variant: &Variant, source: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    source.hash(&mut hasher);
    (
        variant.width,
        variant.height,
        variant.format,
        variant.quality,
        AVIF_SPEED,
    )
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The bytes of one variant, from the cache when an earlier run made them.
pub fn encode(variant: &Variant) -> Result<Vec<u8>> {
    let source = std::fs::read(&variant.source)
        .with_context(|| format!("reading {}", variant.source.display()))?;
    let cached = cache_dir().map(|dir| dir.join(cache_name(variant, &source)));
    if let Some(bytes) = cached.as_ref().and_then(|f| std::fs::read(f).ok()) {
        return Ok(bytes);
    }
    let bytes = encode_uncached(variant, &source)
        .with_context(|| format!("processing {}", variant.source.display()))?;
    // Best effort: a cache that cannot be written only costs time. The
    // rename makes a half-written file impossible to read back.
    if let Some(file) = &cached
        && let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let partial = file.with_extension(format!("{}.part", std::process::id()));
        if std::fs::write(&partial, &bytes).is_ok() {
            let _ = std::fs::rename(&partial, file);
        }
    }
    Ok(bytes)
}

fn encode_uncached(variant: &Variant, source: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = ImageReader::new(std::io::Cursor::new(source))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    if image.width() != variant.width || image.height() != variant.height {
        image = image.resize_exact(variant.width, variant.height, FilterType::Lanczos3);
    }
    let alpha = image.color().has_alpha();
    let mut out = Vec::new();
    match variant.format {
        Format::Jpeg => {
            let encoder = JpegEncoder::new_with_quality(&mut out, variant.quality);
            DynamicImage::ImageRgb8(image.to_rgb8()).write_with_encoder(encoder)?;
        }
        Format::Png => image.write_with_encoder(PngEncoder::new(&mut out))?,
        Format::Webp => {
            let (w, h) = (image.width(), image.height());
            let memory = if alpha {
                webp::Encoder::from_rgba(image.to_rgba8().as_raw(), w, h)
                    .encode_simple(false, f32::from(variant.quality))
            } else {
                webp::Encoder::from_rgb(image.to_rgb8().as_raw(), w, h)
                    .encode_simple(false, f32::from(variant.quality))
            }
            .map_err(|e| anyhow::anyhow!("WebP encoding failed: {e:?}"))?;
            out.extend_from_slice(&memory);
        }
        Format::Avif => {
            let encoder =
                AvifEncoder::new_with_speed_quality(&mut out, AVIF_SPEED, variant.quality);
            let image = if alpha {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            };
            image.write_with_encoder(encoder)?;
        }
        Format::Gif => bail!("GIF variants are never planned"),
    }
    Ok(out)
}

/// One variant's bytes, once a thread has encoded it.
type Slot = Mutex<Option<Result<Vec<u8>>>>;

/// Encodes every variant, on every core: the plan renders one output after
/// another, and these are the slow ones.
pub fn encode_all(variants: &[&Variant]) -> Result<Vec<Vec<u8>>> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(variants.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Slot> = variants.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(variant) = variants.get(i) else {
                        break;
                    };
                    *results[i].lock().expect("result slot") = Some(encode(variant));
                }
            });
        }
    });
    results
        .into_iter()
        .map(|slot| {
            slot.into_inner()
                .expect("result slot")
                .expect("every variant was encoded")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Images {
        Images {
            enabled: true,
            ..Images::default()
        }
    }

    fn jpeg(width: u32, height: u32) -> Info {
        Info {
            width,
            height,
            format: Format::Jpeg,
        }
    }

    #[test]
    fn a_wide_image_gets_every_width_and_never_its_own() {
        let p = picture(
            &config(),
            "../img/a.jpg",
            "img/a.jpg",
            Path::new("/s/static/img/a.jpg"),
            jpeg(3000, 2000),
            "An A",
            "",
        );
        let paths: Vec<&str> = p.variants.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "img/a-480w.webp",
                "img/a-960w.webp",
                "img/a-1440w.webp",
                "img/a-480w.jpg",
                "img/a-960w.jpg",
                "img/a-1440w.jpg",
            ]
        );
        assert_eq!(p.variants[0].1.height, 320);
        assert!(
            p.html.starts_with(
                "<picture><source type=\"image/webp\" srcset=\"../img/a-480w.webp 480w, ../img/a-960w.webp 960w, ../img/a-1440w.webp 1440w\""
            ),
            "{}",
            p.html
        );
        assert!(p.html.contains("<img src=\"../img/a-1440w.jpg\" srcset=\"../img/a-480w.jpg 480w, ../img/a-960w.jpg 960w, ../img/a-1440w.jpg 1440w\" sizes=\"100vw\" width=\"1440\" height=\"960\" alt=\"An A\" loading=\"lazy\" decoding=\"async\" />"), "{}", p.html);
    }

    #[test]
    fn an_image_within_the_widths_keeps_its_original() {
        let p = picture(
            &config(),
            "/img/a.jpg",
            "img/a.jpg",
            Path::new("a.jpg"),
            jpeg(600, 800),
            "",
            "t",
        );
        let paths: Vec<&str> = p.variants.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            ["img/a-480w.webp", "img/a-600w.webp", "img/a-480w.jpg"]
        );
        assert!(
            p.html.contains(
                "<img src=\"/img/a.jpg\" srcset=\"/img/a-480w.jpg 480w, /img/a.jpg 600w\""
            ),
            "{}",
            p.html
        );
        assert!(p.html.contains("width=\"600\" height=\"800\""));
        assert!(p.html.contains("title=\"t\""));
    }

    #[test]
    fn a_small_image_without_extra_formats_is_a_plain_img() {
        let config = Images {
            formats: Vec::new(),
            lazy: false,
            ..config()
        };
        let p = picture(
            &config,
            "a.png",
            "x/a.png",
            Path::new("a.png"),
            Info {
                width: 100,
                height: 50,
                format: Format::Png,
            },
            "a \"quote\"",
            "",
        );
        assert!(p.variants.is_empty());
        assert_eq!(
            p.html,
            "<img src=\"a.png\" width=\"100\" height=\"50\" alt=\"a &quot;quote&quot;\" decoding=\"async\" />"
        );
    }

    #[test]
    fn a_gif_gets_its_size_but_no_variants() {
        let p = picture(
            &config(),
            "a.gif",
            "a.gif",
            Path::new("a.gif"),
            Info {
                width: 2000,
                height: 1000,
                format: Format::Gif,
            },
            "",
            "",
        );
        assert!(p.variants.is_empty());
        assert!(
            p.html
                .starts_with("<img src=\"a.gif\" width=\"2000\" height=\"1000\"")
        );
    }

    #[test]
    fn a_webp_original_is_not_offered_again_as_webp() {
        let p = picture(
            &config(),
            "a.webp",
            "a.webp",
            Path::new("a.webp"),
            Info {
                width: 500,
                height: 500,
                format: Format::Webp,
            },
            "",
            "",
        );
        let paths: Vec<&str> = p.variants.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["a-480w.webp"]);
        assert!(!p.html.contains("<picture>"));
    }

    #[test]
    fn avif_comes_first_when_listed_first() {
        let config = Images {
            formats: vec!["avif".into(), "webp".into()],
            ..config()
        };
        let p = picture(
            &config,
            "a.jpg",
            "a.jpg",
            Path::new("a.jpg"),
            jpeg(400, 300),
            "",
            "",
        );
        let avif = p.html.find("image/avif").unwrap();
        let webp = p.html.find("image/webp").unwrap();
        assert!(avif < webp);
    }

    #[test]
    fn checks_reject_unknown_formats_and_bad_quality() {
        let bad = Images {
            formats: vec!["jxl".into()],
            ..config()
        };
        assert!(check(&bad).unwrap_err().to_string().contains("\"jxl\""));
        let bad = Images {
            quality: 0,
            ..config()
        };
        assert!(check(&bad).is_err());
        let bad = Images {
            formats: vec!["webp".into(), "webp".into()],
            ..config()
        };
        assert!(check(&bad).unwrap_err().to_string().contains("twice"));
        assert!(check(&config()).is_ok());
    }

    #[test]
    fn handled_extensions() {
        assert!(handles("a/b.JPG"));
        assert!(handles("b.png"));
        assert!(handles("b.gif"));
        assert!(!handles("b.svg"));
        assert!(!handles("b.avif"));
        assert!(!handles("noext"));
    }
}
