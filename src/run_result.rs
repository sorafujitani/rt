use crate::error::{RtError, TaskFailure};
use crate::metadata::LoadError;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Serialize;

pub const RUN_RESULT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Default)]
pub(crate) struct CapturedBytes {
    bytes: Vec<u8>,
    total_bytes: u64,
}

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
    total_bytes: u64,
    captured_bytes: u64,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct RunError {
    kind: &'static str,
    class: Option<String>,
    message: String,
    backtrace: Vec<String>,
}

impl CapturedOutput {
    fn from_capture(capture: CapturedBytes) -> Self {
        let captured_bytes = capture.bytes.len() as u64;
        let truncated = captured_bytes < capture.total_bytes;
        let (encoding, data) = match String::from_utf8(capture.bytes) {
            Ok(data) => ("utf-8", data),
            Err(error) => ("base64", STANDARD.encode(error.into_bytes())),
        };
        Self {
            encoding,
            data,
            total_bytes: capture.total_bytes,
            captured_bytes,
            truncated,
        }
    }
}

impl CapturedBytes {
    pub(crate) fn new(bytes: Vec<u8>, total_bytes: u64) -> Self {
        debug_assert!(total_bytes >= bytes.len() as u64);
        Self { bytes, total_bytes }
    }

    pub(crate) fn empty() -> Self {
        Self::default()
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
        stdout: CapturedBytes,
        stderr: CapturedBytes,
        load_errors: Vec<LoadError>,
    ) -> Self {
        Self {
            schema_version: RUN_RESULT_SCHEMA_VERSION,
            task: task.to_string(),
            status: RunStatus::Success,
            exit_code: 0,
            stdout: CapturedOutput::from_capture(stdout),
            stderr: CapturedOutput::from_capture(stderr),
            error: None,
            load_errors,
        }
    }

    pub fn error(
        task: &str,
        error: RtError,
        stdout: CapturedBytes,
        stderr: CapturedBytes,
        load_errors: Vec<LoadError>,
    ) -> Self {
        let exit_code = error.exit_code();
        Self {
            schema_version: RUN_RESULT_SCHEMA_VERSION,
            task: task.to_string(),
            status: RunStatus::Error,
            exit_code,
            stdout: CapturedOutput::from_capture(stdout),
            stderr: CapturedOutput::from_capture(stderr),
            error: Some(RunError::from_rt_error(&error)),
            load_errors,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn successful_run_json_contract() {
        let result = RunResult::success(
            "greet",
            CapturedBytes::new(b"hello\n".to_vec(), 6),
            CapturedBytes::empty(),
            Vec::new(),
        );
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "schema_version": 2,
                "task": "greet",
                "status": "success",
                "exit_code": 0,
                "stdout": {
                    "encoding": "utf-8",
                    "data": "hello\n",
                    "total_bytes": 6,
                    "captured_bytes": 6,
                    "truncated": false
                },
                "stderr": {
                    "encoding": "utf-8",
                    "data": "",
                    "total_bytes": 0,
                    "captured_bytes": 0,
                    "truncated": false
                },
                "error": null,
                "load_errors": []
            })
        );
    }
}
