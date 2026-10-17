//! Per-language translation files (`i18n/<code>.toml`) and locale-aware date
//! formatting. Month and weekday names come from the same file, so a
//! language gets its dates right by adding one table, not by hoping the
//! template engine was compiled with a locale.

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DateNames {
    pub months: Vec<String>,
    #[serde(default)]
    pub months_short: Vec<String>,
    /// Sunday first.
    #[serde(default)]
    pub weekdays: Vec<String>,
    #[serde(default)]
    pub weekdays_short: Vec<String>,
}

impl DateNames {
    pub fn english() -> DateNames {
        let months = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        let weekdays = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        DateNames {
            months: months.iter().map(|s| (*s).to_owned()).collect(),
            months_short: months.iter().map(|s| s[..3].to_owned()).collect(),
            weekdays: weekdays.iter().map(|s| (*s).to_owned()).collect(),
            weekdays_short: weekdays.iter().map(|s| s[..3].to_owned()).collect(),
        }
    }

    fn validate(&self, path: &Path) -> Result<()> {
        if self.months.len() != 12 {
            bail!("{}: [dates] months must list 12 names", path.display());
        }
        if !self.months_short.is_empty() && self.months_short.len() != 12 {
            bail!(
                "{}: [dates] months_short must list 12 names",
                path.display()
            );
        }
        if !self.weekdays.is_empty() && self.weekdays.len() != 7 {
            bail!(
                "{}: [dates] weekdays must list 7 names, Sunday first",
                path.display()
            );
        }
        if !self.weekdays_short.is_empty() && self.weekdays_short.len() != 7 {
            bail!(
                "{}: [dates] weekdays_short must list 7 names, Sunday first",
                path.display()
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Translations {
    pub code: String,
    /// The files these came from, in the order they were layered (a
    /// theme's first, the site's last); empty when none exists.
    pub paths: Vec<PathBuf>,
    pub strings: BTreeMap<String, String>,
    pub dates: DateNames,
    /// Whether a `[dates]` table was read, as opposed to the English
    /// defaults standing in; a layer without one leaves an earlier layer's
    /// names alone.
    dates_set: bool,
}

impl Translations {
    /// Load `<dir>/<code>.toml` from each of `dirs` in turn, later ones
    /// overriding earlier ones key by key (a theme's directory first, the
    /// site's last). A missing file contributes nothing; no file at all is
    /// an empty table with English date names, so a single-language site
    /// needs no i18n directory.
    pub fn load_layered(dirs: &[PathBuf], code: &str) -> Result<Translations> {
        let mut merged = Translations {
            code: code.to_owned(),
            paths: Vec::new(),
            strings: BTreeMap::new(),
            dates: DateNames::english(),
            dates_set: false,
        };
        for dir in dirs {
            let layer = Self::load(dir, code)?;
            if layer.paths.is_empty() {
                continue;
            }
            merged.paths.extend(layer.paths);
            merged.strings.extend(layer.strings);
            if layer.dates_set {
                merged.dates = layer.dates;
                merged.dates_set = true;
            }
        }
        Ok(merged)
    }

    /// Load `<dir>/<code>.toml`. A missing file is an empty table with
    /// English date names, so a single-language site needs no i18n directory.
    pub fn load(dir: &Path, code: &str) -> Result<Translations> {
        let path = dir.join(format!("{code}.toml"));
        if !path.is_file() {
            return Ok(Translations {
                code: code.to_owned(),
                paths: Vec::new(),
                strings: BTreeMap::new(),
                dates: DateNames::english(),
                dates_set: false,
            });
        }
        let text = crate::util::read(&path)?;
        let table: toml::Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let mut strings = BTreeMap::new();
        let mut dates = DateNames::english();
        let mut dates_set = false;
        for (key, value) in table {
            match (key.as_str(), value) {
                ("dates", toml::Value::Table(t)) => {
                    dates_set = true;
                    let names: DateNames = toml::Value::Table(t)
                        .try_into()
                        .with_context(|| format!("{}: [dates]", path.display()))?;
                    names.validate(&path)?;
                    if names.months_short.is_empty()
                        || names.weekdays.is_empty()
                        || names.weekdays_short.is_empty()
                    {
                        // Fill the optional lists from English rather than
                        // leaving a short name blank in the output.
                        let english = DateNames::english();
                        dates = DateNames {
                            months: names.months,
                            months_short: if names.months_short.is_empty() {
                                english.months_short
                            } else {
                                names.months_short
                            },
                            weekdays: if names.weekdays.is_empty() {
                                english.weekdays
                            } else {
                                names.weekdays
                            },
                            weekdays_short: if names.weekdays_short.is_empty() {
                                english.weekdays_short
                            } else {
                                names.weekdays_short
                            },
                        };
                    } else {
                        dates = names;
                    }
                }
                (_, toml::Value::String(s)) => {
                    strings.insert(key, s);
                }
                (k, other) => bail!(
                    "{}: key {k:?} must be a string, found a {}",
                    path.display(),
                    other.type_str()
                ),
            }
        }
        Ok(Translations {
            code: code.to_owned(),
            paths: vec![path],
            strings,
            dates,
            dates_set,
        })
    }

    /// The translation of `key`, or an error naming the file it is missing
    /// from. Never a silent fallback to another language.
    pub fn get(&self, key: &str) -> Result<&str> {
        match self.strings.get(key) {
            Some(s) => Ok(s),
            None => {
                let file = match self.paths.last() {
                    Some(p) => p.display().to_string(),
                    None => format!("i18n/{}.toml (no such file)", self.code),
                };
                bail!(
                    "translation key {key:?} is missing for language {:?} in {file}",
                    self.code
                )
            }
        }
    }
}

/// A parsed date value as the content layer stores it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateValue {
    pub datetime: DateTime<FixedOffset>,
}

impl DateValue {
    /// Parse `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM:SS`, or RFC 3339 with offset.
    pub fn parse(text: &str) -> Result<DateValue> {
        let text = text.trim();
        if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            let naive = date.and_hms_opt(0, 0, 0).expect("midnight exists");
            return Ok(DateValue {
                datetime: Utc.from_utc_datetime(&naive).fixed_offset(),
            });
        }
        if let Ok(datetime) = DateTime::parse_from_rfc3339(text) {
            return Ok(DateValue { datetime });
        }
        for format in [
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%dT%H:%M:%S%.f",
        ] {
            if let Ok(naive) = NaiveDateTime::parse_from_str(text, format) {
                return Ok(DateValue {
                    datetime: Utc.from_utc_datetime(&naive).fixed_offset(),
                });
            }
        }
        bail!("{text:?} is not a date: expected YYYY-MM-DD or an RFC 3339 timestamp")
    }

    pub fn ymd(&self) -> String {
        self.datetime.format("%Y-%m-%d").to_string()
    }

    pub fn rfc3339(&self) -> String {
        self.datetime.to_rfc3339()
    }
}

const MONTH: char = '\u{E000}';
const MONTH_SHORT: char = '\u{E001}';
const WEEKDAY: char = '\u{E002}';
const WEEKDAY_SHORT: char = '\u{E003}';

/// Format a date string with a strftime `format`, substituting the names in
/// `names` for `%B`, `%b`, `%A` and `%a`. An invalid format is an error, not
/// a panic and not empty output.
pub fn format_date(value: &str, format: &str, names: &DateNames) -> Result<String> {
    let date = DateValue::parse(value)?.datetime;
    let mut chrono_format = String::with_capacity(format.len());
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            chrono_format.push(c);
            continue;
        }
        match chars.peek() {
            Some('B') => {
                chars.next();
                chrono_format.push(MONTH);
            }
            Some('b') => {
                chars.next();
                chrono_format.push(MONTH_SHORT);
            }
            Some('A') => {
                chars.next();
                chrono_format.push(WEEKDAY);
            }
            Some('a') => {
                chars.next();
                chrono_format.push(WEEKDAY_SHORT);
            }
            Some('%') => {
                chars.next();
                chrono_format.push_str("%%");
            }
            _ => chrono_format.push('%'),
        }
    }
    let items: Vec<chrono::format::Item> =
        chrono::format::StrftimeItems::new(&chrono_format).collect();
    if items
        .iter()
        .any(|item| matches!(item, chrono::format::Item::Error))
    {
        bail!("date format {format:?} is not a valid strftime format");
    }
    let mut formatted = String::new();
    write!(formatted, "{}", date.format_with_items(items.iter()))
        .map_err(|_| anyhow::anyhow!("date format {format:?} cannot format {value:?}"))?;
    let month = date.month0() as usize;
    let weekday = date.weekday().num_days_from_sunday() as usize;
    Ok(formatted
        .replace(MONTH, &names.months[month])
        .replace(MONTH_SHORT, &names.months_short[month])
        .replace(WEEKDAY, &names.weekdays[weekday])
        .replace(WEEKDAY_SHORT, &names.weekdays_short[weekday]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_default_names() {
        let names = DateNames::english();
        assert_eq!(
            format_date("2026-04-28", "%-d %B %Y", &names).unwrap(),
            "28 April 2026"
        );
        assert_eq!(
            format_date("2026-04-28", "%a %b", &names).unwrap(),
            "Tue Apr"
        );
        assert_eq!(format_date("2026-04-28", "100%%", &names).unwrap(), "100%");
    }

    #[test]
    fn localized_names() {
        let mut names = DateNames::english();
        names.months[3] = "אפריל".into();
        assert_eq!(
            format_date("2026-04-28", "%-d ב%B %Y", &names).unwrap(),
            "28 באפריל 2026"
        );
    }

    #[test]
    fn rfc3339_input() {
        let names = DateNames::english();
        assert_eq!(
            format_date("2026-04-28T10:30:00+03:00", "%H:%M", &names).unwrap(),
            "10:30"
        );
    }

    #[test]
    fn invalid_format_is_an_error() {
        let names = DateNames::english();
        assert!(format_date("2026-04-28", "%Q", &names).is_err());
        assert!(format_date("not a date", "%Y", &names).is_err());
    }

    #[test]
    fn date_values_parse_every_documented_form() {
        assert_eq!(DateValue::parse("2026-04-28").unwrap().ymd(), "2026-04-28");
        assert_eq!(
            DateValue::parse("2026-04-28").unwrap().rfc3339(),
            "2026-04-28T00:00:00+00:00",
            "a bare date is midnight UTC"
        );
        assert_eq!(
            DateValue::parse("2026-04-28T10:30:00").unwrap().rfc3339(),
            "2026-04-28T10:30:00+00:00"
        );
        assert_eq!(
            DateValue::parse("2026-04-28 10:30:00").unwrap().rfc3339(),
            "2026-04-28T10:30:00+00:00"
        );
        assert_eq!(
            DateValue::parse("2026-04-28T10:30:00.5").unwrap().rfc3339(),
            "2026-04-28T10:30:00.500+00:00"
        );
        assert_eq!(
            DateValue::parse("2026-04-28T10:30:00+02:00")
                .unwrap()
                .rfc3339(),
            "2026-04-28T10:30:00+02:00",
            "an offset is kept, not normalised"
        );
        assert_eq!(
            DateValue::parse("2026-04-28T10:30:00Z").unwrap().rfc3339(),
            "2026-04-28T10:30:00+00:00"
        );
        assert_eq!(
            DateValue::parse("  2026-04-28 \n").unwrap().ymd(),
            "2026-04-28"
        );
    }

    #[test]
    fn date_values_reject_what_is_not_a_date() {
        for text in [
            "",
            "2026",
            "2026-13-01",
            "2026-02-30",
            "28/04/2026",
            "2026-04-28T25:00:00",
            "April 28",
        ] {
            let error = DateValue::parse(text).unwrap_err().to_string();
            assert!(error.contains("is not a date"), "{text:?}: {error}");
        }
    }

    #[test]
    fn date_values_order_by_instant() {
        let a = DateValue::parse("2026-04-28T10:00:00+02:00").unwrap();
        let b = DateValue::parse("2026-04-28T09:00:00+00:00").unwrap();
        let c = DateValue::parse("2026-04-28").unwrap();
        assert!(b > a, "09:00Z is after 10:00+02:00 (08:00Z)");
        assert!(a > c);
        assert_eq!(a, DateValue::parse("2026-04-28T08:00:00Z").unwrap());
    }

    #[test]
    fn every_name_placeholder_is_substituted() {
        let mut names = DateNames::english();
        names.months[0] = "ינואר".into();
        names.months_short[0] = "ינו".into();
        names.weekdays[4] = "חמישי".into();
        names.weekdays_short[4] = "ה".into();
        // 2026-01-01 is a Thursday.
        assert_eq!(
            format_date("2026-01-01", "%A %a %B %b %d", &names).unwrap(),
            "חמישי ה ינואר ינו 01"
        );
        assert_eq!(
            format_date("2026-01-01", "%B%B", &names).unwrap(),
            "ינוארינואר"
        );
    }

    #[test]
    fn percent_sequences_that_are_not_names_pass_to_chrono() {
        let names = DateNames::english();
        assert_eq!(
            format_date("2026-01-01", "%Y/%m/%d", &names).unwrap(),
            "2026/01/01"
        );
        assert_eq!(format_date("2026-01-01", "%e", &names).unwrap(), " 1");
        assert_eq!(format_date("2026-01-01", "%j", &names).unwrap(), "001");
        assert_eq!(
            format_date("2026-01-01", "%F", &names).unwrap(),
            "2026-01-01"
        );
        assert_eq!(
            format_date("2026-01-01", "literal", &names).unwrap(),
            "literal"
        );
        assert_eq!(format_date("2026-01-01", "", &names).unwrap(), "");
    }

    #[test]
    fn a_trailing_percent_is_an_error_not_a_panic() {
        let names = DateNames::english();
        assert!(format_date("2026-01-01", "%Y %", &names).is_err());
    }

    #[test]
    fn formatting_uses_the_offset_the_date_was_written_in() {
        let names = DateNames::english();
        assert_eq!(
            format_date("2026-01-01T23:30:00+02:00", "%Y-%m-%d %H:%M %z", &names).unwrap(),
            "2026-01-01 23:30 +0200"
        );
        // Crossing midnight in the offset: the weekday is the local one.
        assert_eq!(
            format_date("2026-01-01T23:30:00-05:00", "%A", &names).unwrap(),
            "Thursday"
        );
    }

    #[test]
    fn english_names_are_complete() {
        let names = DateNames::english();
        assert_eq!(names.months.len(), 12);
        assert_eq!(names.months_short.len(), 12);
        assert_eq!(names.weekdays.len(), 7);
        assert_eq!(names.weekdays_short.len(), 7);
        assert_eq!(names.weekdays[0], "Sunday");
        assert_eq!(names.months_short[8], "Sep");
        assert_eq!(names.months_short[4], "May");
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rssite-i18n-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_file_is_an_empty_table_with_english_dates() {
        let dir = temp("missing");
        let t = Translations::load(&dir, "en").unwrap();
        assert!(t.paths.is_empty());
        assert!(t.strings.is_empty());
        assert_eq!(t.dates.months[0], "January");
        let error = t.get("nav").unwrap_err().to_string();
        assert!(error.contains("i18n/en.toml (no such file)"), "{error}");
        assert!(error.contains("\"nav\""), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn strings_and_dates_load_from_a_file() {
        let dir = temp("load");
        std::fs::write(
            dir.join("he.toml"),
            "nav = \"ניווט\"\nempty = \"\"\n[dates]\nmonths = [\"א\", \"ב\", \"ג\", \"ד\", \"ה\", \"ו\", \"ז\", \"ח\", \"ט\", \"י\", \"יא\", \"יב\"]\nweekdays = [\"ראשון\", \"שני\", \"שלישי\", \"רביעי\", \"חמישי\", \"שישי\", \"שבת\"]\n",
        )
        .unwrap();
        let t = Translations::load(&dir, "he").unwrap();
        assert_eq!(t.paths, vec![dir.join("he.toml")]);
        assert_eq!(t.get("nav").unwrap(), "ניווט");
        assert_eq!(t.get("empty").unwrap(), "");
        assert_eq!(t.dates.months[11], "יב");
        assert_eq!(t.dates.weekdays[6], "שבת");
        assert_eq!(
            t.dates.months_short[0], "Jan",
            "unset short names fall back to English"
        );
        assert_eq!(t.dates.weekdays_short[0], "Sun");
        let error = t.get("missing").unwrap_err().to_string();
        assert!(error.contains("he.toml"), "{error}");
        assert!(error.contains("\"he\""), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn non_string_values_are_rejected() {
        let dir = temp("types");
        for text in ["n = 1\n", "b = true\n", "l = [\"a\"]\n", "[t]\nx = \"y\"\n"] {
            std::fs::write(dir.join("en.toml"), text).unwrap();
            let error = format!("{:#}", Translations::load(&dir, "en").unwrap_err());
            assert!(error.contains("must be a string"), "{text:?}: {error}");
            assert!(error.contains("en.toml"), "{error}");
        }
        std::fs::write(dir.join("en.toml"), "not toml =\n").unwrap();
        let error = format!("{:#}", Translations::load(&dir, "en").unwrap_err());
        assert!(error.contains("parsing"), "{error}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn date_name_lists_are_checked_for_length() {
        let dir = temp("lengths");
        let cases = [
            ("[dates]\nmonths = [\"a\"]\n", "months must list 12"),
            (
                "[dates]\nmonths = [\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"11\",\"12\"]\nmonths_short = [\"a\"]\n",
                "months_short must list 12",
            ),
            (
                "[dates]\nmonths = [\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"11\",\"12\"]\nweekdays = [\"a\"]\n",
                "weekdays must list 7",
            ),
            (
                "[dates]\nmonths = [\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"11\",\"12\"]\nweekdays_short = [\"a\"]\n",
                "weekdays_short must list 7",
            ),
            ("[dates]\nweekdays = [\"a\"]\n", "months"),
            (
                "[dates]\nmonths = [\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\",\"10\",\"11\",\"12\"]\nmonth = []\n",
                "unknown field",
            ),
        ];
        for (text, needle) in cases {
            std::fs::write(dir.join("en.toml"), text).unwrap();
            let error = format!("{:#}", Translations::load(&dir, "en").unwrap_err());
            assert!(error.contains(needle), "{text:?}: {error}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn layers_override_key_by_key_and_remember_every_file() {
        let dir = temp("layers");
        let theme = dir.join("theme");
        let site = dir.join("site");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(
            theme.join("en.toml"),
            "a = \"theme a\"\nb = \"theme b\"\n[dates]\nmonths = [\"t1\",\"t2\",\"t3\",\"t4\",\"t5\",\"t6\",\"t7\",\"t8\",\"t9\",\"t10\",\"t11\",\"t12\"]\n",
        )
        .unwrap();
        std::fs::write(site.join("en.toml"), "b = \"site b\"\nc = \"site c\"\n").unwrap();
        let t = Translations::load_layered(&[theme.clone(), site.clone()], "en").unwrap();
        assert_eq!(t.get("a").unwrap(), "theme a");
        assert_eq!(t.get("b").unwrap(), "site b");
        assert_eq!(t.get("c").unwrap(), "site c");
        assert_eq!(t.paths, vec![theme.join("en.toml"), site.join("en.toml")]);
        assert_eq!(
            t.dates.months[0], "t1",
            "a layer without [dates] leaves the theme's names alone"
        );
        let error = t.get("d").unwrap_err().to_string();
        assert!(
            error.contains("site/en.toml"),
            "the site's file is named:\n{error}"
        );
        // A layer without the file contributes nothing and is not listed.
        let only_theme =
            Translations::load_layered(&[site.join("none"), theme.clone()], "en").unwrap();
        assert_eq!(only_theme.paths, vec![theme.join("en.toml")]);
        assert_eq!(only_theme.dates.months[0], "t1");
        // No layer at all.
        let none = Translations::load_layered(&[], "en").unwrap();
        assert!(none.paths.is_empty());
        assert_eq!(none.code, "en");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
