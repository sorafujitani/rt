use crate::args;
use crate::cache;
use crate::error::RtError;
use crate::metadata::Metadata;
use crate::output;
use crate::ruby::{self, RubyCommand};
use serde::Deserialize;
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Stdio;

const ERROR_SENTINEL: &[u8] = b"\x1e__RT_ERROR__";

pub fn list(root: &Path, json: bool) -> Result<(), RtError> {
    // In --json mode, resolution warnings must not touch stderr.
    let ruby = RubyCommand::resolve(root, !json);
    let (meta, _) = cache::load(root, &ruby)?;
    if json {
        let text = serde_json::to_string_pretty(&meta)
            .map_err(|e| RtError::Internal(format!("cannot serialize metadata: {e}")))?;
        println!("{text}");
    } else {
        output::warn_load_errors(&meta);
        output::print_list(&meta);
    }
    Ok(())
}

pub fn help(root: &Path, task: &str, json: bool) -> Result<(), RtError> {
    let ruby = RubyCommand::resolve(root, !json);
    let (meta, _) = cache::load(root, &ruby)?;
    let found = meta
        .find_task(task)
        .ok_or_else(|| unknown_task(&meta, task))?;
    if json {
        let payload = json!({ "protocol_version": meta.protocol_version, "task": found });
        let text = serde_json::to_string_pretty(&payload)
            .map_err(|e| RtError::Internal(format!("cannot serialize task: {e}")))?;
        println!("{text}");
    } else {
        output::print_help(found);
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

#[derive(Debug, Deserialize)]
struct TaskFailure {
    class: String,
    message: String,
    #[serde(default)]
    backtrace: Vec<String>,
}

pub fn run(root: &Path, task_name: &str, raw_args: &[String]) -> Result<(), RtError> {
    let resolved = RubyCommand::resolve(root, true);
    // `load` may fall back to plain ruby if `bundle exec` is broken; run the
    // task with whatever actually produced the metadata.
    let (meta, ruby) = cache::load(root, &resolved)?;
    let task = meta
        .find_task(task_name)
        .ok_or_else(|| unknown_task(&meta, task_name))?;
    let parsed = args::parse(task, raw_args)?;

    let input = json!({
        "task": task_name,
        "params": parsed.params,
        "options": parsed.options,
        "dry_run": parsed.dry_run,
    });
    let input_bytes = serde_json::to_vec(&input)
        .map_err(|e| RtError::Internal(format!("cannot serialize task args: {e}")))?;

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
        let _ = stdin.write_all(&input_bytes);
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
                return Err(RtError::Task {
                    message: format_failure(&failure),
                });
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

/// A withheld sentinel line: the parsed failure (if the payload was valid JSON)
/// plus the raw bytes from the sentinel marker onward, so the caller can decide
/// whether to surface it as an error or re-emit it verbatim.
struct Captured {
    failure: Option<TaskFailure>,
    raw: Vec<u8>,
}

/// Stream the task's stderr through byte-for-byte, withholding the sentinel
/// payload wherever it appears. Works on raw bytes so non-UTF-8 output neither
/// aborts the tee nor leaks the sentinel.
fn tee_stderr<R: Read>(reader: R) -> Option<Captured> {
    let mut reader = BufReader::new(reader);
    let mut captured = None;
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        match find_subslice(&line, ERROR_SENTINEL) {
            Some(pos) => {
                let prefix = &line[..pos];
                if !prefix.is_empty() {
                    eprint!("{}", String::from_utf8_lossy(prefix));
                }
                let raw = line[pos..].to_vec();
                let rest = &line[pos + ERROR_SENTINEL.len()..];
                let json = String::from_utf8_lossy(rest);
                captured = Some(Captured {
                    failure: serde_json::from_str(json.trim()).ok(),
                    raw,
                });
            }
            None => eprint!("{}", String::from_utf8_lossy(&line)),
        }
    }
    captured
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn format_failure(failure: &TaskFailure) -> String {
    let mut msg = format!("{}: {}", failure.class, failure.message);
    for frame in &failure.backtrace {
        msg.push_str(&format!("\n    from {frame}"));
    }
    msg
}
