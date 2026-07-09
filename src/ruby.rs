use crate::error::RtError;
use crate::metadata::Metadata;
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
    pub fn resolve(root: &Path, warn: bool) -> Self {
        if let Some(explicit) = std::env::var_os("RT_RUBY") {
            return RubyCommand {
                program: explicit.to_string_lossy().into_owned(),
                prep_args: Vec::new(),
                bundle_gemfile: None,
                isolated: false,
            };
        }

        let gemfile = root.join("Gemfile");
        if gemfile.is_file() {
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
        if let Some(g) = &self.bundle_gemfile {
            cmd.env("BUNDLE_GEMFILE", g);
        }
        if self.isolated {
            cmd.env_remove("BUNDLE_GEMFILE");
            cmd.env_remove("RUBYOPT");
        }
        cmd.arg(harness);
        cmd
    }

    pub fn program(&self) -> &str {
        &self.program
    }
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

/// Materialize the embedded harness at `.rt/harness-<hash>.rb`, writing only
/// when the content differs. Returns the path.
pub fn ensure_harness(root: &Path) -> Result<PathBuf, RtError> {
    let rt_dir = root.join(".rt");
    std::fs::create_dir_all(&rt_dir)
        .map_err(|e| RtError::Internal(format!("cannot create .rt directory: {e}")))?;

    let gitignore = rt_dir.join(".gitignore");
    if !gitignore.exists() {
        let _ = std::fs::write(&gitignore, "*\n");
    }

    let mut hasher = DefaultHasher::new();
    HARNESS.hash(&mut hasher);
    let hash = hasher.finish();
    let path = rt_dir.join(format!("harness-{hash:016x}.rb"));
    if !path.exists() {
        let tmp = rt_dir.join(format!("harness-{hash:016x}.rb.{}.tmp", std::process::id()));
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
) -> Result<(Metadata, RubyCommand), RtError> {
    match discover(root, ruby) {
        Ok(meta) => Ok((meta, ruby.clone())),
        Err(err) if ruby.is_bundle() => {
            output::print_warning(&format!(
                "`bundle exec ruby` failed ({err}); retrying with plain ruby"
            ));
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
