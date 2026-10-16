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
        };
        for dir in dirs {
            let layer = Self::load(dir, code)?;
            if layer.paths.is_empty() {
                continue;
            }
            merged.paths.extend(layer.paths);
            merged.strings.extend(layer.strings);
            merged.dates = layer.dates;
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
            });
        }
        let text = crate::util::read(&path)?;
        let table: toml::Table =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let mut strings = BTreeMap::new();
        let mut dates = DateNames::english();
        for (key, value) in table {
            match (key.as_str(), value) {
                ("dates", toml::Value::Table(t)) => {
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
}
