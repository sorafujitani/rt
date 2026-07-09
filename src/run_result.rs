use crate::error::{RtError, TaskFailure};
use crate::metadata::LoadError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Serialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct RunResult {
    schema_version: u32,
    task: String,
    status: RunStatus,
    pub exit_code: i32,
    stdout: CapturedOutput,
    stderr: CapturedOutput,
    error: Option<RunError>,
    load_errors: Vec<LoadError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum RunStatus {
    Success,
    Error,
}

#[derive(Debug, Serialize)]
struct CapturedOutput {
    encoding: &'static str,
    data: String,
}

#[derive(Debug, Serialize)]
struct RunError {
    kind: &'static str,
    class: Option<String>,
    message: String,
    backtrace: Vec<String>,
}

impl CapturedOutput {
    fn from_bytes(bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(data) => Self {
                encoding: "utf-8",
                data,
            },
            Err(error) => Self {
                encoding: "base64",
                data: STANDARD.encode(error.into_bytes()),
            },
        }
    }
}

impl RunError {
    fn from_rt_error(error: &RtError) -> Self {
        match error {
            RtError::Usage(message) => Self::plain("usage", message.clone()),
            RtError::Task(failure) => Self::task(failure),
            RtError::Internal(message) => {
                Self::plain("internal", format!("internal error: {message}"))
            }
            RtError::Environment(message) => Self::plain("environment", message.clone()),
            RtError::TaskExit(70) => {
                Self::plain("internal", "task harness exited with code 70".to_string())
            }
            RtError::TaskExit(74) => Self::plain(
                "environment",
                "task environment setup exited with code 74".to_string(),
            ),
            RtError::TaskExit(code) => {
                Self::plain("task_exit", format!("task exited with code {code}"))
            }
        }
    }

    fn task(failure: &TaskFailure) -> Self {
        Self {
            kind: "task_exception",
            class: Some(failure.class.clone()),
            message: failure.message.clone(),
            backtrace: failure.backtrace.clone(),
        }
    }

    fn plain(kind: &'static str, message: String) -> Self {
        Self {
            kind,
            class: None,
            message,
            backtrace: Vec::new(),
        }
    }
}

impl RunResult {
    pub fn success(
        task: &str,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        load_errors: Vec<LoadError>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            task: task.to_string(),
            status: RunStatus::Success,
            exit_code: 0,
            stdout: CapturedOutput::from_bytes(stdout),
            stderr: CapturedOutput::from_bytes(stderr),
            error: None,
            load_errors,
        }
    }

    pub fn error(
        task: &str,
        error: RtError,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        load_errors: Vec<LoadError>,
    ) -> Self {
        let exit_code = error.exit_code();
        Self {
            schema_version: SCHEMA_VERSION,
            task: task.to_string(),
            status: RunStatus::Error,
            exit_code,
            stdout: CapturedOutput::from_bytes(stdout),
            stderr: CapturedOutput::from_bytes(stderr),
            error: Some(RunError::from_rt_error(&error)),
            load_errors,
        }
    }
}
