//! URL slugs. The default keeps every alphanumeric character of every
//! script, so a Hebrew tag stays readable in its URL; transliteration to
//! ASCII is opt-in per kind (paths, taxonomies, anchors).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SlugMode {
    /// Lowercase, keep Unicode alphanumerics, everything else becomes `-`.
    #[default]
    Safe,
    /// Transliterate to ASCII first, then as `safe`.
    Ascii,
}

/// Slugify `input`. The result may be empty when the input has no
/// alphanumeric character at all; callers treat that as an error.
pub fn slugify(input: &str, mode: SlugMode) -> String {
    let text: String = match mode {
        SlugMode::Safe => input.to_owned(),
        SlugMode::Ascii => deunicode::deunicode(input),
    };
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for c in text.chars() {
        let keep = match mode {
            SlugMode::Safe => c.is_alphanumeric(),
            SlugMode::Ascii => c.is_ascii_alphanumeric(),
        };
        if keep {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            for lower in c.to_lowercase() {
                out.push(lower);
            }
        } else {
            pending_dash = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_keeps_hebrew() {
        assert_eq!(slugify("ארכיאולוגיה", SlugMode::Safe), "ארכיאולוגיה");
    }

    #[test]
    fn safe_collapses_punctuation() {
        assert_eq!(
            slugify("Hello,  World! -- again", SlugMode::Safe),
            "hello-world-again"
        );
        assert_eq!(slugify("  --x--  ", SlugMode::Safe), "x");
    }

    #[test]
    fn ascii_transliterates() {
        assert_eq!(slugify("Café au lait", SlugMode::Ascii), "cafe-au-lait");
    }

    #[test]
    fn empty_when_nothing_survives() {
        assert_eq!(slugify("!!!", SlugMode::Safe), "");
    }
}
