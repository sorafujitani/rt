use crate::error::RtError;
use std::path::{Path, PathBuf};

/// Locate the project root by walking upward looking for `rt.yml` or a
/// `tasks/` directory. `RT_ROOT` overrides discovery when set.
pub fn find_root() -> Result<PathBuf, RtError> {
    if let Some(explicit) = std::env::var_os("RT_ROOT") {
        let root = PathBuf::from(explicit);
        if is_root(&root) {
            return Ok(root);
        }
        return Err(RtError::Usage(format!(
            "RT_ROOT is set to {} but no tasks/ directory or rt.yml was found there",
            root.display()
        )));
    }

    let start = std::env::current_dir()
        .map_err(|e| RtError::Internal(format!("cannot read current directory: {e}")))?;
    let mut dir = start.as_path();
    loop {
        if is_root(dir) {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    Err(RtError::Usage(
        "no rt project found: expected a tasks/ directory (or rt.yml) here or in a parent directory"
            .to_string(),
    ))
}

fn is_root(dir: &Path) -> bool {
    dir.join("tasks").is_dir() || dir.join("rt.yml").is_file()
}

#[cfg(test)]
mod tests {
    use super::is_root;
    use std::fs;

    #[test]
    fn tasks_dir_marks_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_root(dir.path()));
        fs::create_dir(dir.path().join("tasks")).unwrap();
        assert!(is_root(dir.path()));
    }

    #[test]
    fn rt_yml_marks_root() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("rt.yml"), "").unwrap();
        assert!(is_root(dir.path()));
    }
}
