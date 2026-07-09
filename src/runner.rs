use crate::args;
use crate::cache;
use crate::error::{RtError, TaskFailure};
use crate::metadata::{LoadError, Metadata, Source, METADATA_SCHEMA_VERSION};
use crate::output;
use crate::project::Roots;
use crate::ruby::{self, RubyCommand};
use crate::run_result::RunResult;
use serde_json::json;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::Stdio;

const ERROR_SENTINEL: &[u8] = b"\x1e__RT_ERROR__";

/// Metadata merged across roots, plus the interpreter that produced each root's
/// metadata (needed to run a task with the same Ruby that discovered it).
struct Loaded {
    meta: Metadata,
    project: Option<(PathBuf, RubyCommand)>,
    global: Option<(PathBuf, RubyCommand)>,
}

fn load_all(roots: &Roots, warn: bool) -> Result<Loaded, RtError> {
    let mut project_meta = None;
    let mut project = None;
    if let Some(root) = &roots.project {
        let ruby = RubyCommand::resolve(root, Source::Project, warn);
        let (meta, used) = cache::load(root, &ruby, warn)?;
        project_meta = Some(meta);
        project = Some((root.clone(), used));
    }

    let mut global_meta = None;
    let mut global = None;
    if let Some(root) = &roots.global {
        let ruby = RubyCommand::resolve(root, Source::Global, warn);
        let (meta, used) = cache::load(root, &ruby, warn)?;
        global_meta = Some(meta);
        global = Some((root.clone(), used));
    }

    Ok(Loaded {
        meta: merge(project_meta, global_meta),
        project,
        global,
    })
}

/// Merge project and global metadata into a single name-unique task list.
/// Project wins name collisions; the hidden global task is dropped and recorded
/// as a `ShadowedTask` warning so the JSON task list stays unique.
fn merge(project: Option<Metadata>, global: Option<Metadata>) -> Metadata {
    let mut tasks = Vec::new();
    let mut errors = Vec::new();
    let mut names: HashSet<String> = HashSet::new();

    if let Some(m) = project {
        for mut t in m.tasks {
            t.source = Source::Project;
            names.insert(t.name.clone());
            tasks.push(t);
        }
        for mut e in m.errors {
            e.source = Source::Project;
            errors.push(e);
        }
    }

    if let Some(m) = global {
        for mut t in m.tasks {
            t.source = Source::Global;
            if names.contains(&t.name) {
                errors.push(LoadError {
                    file: t.file.clone(),
                    class: "ShadowedTask".to_string(),
                    message: format!(
                        "global task {:?} is hidden by a project task of the same name",
                        t.name
                    ),
                    source: Source::Global,
                });
            } else {
                names.insert(t.name.clone());
                tasks.push(t);
            }
        }
        for mut e in m.errors {
            e.source = Source::Global;
            errors.push(e);
        }
    }

    Metadata {
        schema_version: METADATA_SCHEMA_VERSION,
        tasks,
        errors,
    }
}

pub fn list(roots: &Roots, json: bool) -> Result<(), RtError> {
    // In --json mode, resolution warnings must not touch stderr.
    let loaded = load_all(roots, !json)?;
    if json {
        let text = serde_json::to_string_pretty(&loaded.meta)
            .map_err(|e| RtError::Internal(format!("cannot serialize metadata: {e}")))?;
        println!("{text}");
    } else {
        output::warn_load_errors(&loaded.meta);
        output::print_list(&loaded.meta);
    }
    Ok(())
}

pub fn help(roots: &Roots, task: &str, json: bool) -> Result<(), RtError> {
    let loaded = load_all(roots, !json)?;
    let found = loaded
        .meta
        .find_task(task)
        .ok_or_else(|| unknown_task(&loaded.meta, task))?;
    if json {
        let payload = json!({ "protocol_version": loaded.meta.schema_version, "task": found });
        let text = serde_json::to_string_pretty(&payload)
            .map_err(|e| RtError::Internal(format!("cannot serialize task: {e}")))?;
        println!("{text}");
    } else {
        // Show where a global task lives so `Source: global (<path>)` is actionable.
        let source_path = match found.source {
            Source::Global => loaded.global.as_ref().map(|(root, _)| root.as_path()),
            Source::Project => None,
        };
        output::print_help(found, source_path);
    }
    Ok(())
}

fn unknown_task(meta: &Metadata, task: &str) -> RtError {
    let mut names: Vec<&str> = meta.tasks.iter().map(|t| t.name.as_str()).collect();
    names.sort_unstable();
    RtError::Usage(format!(
        "unknown task {task:?}. Available tasks: {}",
        names.join(", ")
    ))
}

struct PreparedRun {
    root: PathBuf,
    ruby: RubyCommand,
    input: Vec<u8>,
    load_errors: Vec<LoadError>,
}

struct PrepareError {
    error: RtError,
    load_errors: Vec<LoadError>,
}

fn prepare_run(
    roots: &Roots,
    task_name: &str,
    raw_args: &[String],
    warn: bool,
) -> Result<PreparedRun, PrepareError> {
    let loaded = load_all(roots, warn).map_err(|error| PrepareError {
        error,
        load_errors: Vec::new(),
    })?;
    let load_errors = loaded.meta.errors.clone();
    // Clone to end the borrow on loaded.meta before moving a root out of loaded.
    let task = match loaded.meta.find_task(task_name) {
        Some(t) => t.clone(),
        None => {
            return Err(PrepareError {
                error: unknown_task(&loaded.meta, task_name),
                load_errors,
            })
        }
    };

    // A task runs against the root it was discovered in, with the interpreter
    // that produced its metadata (which may differ per root).
    let (root, ruby) = match task.source {
        Source::Project => loaded.project,
        Source::Global => loaded.global,
    }
    .ok_or_else(|| PrepareError {
        error: RtError::Internal("resolved task has no backing root".to_string()),
        load_errors: load_errors.clone(),
    })?;
    let parsed = args::parse(&task, raw_args).map_err(|error| PrepareError {
        error,
        load_errors: load_errors.clone(),
    })?;

    let input = json!({
        "task": task_name,
        "params": parsed.params,
        "options": parsed.options,
        "dry_run": parsed.dry_run,
    });
    let input = serde_json::to_vec(&input).map_err(|e| PrepareError {
        error: RtError::Internal(format!("cannot serialize task args: {e}")),
        load_errors: load_errors.clone(),
    })?;

    // A task declaring inline gems must run self-contained: bundler/inline
    // fights an active `bundle exec`, so drop to isolated plain Ruby.
    let ruby = if task.gems.is_empty() {
        ruby
    } else {
        RubyCommand::plain_isolated()
    };

    Ok(PreparedRun {
        root,
        ruby,
        input,
        load_errors,
    })
}

pub fn run(roots: &Roots, task_name: &str, raw_args: &[String]) -> Result<(), RtError> {
    let prepared = prepare_run(roots, task_name, raw_args, true).map_err(|error| error.error)?;
    let root = prepared.root.as_path();
    let ruby = prepared.ruby;

    let harness = ruby::ensure_harness(root)?;
    let mut child = ruby
        .command(&harness)
        .arg("--run")
        .arg(root)
        .arg(task_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ruby::environment_error(&ruby, &e))?;

    let stderr = child.stderr.take().expect("stderr piped");
    let tee = std::thread::spawn(move || tee_stderr(stderr));

    if let Some(mut stdin) = child.stdin.take() {
        // A BrokenPipe here just means the task never read stdin.
        let _ = stdin.write_all(&prepared.input);
    }

    let status = child
        .wait()
        .map_err(|e| RtError::Internal(format!("failed to wait for task: {e}")))?;
    let captured = tee.join().unwrap_or(None);
    let succeeded = status.code() == Some(0);

    // Only treat the sentinel as a real failure when the task actually failed.
    // A task that exits 0 while happening to print a sentinel-shaped line is a
    // success; re-emit the withheld line so nothing is silently swallowed.
    if let Some(cap) = captured {
        if !succeeded {
            if let Some(failure) = cap.failure {
                return Err(RtError::Task(failure));
            }
        }
        let mut err = std::io::stderr();
        let _ = err.write_all(&cap.raw);
    }

    match status.code() {
        Some(0) => Ok(()),
        Some(n) => Err(RtError::TaskExit(n)),
        None => Err(RtError::Internal("task terminated by signal".to_string())),
    }
}

pub fn run_json(roots: &Roots, task_name: &str, raw_args: &[String]) -> RunResult {
    let prepared = match prepare_run(roots, task_name, raw_args, false) {
        Ok(prepared) => prepared,
        Err(error) => {
            return RunResult::error(
                task_name,
                error.error,
                Vec::new(),
                Vec::new(),
                error.load_errors,
            )
        }
    };
    let load_errors = prepared.load_errors.clone();

    match run_captured(prepared, task_name) {
        Ok(captured) => {
            let CapturedRun {
                code,
                stdout,
                mut stderr,
                captured_error,
            } = captured;

            match code {
                Some(0) => {
                    if let Some(error) = captured_error {
                        stderr.extend(error.raw);
                    }
                    RunResult::success(task_name, stdout, stderr, load_errors)
                }
                Some(code) => {
                    let error = match captured_error {
                        Some(Captured {
                            failure: Some(failure),
                            ..
                        }) => RtError::Task(failure),
                        Some(Captured { failure: None, raw }) => {
                            stderr.extend(raw);
                            RtError::TaskExit(code)
                        }
                        None => RtError::TaskExit(code),
                    };
                    RunResult::error(task_name, error, stdout, stderr, load_errors)
                }
                None => RunResult::error(
                    task_name,
                    RtError::Internal("task terminated by signal".to_string()),
                    stdout,
                    stderr,
                    load_errors,
                ),
            }
        }
        Err(error) => RunResult::error(task_name, error, Vec::new(), Vec::new(), load_errors),
    }
}

struct CapturedRun {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    captured_error: Option<Captured>,
}

fn run_captured(prepared: PreparedRun, task_name: &str) -> Result<CapturedRun, RtError> {
    let root = prepared.root.as_path();
    let ruby = prepared.ruby;
    let harness = ruby::ensure_harness(root)?;
    let mut child = ruby
        .command(&harness)
        .arg("--run")
        .arg(root)
        .arg(task_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ruby::environment_error(&ruby, &e))?;

    let mut stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let stdout_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let stderr_reader = std::thread::spawn(move || capture_stderr(stderr, false));

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&prepared.input);
    }

    let status = child
        .wait()
        .map_err(|e| RtError::Internal(format!("failed to wait for task: {e}")))?;
    let stdout = stdout_reader
        .join()
        .map_err(|_| RtError::Internal("stdout reader panicked".to_string()))?
        .map_err(|e| RtError::Internal(format!("failed to read task stdout: {e}")))?;
    let captured = stderr_reader
        .join()
        .map_err(|_| RtError::Internal("stderr reader panicked".to_string()))?
        .map_err(|e| RtError::Internal(format!("failed to read task stderr: {e}")))?;

    Ok(CapturedRun {
        code: status.code(),
        stdout,
        stderr: captured.visible,
        captured_error: captured.error,
    })
}

/// A withheld sentinel line: the parsed failure (if the payload was valid JSON)
/// plus the raw bytes from the sentinel marker onward, so the caller can decide
/// whether to surface it as an error or re-emit it verbatim.
struct Captured {
    failure: Option<TaskFailure>,
    raw: Vec<u8>,
}

struct CapturedStderr {
    visible: Vec<u8>,
    error: Option<Captured>,
}

/// Stream task stderr while withholding the sentinel payload. Parsing stays on
/// raw bytes so non-UTF-8 output neither aborts the tee nor leaks the sentinel;
/// human mode keeps its existing lossy display behavior.
fn tee_stderr<R: Read>(reader: R) -> Option<Captured> {
    capture_stderr(reader, true)
        .map(|captured| captured.error)
        .unwrap_or(None)
}

fn capture_stderr<R: Read>(reader: R, passthrough: bool) -> std::io::Result<CapturedStderr> {
    let mut reader = BufReader::new(reader);
    let mut visible = Vec::new();
    let mut error = None;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break,
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        match find_subslice(&line, ERROR_SENTINEL) {
            Some(pos) => {
                let prefix = &line[..pos];
                if !prefix.is_empty() {
                    visible.extend_from_slice(prefix);
                    if passthrough {
                        eprint!("{}", String::from_utf8_lossy(prefix));
                    }
                }
                let raw = line[pos..].to_vec();
                let rest = &line[pos + ERROR_SENTINEL.len()..];
                let json = String::from_utf8_lossy(rest);
                error = Some(Captured {
                    failure: serde_json::from_str(json.trim()).ok(),
                    raw,
                });
            }
            None => {
                visible.extend_from_slice(&line);
                if passthrough {
                    eprint!("{}", String::from_utf8_lossy(&line));
                }
            }
        }
    }
    Ok(CapturedStderr { visible, error })
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
