use crate::error::RtError;
use crate::metadata::{Metadata, Source};
use crate::output;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

const HARNESS: &str = include_str!("harness.rb");

/// How rt will invoke Ruby. Resolved once and reused for both discover and run
/// so a mismatch cannot invalidate the cache mid-session.
#[derive(Debug, Clone)]
pub struct RubyCommand {
    program: String,
    prep_args: Vec<String>,
    bundle_gemfile: Option<PathBuf>,
    /// When set, the child runs outside any Bundler context (BUNDLE_GEMFILE and
    /// RUBYOPT stripped). Used for inline-gem tasks so bundler/inline does not
    /// collide with an active `bundle exec`.
    isolated: bool,
}

impl RubyCommand {
    /// Resolution order: RT_RUBY -> `bundle exec ruby` (if a Gemfile and
    /// bundle exist) -> plain `ruby` on PATH. `warn` is false in --json mode so
    /// the fallback notice never pollutes stdout's JSON companion, stderr.
    pub fn resolve(root: &Path, source: Source, warn: bool) -> Self {
        if let Some(explicit) = std::env::var_os("RT_RUBY") {
            return RubyCommand {
                program: explicit.to_string_lossy().into_owned(),
                prep_args: Vec::new(),
                bundle_gemfile: None,
                isolated: false,
            };
        }

        if let Some(gemfile) = find_gemfile(root, source) {
            if bundle_available() {
                return RubyCommand {
                    program: "bundle".to_string(),
                    prep_args: vec!["exec".to_string(), "ruby".to_string()],
                    bundle_gemfile: Some(gemfile),
                    isolated: false,
                };
            }
            if warn {
                output::print_warning(
                    "found a Gemfile but `bundle` is not on PATH; falling back to plain ruby",
                );
            }
        }

        Self::plain()
    }

    /// Plain `ruby` on PATH, no Bundler wrapping.
    pub fn plain() -> Self {
        RubyCommand {
            program: "ruby".to_string(),
            prep_args: Vec::new(),
            bundle_gemfile: None,
            isolated: false,
        }
    }

    /// Plain Ruby with any inherited Bundler context stripped, so bundler/inline
    /// can resolve a task's declared gems without fighting an active
    /// `bundle exec`. Honors RT_RUBY for interpreter selection.
    pub fn plain_isolated() -> Self {
        let program = std::env::var_os("RT_RUBY")
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "ruby".to_string());
        RubyCommand {
            program,
            prep_args: Vec::new(),
            bundle_gemfile: None,
            isolated: true,
        }
    }

    fn is_bundle(&self) -> bool {
        self.bundle_gemfile.is_some()
    }

    /// Stable string recorded in the cache; a change invalidates it.
    pub fn describe(&self) -> String {
        let mut s = self.program.clone();
        for a in &self.prep_args {
            s.push(' ');
            s.push_str(a);
        }
        if let Some(g) = &self.bundle_gemfile {
            s.push_str(" @");
            s.push_str(&g.to_string_lossy());
        }
        s
    }

    /// A Command with the interpreter, any `bundle exec` prefix, the Gemfile
    /// env, and the harness path already applied. Callers append the mode.
    pub fn command(&self, harness: &Path) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.prep_args);

        // Minimal scrub on every path: a RUBYOPT/RUBYLIB inherited from the
        // caller's shell (e.g. rt launched under `bundle exec`) injects requires
        // and load paths that can break the harness before it runs. A
        // user-intended RUBYOPT (--yjit) is lost too; noted in the README.
        cmd.env_remove("RUBYOPT");
        cmd.env_remove("RUBYLIB");

        if let Some(g) = &self.bundle_gemfile {
            cmd.env("BUNDLE_GEMFILE", g);
        }

        if self.isolated {
            // Full scrub: the harness builds its own GEM_HOME for inline-gem
            // installs, so every inherited gem/bundler variable that could
            // redirect resolution is stripped. GEM_HOME/GEM_PATH are left alone
            // on non-isolated paths so a bundle living under the user's GEM_HOME
            // keeps working.
            cmd.env_remove("GEM_HOME");
            cmd.env_remove("GEM_PATH");
            cmd.env_remove("RUBYGEMS_GEMDEPS");
            for (key, _) in std::env::vars_os() {
                let name = key.to_string_lossy();
                // `bundle exec` exports both BUNDLE_* (config) and BUNDLER_*
                // (e.g. BUNDLER_VERSION, BUNDLER_ORIG_GEM_HOME) into the child.
                if name.starts_with("BUNDLE_") || name.starts_with("BUNDLER_") {
                    cmd.env_remove(&key);
                }
            }
        }
        cmd.arg(harness);
        cmd
    }

    pub fn program(&self) -> &str {
        &self.program
    }
}

/// A Gemfile inside the home itself wins as the more specific location. Only a
/// project home also checks its parent (the repo root, where a project's bundle
/// usually lives); the global home's parent (e.g. ~/.config) is not a project
/// root and must never be consulted.
fn find_gemfile(root: &Path, source: Source) -> Option<PathBuf> {
    let parent = match source {
        Source::Project => root.parent().map(|p| p.join("Gemfile")),
        Source::Global => None,
    };
    [Some(root.join("Gemfile")), parent]
        .into_iter()
        .flatten()
        .find(|g| g.is_file())
}

fn bundle_available() -> bool {
    Command::new("bundle")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Create the task home and converge rt-managed ignore rules. This runs even
/// when metadata comes from cache, so stale generated state cannot survive an
/// otherwise read-only invocation.
pub fn maintain_home(root: &Path) -> Result<(), RtError> {
    std::fs::create_dir_all(root)
        .map_err(|e| RtError::Internal(format!("cannot create rt home directory: {e}")))?;

    // The home also holds tasks/ (versioned), so only rt's generated files may
    // be ignored, anchored to the home so nothing under tasks/ ever matches.
    // Older rt versions wrote a blanket `*` here (the home was generated-only
    // back then); left in place it would keep tasks/ invisible to git, so it is
    // rewritten. Any other user-edited content is preserved.
    let gitignore = root.join(".gitignore");
    let stale = std::fs::read_to_string(&gitignore)
        .map(|content| content.trim() == "*")
        .unwrap_or(true);
    if stale {
        let _ = std::fs::write(&gitignore, "/cache.json\n/harness-*.rb\n/*.tmp\n");
    }
    Ok(())
}

/// Materialize the embedded harness at `<home>/harness-<hash>.rb`, writing
/// only when the content differs. Returns the path.
pub fn ensure_harness(root: &Path) -> Result<PathBuf, RtError> {
    maintain_home(root)?;

    let mut hasher = DefaultHasher::new();
    HARNESS.hash(&mut hasher);
    let hash = hasher.finish();
    let path = root.join(format!("harness-{hash:016x}.rb"));
    if !path.exists() {
        let tmp = root.join(format!("harness-{hash:016x}.rb.{}.tmp", std::process::id()));
        std::fs::write(&tmp, HARNESS)
            .map_err(|e| RtError::Internal(format!("cannot write harness: {e}")))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| RtError::Internal(format!("cannot install harness: {e}")))?;
    }
    Ok(path)
}

/// Discover metadata, falling back to plain ruby once if a `bundle exec`
/// interpreter fails (e.g. gems not installed). Returns the interpreter that
/// actually produced the metadata so callers can cache and reuse it.
pub fn discover_with_fallback(
    root: &Path,
    ruby: &RubyCommand,
    warn: bool,
) -> Result<(Metadata, RubyCommand), RtError> {
    match discover(root, ruby) {
        Ok(meta) => Ok((meta, ruby.clone())),
        Err(err) if ruby.is_bundle() => {
            if warn {
                output::print_warning(&format!(
                    "`bundle exec ruby` failed ({err}); retrying with plain ruby"
                ));
            }
            let plain = RubyCommand::plain();
            let meta = discover(root, &plain)?;
            Ok((meta, plain))
        }
        Err(err) => Err(err),
    }
}

/// Load task metadata by running the harness in --emit-metadata mode.
pub fn discover(root: &Path, ruby: &RubyCommand) -> Result<Metadata, RtError> {
    let harness = ensure_harness(root)?;
    let output = ruby
        .command(&harness)
        .arg("--emit-metadata")
        .arg(root)
        .output()
        .map_err(|e| environment_error(ruby, &e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RtError::Internal(format!(
            "harness failed during discovery: {}",
            stderr.trim()
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| RtError::Internal(format!("could not parse task metadata: {e}")))
}

pub fn environment_error(ruby: &RubyCommand, e: &std::io::Error) -> RtError {
    RtError::Environment(format!(
        "could not start Ruby ({}): {e}. Install Ruby or set RT_RUBY to a Ruby interpreter.",
        ruby.program()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    /// Keys the Command explicitly clears (env_remove -> value None).
    fn removed(cmd: &Command) -> Vec<String> {
        cmd.get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect()
    }

    /// The value a Command sets for `key`, if any.
    fn set_value<'a>(cmd: &'a Command, key: &str) -> Option<&'a OsStr> {
        cmd.get_envs()
            .find(|(k, _)| *k == OsStr::new(key))
            .and_then(|(_, v)| v)
    }

    fn bundle(gemfile: &str) -> RubyCommand {
        RubyCommand {
            program: "bundle".to_string(),
            prep_args: vec!["exec".to_string(), "ruby".to_string()],
            bundle_gemfile: Some(PathBuf::from(gemfile)),
            isolated: false,
        }
    }

    #[test]
    fn plain_command_minimally_scrubs_only_rubyopt_and_rubylib() {
        let cmd = RubyCommand::plain().command(Path::new("/h.rb"));
        let removed = removed(&cmd);
        assert!(removed.contains(&"RUBYOPT".to_string()));
        assert!(removed.contains(&"RUBYLIB".to_string()));
        // GEM_HOME/GEM_PATH must survive so a bundle under the user's GEM_HOME works.
        assert!(!removed.contains(&"GEM_HOME".to_string()));
        assert!(!removed.contains(&"GEM_PATH".to_string()));
    }

    #[test]
    fn bundle_command_sets_gemfile_and_minimally_scrubs() {
        let cmd = bundle("/proj/Gemfile").command(Path::new("/h.rb"));
        assert_eq!(
            set_value(&cmd, "BUNDLE_GEMFILE"),
            Some(OsStr::new("/proj/Gemfile"))
        );
        let removed = removed(&cmd);
        assert!(removed.contains(&"RUBYOPT".to_string()));
        assert!(removed.contains(&"RUBYLIB".to_string()));
        assert!(!removed.contains(&"GEM_HOME".to_string()));
    }

    #[test]
    fn isolated_command_fully_scrubs_gem_and_bundle_env() {
        // Ambient BUNDLE_*/BUNDLER_* vars so the dynamic sweep has work to do.
        std::env::set_var("BUNDLE_PATH", "/somewhere");
        std::env::set_var("BUNDLER_VERSION", "2.5.0");
        let cmd = RubyCommand::plain_isolated().command(Path::new("/h.rb"));
        let removed = removed(&cmd);
        for key in [
            "RUBYOPT",
            "RUBYLIB",
            "GEM_HOME",
            "GEM_PATH",
            "RUBYGEMS_GEMDEPS",
            "BUNDLE_PATH",
            "BUNDLER_VERSION",
        ] {
            assert!(
                removed.contains(&key.to_string()),
                "isolated command must strip {key}"
            );
        }
        // The command does not re-set a Gemfile env on the isolated path.
        assert_eq!(set_value(&cmd, "BUNDLE_GEMFILE"), None::<&OsStr>);
        std::env::remove_var("BUNDLE_PATH");
        std::env::remove_var("BUNDLER_VERSION");
    }
}
