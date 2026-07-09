use crate::error::RtError;
use crate::metadata::Metadata;
use crate::ruby::{self, RubyCommand};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const CACHE_VERSION: u32 = 1;
const PROTOCOL_VERSION: u32 = 2;

/// Per-file fingerprint: mtime seconds, mtime nanoseconds, and byte size. Size
/// is included because some filesystems only expose 1-second mtime resolution,
/// where a same-second same-nanos(=0) edit would otherwise be missed.
type FileMeta = (u64, u32, u64);
type FileSet = BTreeMap<String, FileMeta>;

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    cache_version: u32,
    ruby_command: String,
    files: FileSet,
    metadata: Metadata,
}

/// Return metadata from cache when nothing relevant changed; otherwise run
/// discovery and refresh the cache. Returns the interpreter that produced the
/// metadata (which may differ from `ruby` if a `bundle exec` fallback fired).
pub fn load(root: &Path, ruby: &RubyCommand) -> Result<(Metadata, RubyCommand), RtError> {
    let current = scan_files(root)?;
    let ruby_desc = ruby.describe();

    if let Some(cache) = read(root) {
        if is_valid(&cache, &ruby_desc, &current) {
            return Ok((cache.metadata, ruby.clone()));
        }
    }

    let (metadata, used) = ruby::discover_with_fallback(root, ruby)?;
    write(
        root,
        &Cache {
            cache_version: CACHE_VERSION,
            ruby_command: used.describe(),
            files: current,
            metadata: metadata.clone(),
        },
    );
    Ok((metadata, used))
}

fn is_valid(cache: &Cache, ruby_desc: &str, current: &FileSet) -> bool {
    cache.cache_version == CACHE_VERSION
        && cache.metadata.protocol_version == PROTOCOL_VERSION
        && cache.ruby_command == ruby_desc
        && &cache.files == current
}

fn scan_files(root: &Path) -> Result<FileSet, RtError> {
    let tasks = root.join("tasks");
    let mut files = FileSet::new();
    if !tasks.is_dir() {
        return Ok(files);
    }
    for entry in WalkDir::new(&tasks).sort_by_file_name() {
        let entry = entry.map_err(|e| RtError::Internal(format!("cannot walk tasks/: {e}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rb") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        files.insert(rel, file_meta(path)?);
    }
    Ok(files)
}

fn file_meta(path: &Path) -> Result<FileMeta, RtError> {
    let meta = std::fs::metadata(path)
        .map_err(|e| RtError::Internal(format!("cannot stat {}: {e}", path.display())))?;
    let modified = meta
        .modified()
        .map_err(|e| RtError::Internal(format!("no mtime for {}: {e}", path.display())))?;
    let dur = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|e| RtError::Internal(format!("bad mtime for {}: {e}", path.display())))?;
    Ok((dur.as_secs(), dur.subsec_nanos(), meta.len()))
}

/// Corrupt or unreadable caches are silently discarded.
fn read(root: &Path) -> Option<Cache> {
    let path = root.join(".rt/cache.json");
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Best-effort atomic write via a temp file + rename.
fn write(root: &Path, cache: &Cache) {
    let rt_dir = root.join(".rt");
    if std::fs::create_dir_all(&rt_dir).is_err() {
        return;
    }
    let Ok(json) = serde_json::to_vec(cache) else {
        return;
    };
    let tmp = rt_dir.join(format!("cache.json.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, &json).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp, rt_dir.join("cache.json"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_metadata() -> Metadata {
        Metadata {
            protocol_version: PROTOCOL_VERSION,
            tasks: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn cache_with(files: FileSet, ruby: &str) -> Cache {
        Cache {
            cache_version: CACHE_VERSION,
            ruby_command: ruby.to_string(),
            files,
            metadata: sample_metadata(),
        }
    }

    fn files(pairs: &[(&str, FileMeta)]) -> FileSet {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn valid_when_everything_matches() {
        let f = files(&[("tasks/a.rb", (10, 0, 100))]);
        let c = cache_with(f.clone(), "ruby");
        assert!(is_valid(&c, "ruby", &f));
    }

    #[test]
    fn invalid_on_ruby_command_change() {
        let f = files(&[("tasks/a.rb", (10, 0, 100))]);
        let c = cache_with(f.clone(), "ruby");
        assert!(!is_valid(&c, "bundle exec ruby", &f));
    }

    #[test]
    fn invalid_on_mtime_change() {
        let c = cache_with(files(&[("tasks/a.rb", (10, 0, 100))]), "ruby");
        let changed = files(&[("tasks/a.rb", (10, 5, 100))]);
        assert!(!is_valid(&c, "ruby", &changed));
    }

    #[test]
    fn invalid_on_size_change_at_same_mtime() {
        let c = cache_with(files(&[("tasks/a.rb", (10, 0, 100))]), "ruby");
        let changed = files(&[("tasks/a.rb", (10, 0, 101))]);
        assert!(!is_valid(&c, "ruby", &changed));
    }

    #[test]
    fn invalid_on_file_set_change() {
        let c = cache_with(files(&[("tasks/a.rb", (10, 0, 100))]), "ruby");
        let added = files(&[("tasks/a.rb", (10, 0, 100)), ("tasks/b.rb", (11, 0, 50))]);
        assert!(!is_valid(&c, "ruby", &added));
    }

    #[test]
    fn invalid_on_cache_version_bump() {
        let f = files(&[("tasks/a.rb", (10, 0, 100))]);
        let mut c = cache_with(f.clone(), "ruby");
        c.cache_version = CACHE_VERSION + 1;
        assert!(!is_valid(&c, "ruby", &f));
    }

    #[test]
    fn invalid_on_protocol_version_mismatch() {
        let f = files(&[("tasks/a.rb", (10, 0, 100))]);
        let mut c = cache_with(f.clone(), "ruby");
        c.metadata.protocol_version = PROTOCOL_VERSION + 1;
        assert!(!is_valid(&c, "ruby", &f));
    }
}
