//! What the last build wrote, so the next one can remove what it no longer
//! writes: a deleted page's file would otherwise stay in the output
//! directory and be deployed.
//!
//! The record is kept outside the site, in `$XDG_CACHE_HOME/rssite/outputs`
//! (`~/.cache/rssite/outputs`), one file per output directory: nothing is
//! added to the site's tree to ignore, and nothing to the output to deploy.
//! Without it (a first build, a fresh checkout, a cleared cache) nothing is
//! removed. Only paths the record names are ever deleted, so a file another
//! tool writes into the output directory is left alone.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize)]
struct Record {
    /// The output directory the paths are relative to, to tell two
    /// directories with one hash apart.
    output_dir: String,
    paths: BTreeSet<String>,
}

fn file_for(output_dir: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    output_dir.hash(&mut hasher);
    Some(crate::util::cache_dir("outputs")?.join(format!("{:016x}.json", hasher.finish())))
}

/// A recorded path is joined to the output directory and deleted, so it is
/// only ever a plain relative one.
fn is_plain(path: &str) -> bool {
    !path.is_empty()
        && !Path::new(path).is_absolute()
        && path
            .split(['/', '\\'])
            .all(|part| part != ".." && !part.is_empty())
}

/// The paths the last build of `output_dir` wrote, relative to it; empty
/// when there is no record of one.
pub fn load(output_dir: &Path) -> BTreeSet<String> {
    let Some(file) = file_for(output_dir) else {
        return BTreeSet::new();
    };
    let Ok(text) = std::fs::read_to_string(file) else {
        return BTreeSet::new();
    };
    match serde_json::from_str::<Record>(&text) {
        Ok(record) if Path::new(&record.output_dir) == output_dir => {
            record.paths.into_iter().filter(|p| is_plain(p)).collect()
        }
        _ => BTreeSet::new(),
    }
}

/// Best effort, like every cache: a record that cannot be written only
/// means the next build removes nothing.
pub fn save<'a>(output_dir: &Path, paths: impl Iterator<Item = &'a String>) {
    let Some(file) = file_for(output_dir) else {
        return;
    };
    let record = Record {
        output_dir: output_dir.to_string_lossy().into_owned(),
        paths: paths.cloned().collect(),
    };
    let Ok(text) = serde_json::to_string(&record) else {
        return;
    };
    if let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let partial = file.with_extension(format!("{}.part", std::process::id()));
        if std::fs::write(&partial, text).is_ok() {
            let _ = std::fs::rename(&partial, &file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_relative_paths_are_recorded_paths() {
        assert!(is_plain("en/blog/index.html"));
        assert!(is_plain("style.css"));
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a//b", "a\\..\\b"] {
            assert!(!is_plain(bad), "{bad:?}");
        }
    }
}
