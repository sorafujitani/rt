use crate::error::RtError;
use std::path::{Path, PathBuf};

/// The task homes rt will search. A home holds `tasks/**/*.rb` and rt's
/// generated files (cache, harness): `<repo>/.rt/` for a project, the config
/// dir for global tasks. Either may be absent; both absent is a usage error.
#[derive(Debug, Clone)]
pub struct Roots {
    pub project: Option<PathBuf>,
    pub global: Option<PathBuf>,
}

/// Resolve both the project home (walked up from the cwd, or `RT_ROOT`) and the
/// global home (`<config_dir>/` when it contains a `tasks/` directory). At
/// least one must exist.
pub fn find_roots() -> Result<Roots, RtError> {
    let project = find_project()?;
    let mut global = find_global();

    // A project home inside the config dir would make project and global the
    // same directory, discovering every task twice and reporting them all as
    // shadowed. Collapse to a single (project) home in that case.
    if let (Some(p), Some(g)) = (&project, &global) {
        if same_dir(p, g) {
            global = None;
        }
    }

    if project.is_none() && global.is_none() {
        return Err(RtError::Usage(
            "no rt project found: expected a .rt/ directory here or in a parent directory. \
             To define machine-wide tasks, create a tasks/ directory under your rt config dir \
             (~/.config/rt or $RT_CONFIG_DIR)."
                .to_string(),
        ));
    }
    Ok(Roots { project, global })
}

/// Locate the project home by walking upward looking for a `.rt/` directory.
/// `RT_ROOT` (the directory containing `.rt/`) overrides discovery; an invalid
/// `RT_ROOT` is an error, but a plain "not found" walk yields `None` (global
/// tasks may still apply).
fn find_project() -> Result<Option<PathBuf>, RtError> {
    if let Some(explicit) = std::env::var_os("RT_ROOT") {
        let root = PathBuf::from(explicit);
        if let Some(home) = home_of(&root) {
            return Ok(Some(home));
        }
        return Err(RtError::Usage(format!(
            "RT_ROOT is set to {} but no .rt/ directory was found there",
            root.display()
        )));
    }

    let start = std::env::current_dir()
        .map_err(|e| RtError::Internal(format!("cannot read current directory: {e}")))?;
    let mut dir = start.as_path();
    loop {
        if let Some(home) = home_of(dir) {
            return Ok(Some(home));
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Ok(None),
        }
    }
}

/// The global home is the config dir itself when it holds a `tasks/` directory.
fn find_global() -> Option<PathBuf> {
    let dir = config_dir()?;
    if dir.join("tasks").is_dir() {
        Some(dir)
    } else {
        None
    }
}

/// Resolution order: `RT_CONFIG_DIR` -> `$XDG_CONFIG_HOME/rt` -> `~/.config/rt`.
fn config_dir() -> Option<PathBuf> {
    if let Some(explicit) = non_empty_env("RT_CONFIG_DIR") {
        return Some(PathBuf::from(explicit));
    }
    if let Some(xdg) = non_empty_env("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join("rt"));
    }
    non_empty_env("HOME").map(|home| PathBuf::from(home).join(".config").join("rt"))
}

fn non_empty_env(key: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(key).filter(|v| !v.is_empty())
}

/// A directory is a project root when it contains a `.rt/` directory; that
/// `.rt/` is the project's home.
fn home_of(dir: &Path) -> Option<PathBuf> {
    let home = dir.join(".rt");
    home.is_dir().then_some(home)
}

/// Whether two paths point at the same directory, comparing canonical forms
/// when available and falling back to a literal comparison.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::home_of;
    use std::fs;

    #[test]
    fn rt_dir_marks_root_and_is_the_home() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(home_of(dir.path()), None);
        fs::create_dir(dir.path().join(".rt")).unwrap();
        assert_eq!(home_of(dir.path()), Some(dir.path().join(".rt")));
    }

    #[test]
    fn rt_file_is_not_a_home() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".rt"), "").unwrap();
        assert_eq!(home_of(dir.path()), None);
    }

    #[test]
    fn tasks_dir_alone_is_not_a_home() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("tasks")).unwrap();
        assert_eq!(home_of(dir.path()), None);
    }
}
