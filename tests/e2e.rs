use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A process-lifetime empty directory used as the default config dir so tests
/// never pick up the real ~/.config/rt. Held in a static so it outlives every
/// test (statics are not dropped, so the tempdir survives to process exit).
fn empty_config() -> &'static Path {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().unwrap()).path()
}

/// Copy a fixture into a fresh tempdir so cache writes don't touch the repo.
fn stage(fixture: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = fixtures_dir().join(fixture);
    copy_dir(&src, dir.path());
    dir
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else if is_rt_generated_file(&path) {
            continue;
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

fn is_rt_generated_file(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    if !parent.join("tasks").is_dir() {
        return false;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name == "cache.json"
        || name == ".gitignore"
        || name.ends_with(".tmp")
        || (name.starts_with("harness-") && name.ends_with(".rb"))
}

#[test]
fn fixture_copy_excludes_rt_generated_files() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let home = src.path().join(".rt");
    std::fs::create_dir_all(home.join("tasks")).unwrap();
    std::fs::write(home.join("tasks/greet.rb"), "task(\"greet\") {}\n").unwrap();
    std::fs::write(home.join("cache.json"), "{}").unwrap();
    std::fs::write(home.join("harness-deadbeef.rb"), "generated").unwrap();
    std::fs::write(home.join(".gitignore"), "*\n").unwrap();

    copy_dir(src.path(), dst.path());

    assert!(dst.path().join(".rt/tasks/greet.rb").is_file());
    assert!(!dst.path().join(".rt/cache.json").exists());
    assert!(!dst.path().join(".rt/harness-deadbeef.rb").exists());
    assert!(!dst.path().join(".rt/.gitignore").exists());
}

fn rt() -> Command {
    let mut cmd = Command::cargo_bin("rt").unwrap();
    // Isolate every test from the real user config dir so global-task discovery
    // finds nothing unless a test opts in with its own RT_CONFIG_DIR.
    cmd.env("RT_CONFIG_DIR", empty_config());
    cmd
}

#[test]
fn missing_rt_dir_is_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    rt().arg("list")
        .current_dir(dir.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn plain_tasks_dir_without_rt_dir_is_not_a_project() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
    std::fs::write(dir.path().join("tasks/x.rb"), "task(\"x\") {}\n").unwrap();
    rt().arg("list")
        .current_dir(dir.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains(".rt/"));
}

#[test]
fn discovers_root_from_subdirectory() {
    let staged = stage("nested");
    let deep = staged.path().join("sub/deeper");
    std::fs::create_dir_all(&deep).unwrap();
    rt().arg("list")
        .current_dir(&deep)
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("hello"));
}

#[test]
fn list_shows_names_and_descriptions() {
    let staged = stage("basic");
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"))
        .stdout(predicates::str::contains("Greet someone by name"));
}

#[test]
fn list_json_emits_only_json_on_stdout() {
    let staged = stage("basic");
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["protocol_version"], 2);
    assert!(!value["tasks"].as_array().unwrap().is_empty());
}

#[test]
fn broken_task_file_warns_on_stderr_but_lists_healthy() {
    let staged = stage("broken");
    let assert = rt()
        .arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("healthy"));
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("warning"));
    assert!(stderr.contains("broken.rb"));
}

#[test]
fn missing_ruby_is_environment_error() {
    let staged = stage("basic");
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_RUBY", "/nonexistent")
        .assert()
        .failure()
        .code(74)
        .stderr(predicates::str::contains("Ruby"));
}

#[test]
fn run_json_reports_missing_ruby_as_json() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "greet"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_RUBY", "/nonexistent")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "error");
    assert_eq!(value["exit_code"], 74);
    assert_eq!(value["error"]["kind"], "environment");
}

#[test]
fn run_json_reports_missing_project_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let out = rt()
        .args(["run", "--json", "greet"])
        .current_dir(dir.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "usage");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("no rt project found"));
}

#[test]
fn run_json_reports_cli_parse_errors_as_json() {
    let out = rt().args(["run", "--json"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["task"], "");
    assert_eq!(value["error"]["kind"], "usage");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("required arguments"));
}

#[test]
fn run_task_produces_output() {
    let staged = stage("basic");
    rt().args(["run", "greet", "--name", "sora"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("Hello, sora!"));
}

#[test]
fn run_json_captures_successful_execution() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "greet", "--name", "sora"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["task"], "greet");
    assert_eq!(value["status"], "success");
    assert_eq!(value["exit_code"], 0);
    assert_eq!(value["stdout"]["encoding"], "utf-8");
    assert_eq!(value["stdout"]["data"], "Hello, sora!\n");
    assert_eq!(value["stderr"]["data"], "");
    assert!(value["error"].is_null());
}

#[test]
fn run_json_captures_both_output_streams() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "both_streams"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["stdout"]["data"], "out");
    assert_eq!(value["stderr"]["data"], "err");
}

#[test]
fn run_json_reports_usage_errors_as_json() {
    let staged = stage("params");
    let out = rt()
        .args(["run", "--json", "deploy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "error");
    assert_eq!(value["exit_code"], 2);
    assert_eq!(value["error"]["kind"], "usage");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("missing required argument"));
}

#[test]
fn run_json_reports_task_exception_structurally() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "boom"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "task_exception");
    assert_eq!(value["error"]["class"], "RuntimeError");
    assert_eq!(value["error"]["message"], "kaboom");
    assert!(value["error"]["backtrace"].is_array());
}

#[test]
fn run_json_hides_exception_sentinel_and_encodes_binary_stderr() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "boom_binary"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stderr.is_empty());
    assert!(!out
        .stdout
        .windows(b"__RT_ERROR__".len())
        .any(|window| window == b"__RT_ERROR__"));

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["class"], "RuntimeError");
    assert_eq!(value["stderr"]["encoding"], "base64");
    assert_eq!(value["stderr"]["data"], "//4=");
}

#[test]
fn run_json_preserves_custom_exit_code() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "bail"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["exit_code"], 3);
    assert_eq!(value["error"]["kind"], "task_exit");
}

#[test]
fn run_json_base64_encodes_non_utf8_output() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "binary_stdout"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["stdout"]["encoding"], "base64");
    assert_eq!(value["stdout"]["data"], "//4=");
}

#[test]
fn run_json_drains_large_stdout_and_stderr_without_deadlock() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "large_streams"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["stdout"]["data"].as_str().unwrap().len(), 131_072);
    assert_eq!(value["stderr"]["data"].as_str().unwrap().len(), 131_072);
}

#[test]
fn run_json_keeps_load_errors_in_the_result() {
    let staged = stage("broken");
    let out = rt()
        .args(["run", "--json", "healthy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!value["load_errors"].as_array().unwrap().is_empty());
    assert_eq!(value["stdout"]["data"], "ok\n");
}

#[test]
fn task_can_receive_its_own_json_option_after_separator() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "owns_json", "--", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["stdout"]["data"], "true\n");
}

#[test]
fn run_dry_run_sets_ctx_dry_run() {
    let staged = stage("basic");
    rt().args(["run", "preview", "--dry-run"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("dry run"));
}

#[test]
fn run_missing_required_param_is_usage_error() {
    let staged = stage("params");
    rt().args(["run", "deploy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("usage:"));
}

#[test]
fn run_enum_violation_is_usage_error() {
    let staged = stage("params");
    rt().args(["run", "deploy", "dev"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("must be one of"));
}

#[test]
fn run_unknown_task_is_usage_error() {
    let staged = stage("basic");
    rt().args(["run", "nope"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2);
}

#[test]
fn run_task_exception_is_exit_1_without_sentinel() {
    let staged = stage("basic");
    let assert = rt()
        .args(["run", "boom"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("RuntimeError"))
        .stderr(predicates::str::contains("kaboom"));
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(!stderr.contains("__RT_ERROR__"), "sentinel must be hidden");
    assert!(
        !stderr.contains('\u{1e}'),
        "record separator must be hidden"
    );
}

#[test]
fn run_partial_stderr_before_exception_hides_sentinel() {
    let staged = stage("basic");
    let assert = rt()
        .args(["run", "boom_partial"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("boom-partial"));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("partial-no-newline"));
    assert!(!stderr.contains("__RT_ERROR__"), "sentinel must be hidden");
    assert!(
        !stderr.contains('\u{1e}'),
        "record separator must be hidden"
    );
}

#[test]
fn run_non_utf8_stderr_still_reports_exception() {
    let staged = stage("basic");
    let assert = rt()
        .args(["run", "boom_binary"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("boom-binary"));
    let bytes = assert.get_output().stderr.clone();
    assert!(
        !bytes.windows(2).any(|w| w == b"\x1e_"),
        "sentinel must be hidden"
    );
}

#[test]
fn run_scripterror_family_is_reported_via_sentinel() {
    let staged = stage("basic");
    let assert = rt()
        .args(["run", "boom_scripterror"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("NotImplementedError"))
        .stderr(predicates::str::contains("not yet"));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    // Harness internal frames must not leak.
    assert!(
        !stderr.contains("run_task"),
        "harness frames must be stripped"
    );
    assert!(!stderr.contains("harness-"), "harness path must not leak");
}

#[test]
fn run_task_with_early_return_on_dry_run() {
    let staged = stage("basic");
    rt().args(["run", "early_return", "--dry-run"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("starting"))
        .stdout(predicates::str::contains("did the work").not());
}

#[test]
fn successful_task_emitting_sentinel_line_stays_exit_zero() {
    let staged = stage("basic");
    let assert = rt()
        .args(["run", "fake_sentinel"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success();
    // The line must not be swallowed: it is re-emitted verbatim on stderr.
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("__RT_ERROR__"));
    assert!(stderr.contains("decoy"));
}

#[test]
fn task_file_exiting_at_load_does_not_kill_discovery() {
    let staged = stage("broken");
    let assert = rt()
        .arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("healthy"));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("exits_on_load.rb"));
}

#[cfg(unix)]
#[test]
fn broken_bundle_exec_falls_back_to_plain_ruby() {
    use std::os::unix::fs::PermissionsExt;

    let staged = stage("basic");
    std::fs::write(
        staged.path().join("Gemfile"),
        "source 'https://rubygems.org'\n",
    )
    .unwrap();

    // A fake `bundle` that reports a version (so it looks installed) but fails
    // any `exec`, standing in for missing/broken gems.
    let bindir = staged.path().join("fakebin");
    std::fs::create_dir_all(&bindir).unwrap();
    let bundle = bindir.join("bundle");
    std::fs::write(
        &bundle,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'Bundler version 2.0.0'; exit 0; fi\necho 'bundle exec is broken' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&bundle, std::fs::Permissions::from_mode(0o755)).unwrap();

    let path = format!(
        "{}:{}",
        bindir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let assert = rt()
        .arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env_remove("RT_RUBY")
        .env("PATH", &path)
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(stderr.contains("plain ruby"), "expected fallback warning");

    // In --json mode the same fallback must stay silent on stderr.
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env_remove("RT_RUBY")
        .env("PATH", &path)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stderr.is_empty(),
        "stderr must be clean in --json mode, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn run_task_custom_exit_code_is_passed_through() {
    let staged = stage("basic");
    rt().args(["run", "bail"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(3);
}

#[test]
fn help_shows_usage_params_and_options() {
    let staged = stage("params");
    rt().args(["help", "deploy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("rt run deploy"))
        .stdout(predicates::str::contains("environment"))
        .stdout(predicates::str::contains("workers"));
}

#[test]
fn help_json_emits_single_task() {
    let staged = stage("params");
    let out = rt()
        .args(["help", "deploy", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["protocol_version"], 2);
    assert_eq!(value["task"]["name"], "deploy");
    assert_eq!(value["task"]["params"][0]["name"], "environment");
}

#[test]
fn help_unknown_task_is_usage_error() {
    let staged = stage("basic");
    rt().args(["help", "nope"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("unknown task"));
}

#[test]
fn generated_gitignore_anchors_patterns_to_the_home() {
    let staged = stage("basic");
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success();
    let content = std::fs::read_to_string(staged.path().join(".rt/.gitignore")).unwrap();
    // Unanchored patterns would recursively ignore files under tasks/ too
    // (e.g. tasks/harness-deploy.rb).
    assert_eq!(content, "/cache.json\n/harness-*.rb\n/*.tmp\n");
}

#[test]
fn stale_wildcard_gitignore_is_rewritten() {
    let staged = stage("basic");
    // Older rt versions wrote `*` when the home held only generated files; left
    // alone it would keep .rt/tasks/ invisible to git.
    std::fs::write(staged.path().join(".rt/.gitignore"), "*\n").unwrap();
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success();
    let content = std::fs::read_to_string(staged.path().join(".rt/.gitignore")).unwrap();
    assert_eq!(content, "/cache.json\n/harness-*.rb\n/*.tmp\n");
}

#[test]
fn user_edited_gitignore_is_preserved() {
    let staged = stage("basic");
    std::fs::write(
        staged.path().join(".rt/.gitignore"),
        "/cache.json\n/my-scratch/\n",
    )
    .unwrap();
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success();
    let content = std::fs::read_to_string(staged.path().join(".rt/.gitignore")).unwrap();
    assert_eq!(content, "/cache.json\n/my-scratch/\n");
}

#[test]
fn second_list_hits_cache_without_launching_ruby() {
    let staged = stage("basic");
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env_remove("RT_RUBY")
        .assert()
        .success();
    assert!(staged.path().join(".rt/cache.json").is_file());

    // Empty PATH makes `ruby` unspawnable; success proves the cache was used.
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env_remove("RT_RUBY")
        .env("PATH", "")
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"));
}

#[test]
fn cache_hit_still_rewrites_stale_gitignore() {
    let staged = stage("basic");
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success();
    std::fs::write(staged.path().join(".rt/.gitignore"), "*\n").unwrap();

    // Empty PATH proves metadata comes from cache, while home maintenance must
    // still run before the early return.
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("PATH", "")
        .assert()
        .success();

    let content = std::fs::read_to_string(staged.path().join(".rt/.gitignore")).unwrap();
    assert_eq!(content, "/cache.json\n/harness-*.rb\n/*.tmp\n");
}

/// Assemble a self-contained offline gem source from the machine's cached rake
/// `.gem` files so `gemfile(true)` can resolve without any network. Returns
/// None when no rake gem is cached (then the caller skips).
fn local_rake_source() -> Option<(tempfile::TempDir, String)> {
    let listing = std::process::Command::new("ruby")
        .args([
            "-e",
            "Gem.path.each { |p| Dir.glob(File.join(p, 'cache', 'rake-*.gem')).each { |g| puts g } }",
        ])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&listing.stdout);
    let gems: Vec<String> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if gems.is_empty() {
        return None;
    }
    let dir = tempfile::tempdir().ok()?;
    let gems_dir = dir.path().join("gems");
    std::fs::create_dir_all(&gems_dir).ok()?;
    for g in &gems {
        let name = Path::new(g).file_name()?;
        std::fs::copy(g, gems_dir.join(name)).ok()?;
    }
    let status = std::process::Command::new("gem")
        .args(["generate_index", "-d"])
        .arg(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let url = format!("file://{}", dir.path().display());
    Some((dir, url))
}

/// Build a tiny pure-Ruby gem and serve it from a local, indexed directory so
/// `gemfile(true)` can install it with no network at all. Returns None when the
/// `gem` toolchain is unavailable (then the caller skips).
fn local_dummy_gem_source() -> Option<(tempfile::TempDir, String)> {
    let build = tempfile::tempdir().ok()?;
    std::fs::create_dir_all(build.path().join("lib")).ok()?;
    std::fs::write(
        build.path().join("lib/rt_dummy.rb"),
        "module RtDummy\n  VERSION = \"1.0.0\"\nend\n",
    )
    .ok()?;
    std::fs::write(
        build.path().join("rt_dummy.gemspec"),
        "Gem::Specification.new do |s|\n  s.name = \"rt_dummy\"\n  s.version = \"1.0.0\"\n  s.summary = \"rt e2e dummy gem\"\n  s.authors = [\"rt\"]\n  s.files = [\"lib/rt_dummy.rb\"]\nend\n",
    )
    .ok()?;
    let built = std::process::Command::new("gem")
        .args(["build", "rt_dummy.gemspec"])
        .current_dir(build.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !built.success() {
        return None;
    }

    let dir = tempfile::tempdir().ok()?;
    let gems_dir = dir.path().join("gems");
    std::fs::create_dir_all(&gems_dir).ok()?;
    for entry in std::fs::read_dir(build.path()).ok()? {
        let path = entry.ok()?.path();
        if path.extension().is_some_and(|e| e == "gem") {
            std::fs::copy(&path, gems_dir.join(path.file_name()?)).ok()?;
        }
    }
    let status = std::process::Command::new("gem")
        .args(["generate_index", "-d"])
        .arg(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let url = format!("file://{}", dir.path().display());
    Some((dir, url))
}

#[test]
fn gem_task_installs_into_isolated_home_offline() {
    let Some((_src, url)) = local_dummy_gem_source() else {
        eprintln!("skipping: cannot build a local dummy gem source");
        return;
    };
    let staged = stage("gems_dummy");
    let gem_home = tempfile::tempdir().unwrap();

    rt().args(["run", "use_dummy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_SOURCE", &url)
        .env("RT_GEM_HOME", gem_home.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("dummy 1.0.0"));

    // The gem lands under a single <engine>-<abi> subdir, in its gems/ folder.
    let subdirs: Vec<PathBuf> = std::fs::read_dir(gem_home.path())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(subdirs.len(), 1, "one per-ABI subdir expected");
    let installed = std::fs::read_dir(subdirs[0].join("gems"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().starts_with("rt_dummy"));
    assert!(
        installed,
        "rt_dummy must be installed under the isolated gem home"
    );
}

#[test]
fn hostile_env_does_not_break_gem_task_or_discovery() {
    let Some((_src, url)) = local_dummy_gem_source() else {
        eprintln!("skipping: cannot build a local dummy gem source");
        return;
    };
    let staged = stage("gems_dummy");
    let gem_home = tempfile::tempdir().unwrap();

    // Discovery survives a poisoned RUBYOPT/RUBYLIB and a broken gem env.
    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RUBYOPT", "-rnonexistent")
        .env("RUBYLIB", "/nonexistent/lib")
        .env("GEM_HOME", "/nonexistent/broken")
        .env("GEM_PATH", "/nonexistent/broken")
        .assert()
        .success()
        .stdout(predicates::str::contains("use_dummy"));

    // Running the gem task survives the same hostile environment: the isolated
    // path scrubs it all and the harness rebuilds a clean GEM_HOME.
    rt().args(["run", "use_dummy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_SOURCE", &url)
        .env("RT_GEM_HOME", gem_home.path())
        .env("RUBYOPT", "-rnonexistent")
        .env("RUBYLIB", "/nonexistent/lib")
        .env("GEM_HOME", "/nonexistent/broken")
        .env("GEM_PATH", "/nonexistent/broken")
        .env("BUNDLE_GEMFILE", "/nonexistent/Gemfile")
        .env("BUNDLE_PATH", "/nonexistent/bundle")
        // Variables a real `bundle exec` exports into its child, which must not
        // redirect the isolated install: this checks the README's "behaves the
        // same under bundle exec" claim against an actual bundle-shaped env.
        .env("BUNDLER_VERSION", "2.5.0")
        .env("BUNDLER_ORIG_GEM_HOME", "/nonexistent/orig-gem-home")
        .env("BUNDLER_SETUP", "/nonexistent/setup")
        .env("RUBYGEMS_GEMDEPS", "/nonexistent/Gemfile")
        .assert()
        .success()
        .stdout(predicates::str::contains("dummy 1.0.0"));
}

#[test]
fn parallel_gem_installs_share_home_without_corruption() {
    let Some((_src, url)) = local_dummy_gem_source() else {
        eprintln!("skipping: cannot build a local dummy gem source");
        return;
    };
    let staged = stage("gems_dummy");
    let gem_home = tempfile::tempdir().unwrap();
    let bin = assert_cmd::cargo::cargo_bin("rt");

    // Two processes race the first install of the same gem into one shared home.
    // The harness's exclusive lock must serialize them so both exit 0 rather than
    // colliding in rubygems' installer.
    let spawn = || {
        std::process::Command::new(&bin)
            .args(["run", "use_dummy"])
            .current_dir(staged.path())
            .env("RT_CONFIG_DIR", empty_config())
            .env_remove("RT_ROOT")
            .env("RT_GEM_SOURCE", &url)
            .env("RT_GEM_HOME", gem_home.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut a = spawn();
    let mut b = spawn();
    let ra = a.wait().unwrap();
    let rb = b.wait().unwrap();
    assert!(ra.success(), "first parallel install should exit 0");
    assert!(rb.success(), "second parallel install should exit 0");
}

#[test]
fn gem_task_lists_gems_in_json_and_help() {
    let staged = stage("gems");
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let task = value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "with_rake")
        .unwrap();
    assert_eq!(task["gems"][0]["name"], "rake");

    rt().args(["help", "with_rake"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("Gems: rake"));
}

#[test]
fn gem_task_runs_with_offline_local_source() {
    let Some((_src, url)) = local_rake_source() else {
        eprintln!("skipping: no cached rake gem to build an offline source");
        return;
    };
    let staged = stage("gems");
    let gem_home = tempfile::tempdir().unwrap();
    rt().args(["run", "with_rake"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_SOURCE", &url)
        .env("RT_GEM_HOME", gem_home.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("rake"));
}

#[test]
fn top_level_require_of_declared_gem_gets_a_hint() {
    let staged = stage("gems_toplevel");
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let errors = value["errors"].as_array().unwrap();
    assert!(errors.iter().any(|e| {
        e["class"] == "LoadError"
            && e["message"]
                .as_str()
                .unwrap_or("")
                .contains("inside the task block")
    }));
}

#[test]
#[ignore = "installs a real gem from rubygems.org; run with `cargo test -- --ignored`"]
fn gem_task_installs_a_real_gem() {
    let staged = stage("gems_real");
    // A throwaway isolated gem home: the run needs no sudo and leaves no trace.
    let gem_home = tempfile::tempdir().unwrap();
    rt().args(["run", "paint_demo"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_HOME", gem_home.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("colored"));
}

#[test]
fn empty_gem_source_falls_back_to_default() {
    // An empty RT_GEM_SOURCE must be treated as unset. Before the fix, a blank
    // value reached bundler as `source("")` and failed with "must be an absolute
    // URI"; after the fix it falls back to the default rubygems.org source. This
    // runs offline, so the fallback may fail to fetch — but never with the blank
    // -source symptom, which is the regression we guard against.
    let staged = stage("gems");
    let gem_home = tempfile::tempdir().unwrap();
    let assert = rt()
        .args(["run", "with_rake"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_SOURCE", "")
        .env("RT_GEM_HOME", gem_home.path())
        .assert();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert!(
        !stderr.contains("absolute URI") && !stderr.contains("must be an absolute"),
        "blank RT_GEM_SOURCE must not reach bundler as an empty source; stderr was:\n{stderr}"
    );
}

#[test]
fn gem_install_failure_is_environment_error() {
    let staged = stage("gems");
    let gem_home = tempfile::tempdir().unwrap();
    // A nonexistent gem plus an unreachable source: bundler cannot fetch specs,
    // so resolution fails and the harness exits 74 (environment error).
    rt().args(["run", "needs_missing"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_SOURCE", "http://127.0.0.1:1")
        .env("RT_GEM_HOME", gem_home.path())
        .assert()
        .failure()
        .code(74)
        .stderr(predicates::str::contains("resolve gems"));
}

#[test]
fn run_json_captures_gem_install_failure() {
    let staged = stage("gems");
    let gem_home = tempfile::tempdir().unwrap();
    let out = rt()
        .args(["run", "--json", "needs_missing"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_SOURCE", "http://127.0.0.1:1")
        .env("RT_GEM_HOME", gem_home.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "environment");
    assert!(value["stderr"]["data"]
        .as_str()
        .unwrap()
        .contains("resolve gems"));
}

#[test]
fn malformed_gem_requirement_is_environment_error() {
    // A bad version string fails resolution locally (no network needed) and
    // must still map to the deterministic environment exit code, not a task bug.
    let staged = stage("gems");
    let gem_home = tempfile::tempdir().unwrap();
    rt().args(["run", "bad_version"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RT_GEM_HOME", gem_home.path())
        .assert()
        .failure()
        .code(74)
        .stderr(predicates::str::contains("resolve gems"));
}

#[test]
fn global_tasks_list_and_run_outside_any_project() {
    let global = stage("global");
    let cwd = tempfile::tempdir().unwrap(); // no project here or above

    rt().arg("list")
        .current_dir(cwd.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("ggreet"));

    rt().args(["run", "ggreet"])
        .current_dir(cwd.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("hello from global"));

    // `rt help` names the source and, for a global task, its config dir path.
    rt().args(["help", "ggreet"])
        .current_dir(cwd.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Source: global ("));

    // The global home keeps its own cache/harness directly under <config_dir>.
    assert!(global.path().join("cache.json").is_file());
    let harness = std::fs::read_dir(global.path())
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().starts_with("harness-"));
    assert!(harness, "global harness should live under <config_dir>");
}

#[test]
fn project_and_global_tasks_list_in_two_sections() {
    let project = stage("basic");
    let global = stage("global");
    let assert = rt()
        .arg("list")
        .current_dir(project.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Project tasks:"))
        .stdout(predicates::str::contains("Global tasks:"))
        .stdout(predicates::str::contains("greet"))
        .stdout(predicates::str::contains("ggreet"));
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    // Project section precedes the global one.
    assert!(stdout.find("Project tasks:").unwrap() < stdout.find("Global tasks:").unwrap());
}

#[test]
fn project_task_shadows_global_of_same_name() {
    let project = stage("basic");
    let global = stage("global");

    // The project's greet wins the name collision.
    rt().args(["run", "greet", "--name", "sora"])
        .current_dir(project.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("Hello, sora!"))
        .stdout(predicates::str::contains("GLOBAL GREET").not());

    let out = rt()
        .args(["list", "--json"])
        .current_dir(project.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let tasks = value["tasks"].as_array().unwrap();

    // Names are unique: exactly one "greet", and it is the project one.
    let greets: Vec<&serde_json::Value> = tasks.iter().filter(|t| t["name"] == "greet").collect();
    assert_eq!(greets.len(), 1);
    assert_eq!(greets[0]["source"], "project");

    // The unique global task survives and is tagged global.
    let ggreet = tasks.iter().find(|t| t["name"] == "ggreet").unwrap();
    assert_eq!(ggreet["source"], "global");

    // The hidden global greet is reported as a ShadowedTask warning.
    let errors = value["errors"].as_array().unwrap();
    assert!(errors
        .iter()
        .any(|e| e["class"] == "ShadowedTask" && e["source"] == "global"));
}

#[test]
fn protocol_version_bump_invalidates_old_cache() {
    let staged = stage("basic");
    let rt_dir = staged.path().join(".rt");
    std::fs::create_dir_all(&rt_dir).unwrap();
    // A well-formed cache from an older protocol: it names a task that does not
    // exist. If honored, "ghost" would appear; a correct bump ignores it.
    let stale = serde_json::json!({
        "cache_version": 1,
        "ruby_command": "ruby",
        "files": {},
        "metadata": {
            "protocol_version": 1,
            "tasks": [{ "name": "ghost", "file": "tasks/ghost.rb" }],
            "errors": []
        }
    });
    std::fs::write(rt_dir.join("cache.json"), stale.to_string()).unwrap();

    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"))
        .stdout(predicates::str::contains("ghost").not());
}

#[test]
fn broken_task_json_puts_errors_in_json_not_stderr() {
    let staged = stage("broken");
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        out.stderr.is_empty(),
        "stderr should be clean in --json mode"
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!value["errors"].as_array().unwrap().is_empty());
}
