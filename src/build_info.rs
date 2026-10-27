//! When the site was built, for a footer that says so. Read once per build,
//! so every page of one build (and both renders of a determinism check)
//! carries the same moment. Unlike `git`, it is not a function of the
//! sources: two builds of one commit name different dates.
//! `SOURCE_DATE_EPOCH`, when set, replaces the clock, the usual way to make
//! such a build reproducible.

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct BuildInfo {
    /// `YYYY-MM-DD`, in UTC.
    pub date: String,
    /// RFC 3339, in UTC, to the second.
    pub time: String,
    pub year: i32,
}

impl BuildInfo {
    pub fn now() -> BuildInfo {
        let epoch = std::env::var("SOURCE_DATE_EPOCH").ok();
        BuildInfo::at(from_epoch(epoch.as_deref()).unwrap_or_else(Utc::now))
    }

    fn at(moment: DateTime<Utc>) -> BuildInfo {
        BuildInfo {
            date: moment.format("%Y-%m-%d").to_string(),
            time: moment.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            year: chrono::Datelike::year(&moment),
        }
    }
}

fn from_epoch(value: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(value?.trim().parse().ok()?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_date_epoch_names_the_moment() {
        let info = BuildInfo::at(from_epoch(Some("1700000000")).unwrap());
        assert_eq!(info.date, "2023-11-14");
        assert_eq!(info.time, "2023-11-14T22:13:20Z");
        assert_eq!(info.year, 2023);
    }

    #[test]
    fn a_malformed_epoch_falls_back_to_the_clock() {
        assert!(from_epoch(Some("yesterday")).is_none());
        assert!(from_epoch(None).is_none());
    }
}
