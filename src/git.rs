//! The commit the site is built from, for a footer that names it. Read
//! once, from `git` itself, and handed to templates as `git`. The files a
//! commit changes (`HEAD`, the branch's ref, the index) are recorded as
//! sources of every output whose template reads `git`, so a new commit
//! rebuilds exactly those.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct GitInfo {
    pub commit: String,
    pub short: String,
    /// The committer date, `YYYY-MM-DD`: when the sources were committed,
    /// not when a machine happened to build them.
    pub date: String,
    pub year: i32,
    /// Whether the working tree differs from the commit. The one field that
    /// is not a function of the recorded sources: an edit nothing staged
    /// changes it without changing them.
    pub dirty: bool,
    /// The git files a commit rewrites, relative to the site root.
    #[serde(skip)]
    pub sources: Vec<String>,
}

fn git(root: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl GitInfo {
    /// `None` when git is missing or the root is not in a repository with a
    /// commit: the templates see `git` as null and leave the line out.
    pub fn read(root: &std::path::Path) -> Option<GitInfo> {
        let commit = git(root, &["rev-parse", "--verify", "-q", "HEAD"])?;
        if commit.is_empty() {
            return None;
        }
        let short = git(root, &["rev-parse", "--short", "HEAD"])?;
        let date = git(root, &["log", "-1", "--format=%cs", "HEAD"])?;
        let year = date.get(..4).and_then(|y| y.parse().ok()).unwrap_or(0);
        let dirty = !git(root, &["status", "--porcelain"])?.is_empty();
        let mut names = vec!["HEAD".to_owned(), "index".to_owned()];
        if let Some(branch) = git(root, &["symbolic-ref", "-q", "HEAD"]) {
            names.push(branch);
            // A branch whose ref was packed lives in packed-refs.
            names.push("packed-refs".to_owned());
        }
        let mut sources = Vec::new();
        for name in names {
            if let Some(path) = git(root, &["rev-parse", "--git-path", &name])
                && root.join(&path).is_file()
            {
                sources.push(normalize(&path));
            }
        }
        sources.sort();
        sources.dedup();
        Some(GitInfo {
            commit,
            short,
            date,
            year,
            dirty,
            sources,
        })
    }
}

/// `git rev-parse --git-path` answers relative to `-C`, which is the site
/// root, or absolutely for a repository elsewhere; either is a source path.
fn normalize(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

/// Whether a template reads `git`: `git.short`, `if git`, `git | ...`.
pub fn reads_git(text: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"\bgit\b\s*(?:\.\w|\||\}\}|-\}\}|%\}|-%\}|\)|and\b|or\b)")
            .expect("valid regex")
    });
    re.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_git_is_detected_in_its_usual_shapes() {
        for yes in [
            "{{ git.short }}",
            "{% if git %}x{% endif %}",
            "{%- if git -%}",
            "{{ git | json_encode }}",
            "{% if git and git.dirty %}",
            "{{ f(x=git) }}",
        ] {
            assert!(reads_git(yes), "{yes}");
        }
        for no in [
            "https://github.com/veltzer",
            "veltzer.github.io",
            "a digit",
            "the git history",
        ] {
            assert!(!reads_git(no), "{no}");
        }
    }

    #[test]
    fn a_directory_outside_git_has_no_info() {
        let dir = std::env::temp_dir().join(format!("rssite-nogit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Only meaningful where the temporary directory is not itself in a
        // repository; skipped otherwise.
        let info = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"])
            .output();
        if let Ok(out) = info
            && !out.status.success()
        {
            assert!(GitInfo::read(&dir).is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
