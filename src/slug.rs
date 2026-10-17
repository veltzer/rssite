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
        assert_eq!(slugify("", SlugMode::Safe), "");
        assert_eq!(slugify("   ", SlugMode::Ascii), "");
    }

    #[test]
    fn safe_lowercases_every_script() {
        assert_eq!(slugify("ÄÖÜ Straße", SlugMode::Safe), "äöü-straße");
        assert_eq!(slugify("ΑΘΗΝΑ", SlugMode::Safe), "αθηνα");
        assert_eq!(slugify("Привет Мир", SlugMode::Safe), "привет-мир");
    }

    #[test]
    fn safe_keeps_digits_and_mixed_scripts() {
        assert_eq!(slugify("Rust 2024", SlugMode::Safe), "rust-2024");
        assert_eq!(slugify("שלום world", SlugMode::Safe), "שלום-world");
        assert_eq!(slugify("v1.2.3", SlugMode::Safe), "v1-2-3");
    }

    #[test]
    fn safe_drops_every_separator_but_keeps_one_dash_between_words() {
        assert_eq!(slugify("a_b/c\\d.e", SlugMode::Safe), "a-b-c-d-e");
        assert_eq!(
            slugify("tab\tand\nnewline", SlugMode::Safe),
            "tab-and-newline"
        );
        assert_eq!(slugify("--leading", SlugMode::Safe), "leading");
        assert_eq!(slugify("trailing--", SlugMode::Safe), "trailing");
    }

    #[test]
    fn ascii_drops_what_cannot_be_transliterated_cleanly() {
        assert_eq!(slugify("naïve façade", SlugMode::Ascii), "naive-facade");
        assert_eq!(slugify("Straße", SlugMode::Ascii), "strasse");
        assert_eq!(slugify("日本語", SlugMode::Ascii), "ri-ben-yu");
        let hebrew = slugify("שלום", SlugMode::Ascii);
        assert!(!hebrew.is_empty() && hebrew.is_ascii(), "{hebrew}");
    }

    #[test]
    fn ascii_is_a_superset_of_safe_on_plain_ascii() {
        for text in ["Hello World", "a--b", "  x  ", "C++ and C#", "2026-01-01"] {
            assert_eq!(
                slugify(text, SlugMode::Safe),
                slugify(text, SlugMode::Ascii),
                "{text:?}"
            );
        }
    }

    #[test]
    fn slug_mode_names_in_config() {
        assert_eq!(
            toml::from_str::<toml::Value>("m = \"ascii\"")
                .unwrap()
                .get("m")
                .cloned()
                .unwrap()
                .try_into::<SlugMode>()
                .unwrap(),
            SlugMode::Ascii
        );
        assert_eq!(
            toml::Value::String("safe".into())
                .try_into::<SlugMode>()
                .unwrap(),
            SlugMode::Safe
        );
        assert!(
            toml::Value::String("Safe".into())
                .try_into::<SlugMode>()
                .is_err(),
            "modes are lowercase"
        );
        assert_eq!(SlugMode::default(), SlugMode::Safe);
    }
}
