use crate::args;
use crate::cache;
use crate::error::{RtError, TaskFailure};
use crate::metadata::{LoadError, Metadata, Source, METADATA_SCHEMA_VERSION};
use crate::output;
use crate::project::Roots;
use crate::ruby::{self, RubyCommand};
use crate::run_result::RunResult;
use command_fds::{CommandFdExt, FdMapping};
use os_pipe::PipeReader;
use serde_json::json;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const CONTROL_FD: i32 = 3;
const CONTROL_FD_ENV: &str = "RT_CONTROL_FD";

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
    let (mut command, control) = task_command(&ruby, &harness)?;
    let mut child = command
        .arg("--run")
        .arg(root)
        .arg(task_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| ruby::environment_error(&ruby, &e))?;
    drop(command);
    let control_reader = std::thread::spawn(move || read_all(control));

    if let Some(mut stdin) = child.stdin.take() {
        // A BrokenPipe here just means the task never read stdin.
        let _ = stdin.write_all(&prepared.input);
    }

    let status = child
        .wait()
        .map_err(|e| RtError::Internal(format!("failed to wait for task: {e}")))?;
    let failure = join_control(control_reader)?;

    if let Some(failure) = failure {
        return Err(RtError::Task(failure));
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
                stderr,
                failure,
            } = captured;

            match (code, failure) {
                (_, Some(failure)) => RunResult::error(
                    task_name,
                    RtError::Task(failure),
                    stdout,
                    stderr,
                    load_errors,
                ),
                (Some(0), None) => RunResult::success(task_name, stdout, stderr, load_errors),
                (Some(code), None) => RunResult::error(
                    task_name,
                    RtError::TaskExit(code),
                    stdout,
                    stderr,
                    load_errors,
                ),
                (None, None) => RunResult::error(
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
    failure: Option<TaskFailure>,
}

fn run_captured(prepared: PreparedRun, task_name: &str) -> Result<CapturedRun, RtError> {
    let root = prepared.root.as_path();
    let ruby = prepared.ruby;
    let harness = ruby::ensure_harness(root)?;
    let (mut command, control) = task_command(&ruby, &harness)?;
    let mut child = command
        .arg("--run")
        .arg(root)
        .arg(task_name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ruby::environment_error(&ruby, &e))?;
    drop(command);

    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stdout_reader = std::thread::spawn(move || read_all(&mut stdout));
    let stderr_reader = std::thread::spawn(move || read_all(&mut stderr));
    let control_reader = std::thread::spawn(move || read_all(control));

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
    let stderr = stderr_reader
        .join()
        .map_err(|_| RtError::Internal("stderr reader panicked".to_string()))?
        .map_err(|e| RtError::Internal(format!("failed to read task stderr: {e}")))?;
    let failure = join_control(control_reader)?;

    Ok(CapturedRun {
        code: status.code(),
        stdout,
        stderr,
        failure,
    })
}

fn task_command(
    ruby: &RubyCommand,
    harness: &std::path::Path,
) -> Result<(Command, PipeReader), RtError> {
    let (control_reader, control_writer) = os_pipe::pipe()
        .map_err(|e| RtError::Internal(format!("failed to create task control pipe: {e}")))?;
    let mut command = ruby.command(harness);
    command
        .fd_mappings(vec![FdMapping {
            parent_fd: control_writer.into(),
            child_fd: CONTROL_FD,
        }])
        .map_err(|e| RtError::Internal(format!("failed to map task control fd: {e}")))?;
    command.env(CONTROL_FD_ENV, CONTROL_FD.to_string());
    Ok((command, control_reader))
}

fn read_all<R: Read>(mut reader: R) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn join_control(
    reader: std::thread::JoinHandle<std::io::Result<Vec<u8>>>,
) -> Result<Option<TaskFailure>, RtError> {
    let bytes = reader
        .join()
        .map_err(|_| RtError::Internal("control reader panicked".to_string()))?
        .map_err(|e| RtError::Internal(format!("failed to read task control message: {e}")))?;
    if bytes.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| RtError::Internal(format!("invalid task control message: {e}")))
}
