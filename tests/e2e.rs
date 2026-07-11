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
    assert_eq!(value["protocol_version"], 4);
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
    assert_eq!(value["schema_version"], 2);
    assert_eq!(value["task"], "greet");
    assert_eq!(value["status"], "success");
    assert_eq!(value["exit_code"], 0);
    assert_eq!(value["stdout"]["encoding"], "utf-8");
    assert_eq!(value["stdout"]["data"], "Hello, sora!\n");
    assert_eq!(value["stdout"]["total_bytes"], 13);
    assert_eq!(value["stdout"]["captured_bytes"], 13);
    assert_eq!(value["stdout"]["truncated"], false);
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
fn run_json_keeps_control_message_separate_and_encodes_binary_stderr() {
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
fn run_json_treats_sentinel_shaped_stderr_as_task_output() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "fake_sentinel_failure"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["exit_code"], 3);
    assert_eq!(value["error"]["kind"], "task_exit");
    assert!(value["stderr"]["data"]
        .as_str()
        .unwrap()
        .contains("__RT_ERROR__"));
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
    assert_eq!(value["stdout"]["total_bytes"], 2);
    assert_eq!(value["stdout"]["captured_bytes"], 2);
    assert_eq!(value["stdout"]["truncated"], false);
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
    assert_eq!(value["stdout"]["total_bytes"], 131_072);
    assert_eq!(value["stderr"]["total_bytes"], 131_072);
    assert_eq!(value["stdout"]["truncated"], false);
    assert_eq!(value["stderr"]["truncated"], false);
}

#[test]
fn run_json_keeps_output_at_the_capture_limit() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "capture_boundary"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let stdout = &value["stdout"];
    assert_eq!(stdout["data"].as_str().unwrap().len(), 1_048_576);
    assert_eq!(stdout["total_bytes"], 1_048_576);
    assert_eq!(stdout["captured_bytes"], 1_048_576);
    assert_eq!(stdout["truncated"], false);
}

#[test]
fn run_json_bounds_both_streams_and_drains_to_eof() {
    let staged = stage("basic");
    let out = rt()
        .args(["run", "--json", "capture_overflow"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for stream in ["stdout", "stderr"] {
        let output = &value[stream];
        assert_eq!(output["data"].as_str().unwrap().len(), 1_048_576);
        assert_eq!(output["total_bytes"], 1_048_577);
        assert_eq!(output["captured_bytes"], 1_048_576);
        assert_eq!(output["truncated"], true);
    }
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
fn run_partial_stderr_before_exception_is_preserved() {
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
    assert!(!stderr.contains("__RT_ERROR__"));
    assert!(!stderr.contains('\u{1e}'));
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
        .code(1);
    let bytes = assert.get_output().stderr.clone();
    assert!(bytes.starts_with(b"\xff\xfe"));
    assert!(String::from_utf8_lossy(&bytes).contains("boom-binary"));
    assert!(!bytes.windows(2).any(|w| w == b"\x1e_"));
}

#[test]
fn run_scripterror_family_is_reported_via_control_channel() {
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
fn failed_task_emitting_sentinel_line_preserves_exit_and_stderr() {
    let staged = stage("basic");
    let assert = rt()
        .args(["run", "fake_sentinel_failure"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(3);
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
fn broken_project_bundle_is_ignored_by_list_and_falls_back_during_run() {
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

    rt().arg("list")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env_remove("RT_RUBY")
        .env("PATH", &path)
        .assert()
        .success()
        .stdout(predicates::str::contains("greet"))
        .stderr("");

    let assert = rt()
        .args(["run", "greet"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env_remove("RT_RUBY")
        .env("PATH", &path)
        .assert()
        .success()
        .stdout("Hello, world!\n");
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
        .stdout(predicates::str::contains("workers"))
        .stdout(predicates::str::contains("[range: 1..16]"));
}

#[test]
fn run_rejects_integer_option_outside_declared_range() {
    let staged = stage("params");
    rt().args(["run", "deploy", "staging", "--workers", "17"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("--workers must be within 1..16"));
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
    assert_eq!(
        value,
        serde_json::json!({
            "protocol_version": 4,
            "task": {
                "name": "deploy",
                "description": "Deploy the application to an environment",
                "file": "tasks/deploy.rb",
                "params": [{
                    "name": "environment",
                    "required": true,
                    "default": null,
                    "enum": ["staging", "production"],
                    "description": "target environment"
                }],
                "options": [
                    {
                        "name": "workers",
                        "type": "integer",
                        "default": 2,
                        "minimum": 1,
                        "maximum": 16,
                        "description": "worker count"
                    },
                    {
                        "name": "force",
                        "type": "boolean",
                        "default": false,
                        "description": "skip safety checks"
                    }
                ],
                "gems": [],
                "requirements": [],
                "source": "project"
            }
        })
    );
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
fn tools_json_emits_catalog_and_filters_one_task() {
    let staged = stage("params");
    let all = rt()
        .args(["tools", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(all.status.success());
    assert!(all.stderr.is_empty());

    let catalog: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(catalog["schema_version"], 3);
    assert_eq!(catalog["tools"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["tools"][0]["task"], "deploy");
    assert_eq!(
        catalog["tools"][0]["input_schema"]["properties"]["workers"]["minimum"],
        1
    );
    assert_eq!(
        catalog["tools"][0]["input_schema"]["properties"]["workers"]["maximum"],
        16
    );
    assert_eq!(
        catalog["tools"][0]["input_schema"]["properties"]["environment"]["enum"],
        serde_json::json!(["staging", "production"])
    );
    assert_eq!(
        catalog["tools"][0]["input_schema"]["required"],
        serde_json::json!(["environment"])
    );
    assert_eq!(
        catalog["tools"][0]["input_schema"]["additionalProperties"],
        false
    );
    assert!(catalog["errors"].as_array().unwrap().is_empty());

    let filtered = rt()
        .args(["tools", "--json", "deploy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(filtered.status.success());
    assert!(filtered.stderr.is_empty());
    let catalog: serde_json::Value = serde_json::from_slice(&filtered.stdout).unwrap();
    assert_eq!(catalog["schema_version"], 3);
    assert_eq!(catalog["tools"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["tools"][0]["task"], "deploy");
}

#[test]
fn tools_json_unknown_task_is_usage_error() {
    let staged = stage("basic");
    rt().args(["tools", "--json", "nope"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stdout(predicates::str::is_empty())
        .stderr(predicates::str::contains("unknown task"));
}

#[test]
fn tools_requires_json_flag() {
    let staged = stage("basic");
    rt().arg("tools")
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("--json"));
}

#[test]
fn tools_json_merges_project_and_global_tasks() {
    let project = stage("basic");
    let global = stage("global");
    let out = rt()
        .args(["tools", "--json"])
        .current_dir(project.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let catalog: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let tools = catalog["tools"].as_array().unwrap();
    let greets: Vec<_> = tools
        .iter()
        .filter(|tool| tool["task"] == "greet")
        .collect();
    assert_eq!(greets.len(), 1);
    assert_eq!(greets[0]["source"], "project");
    assert!(tools.iter().any(|tool| tool["task"] == "ggreet"));
    assert!(catalog["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|error| error["class"] == "ShadowedTask"));
}

#[test]
fn tools_json_includes_load_errors_without_stderr() {
    let staged = stage("broken");
    let out = rt()
        .args(["tools", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let catalog: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!catalog["tools"].as_array().unwrap().is_empty());
    assert!(!catalog["errors"].as_array().unwrap().is_empty());
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
fn old_cache_format_is_rejected() {
    let staged = tempfile::tempdir().unwrap();
    let rt_dir = staged.path().join(".rt");
    std::fs::create_dir_all(&rt_dir).unwrap();
    // This is a valid cache from v0.0.4. Empty PATH proves rt rejects it and
    // attempts fresh discovery rather than returning the cached ghost task.
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
        .env("PATH", "")
        .assert()
        .failure()
        .code(74);
}

#[test]
fn public_schema_version_is_refreshed_without_ruby() {
    let staged = tempfile::tempdir().unwrap();
    let rt_dir = staged.path().join(".rt");
    std::fs::create_dir_all(&rt_dir).unwrap();
    let cache = serde_json::json!({
        "cache_format_version": 3,
        "harness_protocol_version": 4,
        "ruby_command": "ruby [unbundled]",
        "files": {},
        "metadata": {
            "protocol_version": 1,
            "tasks": [{ "name": "ghost", "file": "tasks/ghost.rb" }],
            "errors": []
        }
    });
    std::fs::write(rt_dir.join("cache.json"), cache.to_string()).unwrap();

    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["protocol_version"], 4);
    assert_eq!(value["tasks"][0]["name"], "ghost");
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

#[test]
fn invalid_declarations_are_structured_load_errors() {
    let staged = stage("invalid");
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let tasks = value["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["name"], "healthy");

    let errors = value["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 15);
    assert!(errors
        .iter()
        .all(|error| error["class"] == "InvalidDeclaration"));
    let messages: Vec<&str> = errors
        .iter()
        .map(|error| error["message"].as_str().unwrap())
        .collect();
    for expected in [
        "duplicate param name",
        "duplicate option name",
        "used as both a param and an option",
        "reserved by rt",
        "unknown option type",
        "default must be an integer",
        "default must be a boolean",
        "default must be a string",
        "default must be one of",
        "required must be true or false",
        "cannot have a default",
        "range is only supported for integer options",
        "range must be an inclusive integer range",
        "default must be within",
        "run block is required",
    ] {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "missing declaration error containing {expected:?}: {messages:?}"
        );
    }

    rt().args(["run", "healthy"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout("ok\n");
}

#[test]
fn invalid_task_requirements_are_structured_load_errors() {
    let staged = stage("invalid_requirements");
    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(value["tasks"].as_array().unwrap().is_empty());
    let errors = value["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 3);
    let messages: Vec<&str> = errors
        .iter()
        .map(|error| error["message"].as_str().unwrap())
        .collect();
    for expected in [
        "unknown requirement",
        "duplicate requirement",
        "cannot declare inline gems",
    ] {
        assert!(messages.iter().any(|message| message.contains(expected)));
    }
}

#[test]
fn global_task_cannot_require_rails() {
    let global = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(global.path().join("tasks")).unwrap();
    std::fs::write(
        global.path().join("tasks/rails.rb"),
        "task(\"global_rails\") { |t| t.requires :rails; t.run { |_ctx| } }\n",
    )
    .unwrap();
    let cwd = tempfile::tempdir().unwrap();

    let out = rt()
        .args(["list", "--json"])
        .current_dir(cwd.path())
        .env_remove("RT_ROOT")
        .env("RT_CONFIG_DIR", global.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(value["tasks"].as_array().unwrap().is_empty());
    assert_eq!(value["errors"][0]["class"], "InvalidDeclaration");
    assert!(value["errors"][0]["message"]
        .as_str()
        .unwrap()
        .contains("global tasks cannot require Rails"));
}

#[test]
fn rails_discovery_exposes_metadata_without_booting_the_application() {
    let staged = stage("rails");
    let marker = staged.path().join("booted.txt");

    for args in [
        vec!["list", "--json"],
        vec!["help", "rails:probe", "--json"],
        vec!["tools", "--json", "rails:probe"],
    ] {
        let out = rt()
            .args(args)
            .current_dir(staged.path())
            .env_remove("RT_ROOT")
            .env("RAILS_BOOT_MARKER", &marker)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert!(!marker.exists(), "discovery must not boot Rails");
    }

    let out = rt()
        .args(["list", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let task = value["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["name"] == "rails:probe")
        .unwrap();
    assert_eq!(task["requirements"], serde_json::json!(["rails"]));

    let out = rt()
        .args(["tools", "--json", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["schema_version"], 3);
    assert_eq!(
        value["tools"][0]["requirements"],
        serde_json::json!(["rails"])
    );
}

#[test]
fn rails_metadata_commands_ignore_an_incomplete_application_bundle() {
    let staged = stage("rails");
    std::fs::write(
        staged.path().join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"missing_local_gem\", path: \"vendor/missing\"\n",
    )
    .unwrap();

    for args in [
        vec!["list", "--json"],
        vec!["help", "rails:probe", "--json"],
        vec!["tools", "--json", "rails:probe"],
    ] {
        let out = rt()
            .args(args)
            .current_dir(staged.path())
            .env_remove("RT_ROOT")
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(
            out.stderr.is_empty(),
            "metadata discovery must not inspect the application bundle: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).contains("rails:probe"));
    }
}

#[test]
fn rails_task_help_shows_its_requirement() {
    let staged = stage("rails");
    rt().args(["help", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout(predicates::str::contains("Requires: rails"));
}

#[test]
fn rails_task_boots_once_and_receives_environment_root_and_working_directory() {
    let staged = stage("rails");
    let marker = staged.path().join("booted.txt");
    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(staged.path().join("config"))
        .env_remove("RT_ROOT")
        .env("RAILS_ENV", "test")
        .env("RAILS_BOOT_MARKER", &marker)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "success");
    let stdout = value["stdout"]["data"].as_str().unwrap();
    assert!(stdout.contains("env=test\n"));
    assert!(stdout.contains("users=2\n"));

    let root = stdout
        .lines()
        .find_map(|line| line.strip_prefix("root="))
        .unwrap();
    let cwd = stdout
        .lines()
        .find_map(|line| line.strip_prefix("cwd="))
        .unwrap();
    assert_eq!(
        Path::new(root).canonicalize().unwrap(),
        staged.path().canonicalize().unwrap()
    );
    assert_eq!(
        Path::new(cwd).canonicalize().unwrap(),
        staged.path().canonicalize().unwrap()
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "boot\n");
}

#[test]
fn rails_dry_run_boots_but_task_can_skip_mutation() {
    let staged = stage("rails");
    let marker = staged.path().join("booted.txt");
    rt().args(["run", "rails:probe", "--write", "--dry-run"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RAILS_BOOT_MARKER", &marker)
        .assert()
        .success();

    assert_eq!(std::fs::read_to_string(marker).unwrap(), "boot\n");
    assert!(!staged.path().join("mutation.txt").exists());
}

#[test]
fn project_root_is_available_to_non_rails_project_tasks() {
    let staged = stage("rails");
    let out = rt()
        .args(["run", "root:show"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(out.status.success());
    let root = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        Path::new(root.trim()).canonicalize().unwrap(),
        staged.path().canonicalize().unwrap()
    );
}

#[test]
fn rails_boot_failure_is_a_structured_environment_error() {
    let staged = stage("rails");
    std::fs::write(
        staged.path().join("config/environment.rb"),
        "raise \"boot exploded\"\n",
    )
    .unwrap();

    rt().args(["run", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(74)
        .stderr(
            predicates::str::contains("RuntimeError: boot exploded")
                .and(predicates::str::contains("environment.rb")),
        );

    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    assert!(out.stderr.is_empty());

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "environment");
    assert_eq!(value["error"]["class"], "RuntimeError");
    assert_eq!(value["error"]["message"], "boot exploded");
    assert!(!value["error"]["backtrace"].as_array().unwrap().is_empty());
}

#[test]
fn missing_rails_environment_is_a_structured_environment_error() {
    let staged = stage("rails");
    std::fs::remove_file(staged.path().join("config/environment.rb")).unwrap();

    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "environment");
    assert_eq!(value["error"]["class"], "LoadError");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Rails environment not found"));
}

#[test]
fn rails_task_requires_project_gemfile() {
    let staged = stage("rails");
    std::fs::remove_file(staged.path().join("Gemfile")).unwrap();

    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "environment");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("requires a project Gemfile"));
}

#[test]
fn rails_task_requires_bundler() {
    let staged = stage("rails");
    let bin = tempfile::tempdir().unwrap();
    let ruby = std::process::Command::new("ruby")
        .args(["-rrbconfig", "-e", "print RbConfig.ruby"])
        .output()
        .unwrap();
    assert!(ruby.status.success());
    let ruby = String::from_utf8(ruby.stdout).unwrap();
    std::os::unix::fs::symlink(ruby, bin.path().join("ruby")).unwrap();

    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("PATH", bin.path())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "environment");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("requires Bundler"));
}

#[test]
fn rails_task_rejects_an_incomplete_bundle() {
    let staged = stage("rails");
    std::fs::write(
        staged.path().join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"missing_local_gem\", path: \"vendor/missing\"\n",
    )
    .unwrap();

    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(74));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["error"]["kind"], "environment");
    assert!(value["error"]["message"]
        .as_str()
        .unwrap()
        .contains("bundle is incomplete"));
}

#[test]
fn rails_task_accepts_relative_rt_root() {
    let staged = stage("rails");
    let parent = staged.path().parent().unwrap();
    let relative = staged.path().file_name().unwrap();

    let out = rt()
        .args(["run", "--json", "rails:probe"])
        .current_dir(parent)
        .env("RT_ROOT", relative)
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "success");
    let root = value["stdout"]["data"]
        .as_str()
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("root="))
        .unwrap();
    assert_eq!(
        Path::new(root).canonicalize().unwrap(),
        staged.path().canonicalize().unwrap()
    );
}

#[test]
fn rails_task_does_not_reuse_an_outer_bundle_lockfile() {
    let staged = stage("rails");
    let outer = tempfile::tempdir().unwrap();
    let lockfile = outer.path().join("Gemfile.lock");
    std::fs::write(&lockfile, "outer lockfile must remain unchanged\n").unwrap();

    rt().args(["run", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("BUNDLE_LOCKFILE", &lockfile)
        .env("BUNDLE_BIN_PATH", "/outer/bundle")
        .env("BUNDLER_SETUP", "/outer/bundler/setup")
        .assert()
        .success();

    assert_eq!(
        std::fs::read_to_string(lockfile).unwrap(),
        "outer lockfile must remain unchanged\n"
    );
}

#[test]
fn rails_bundle_probe_ignores_hostile_ruby_environment() {
    let staged = stage("rails");
    rt().args(["run", "rails:probe"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RUBYOPT", "-r/rt_missing_probe")
        .env("RUBYLIB", "/rt/missing/lib")
        .assert()
        .success();
}

#[test]
fn rails_arguments_are_validated_in_the_application_bundle() {
    let staged = stage("rails");
    std::fs::write(
        staged.path().join(".rt/Gemfile"),
        "source \"https://rubygems.org\"\n",
    )
    .unwrap();
    std::fs::write(
        staged.path().join(".rt/tasks/runtime.rb"),
        r#"
task "rails:runtime" do |t|
  t.desc "Validate under the Rails application bundle"
  if ENV.fetch("BUNDLE_GEMFILE", "").end_with?("/.rt/Gemfile")
    t.param :name
  else
    t.param :name, required: true
  end
  t.requires :rails
  t.run do |ctx|
    ctx.say ctx.param(:name)
  end
end
"#,
    )
    .unwrap();

    let list = rt()
        .args(["help", "rails:runtime", "--json"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .output()
        .unwrap();
    assert!(list.status.success());
    let metadata: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert_eq!(metadata["task"]["params"][0]["required"], false);

    rt().args(["run", "rails:runtime"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .failure()
        .code(2)
        .stderr(predicates::str::contains("missing required argument"));

    rt().args(["run", "rails:runtime", "sora"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout("sora\n");
}

#[cfg(target_os = "linux")]
#[test]
fn project_task_runs_from_a_non_utf8_path() {
    use std::os::unix::ffi::OsStringExt;

    let parent = tempfile::tempdir().unwrap();
    let project = parent
        .path()
        .join(std::ffi::OsString::from_vec(b"rt-\xff-project".to_vec()));
    std::fs::create_dir(&project).unwrap();
    copy_dir(&fixtures_dir().join("basic"), &project);

    rt().args(["run", "greet"])
        .current_dir(&project)
        .env_remove("RT_ROOT")
        .assert()
        .success()
        .stdout("Hello, world!\n");
}

#[test]
#[ignore = "requires the Rails integration bundle; CI runs this in a dedicated job"]
fn real_rails_application_uses_active_record() {
    let staged = stage("rails_real");
    let out = rt()
        .args(["run", "--json", "users:smoke"])
        .current_dir(staged.path())
        .env_remove("RT_ROOT")
        .env("RAILS_ENV", "test")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "real Rails task failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["status"], "success");
    assert!(value["stdout"]["data"]
        .as_str()
        .unwrap()
        .contains("rails=8.1.3 env=test users=1"));
}
