use serde::Deserialize;
use std::fmt;

#[derive(Debug, Clone, Deserialize)]
pub struct TaskFailure {
    pub class: String,
    pub message: String,
    #[serde(default)]
    pub backtrace: Vec<String>,
}

#[derive(Debug)]
pub enum RtError {
    Usage(String),
    Task(TaskFailure),
    Internal(String),
    Environment(String),
    /// A task called `exit n`; rt propagates the same code.
    TaskExit(i32),
}

impl RtError {
    pub fn exit_code(&self) -> i32 {
        match self {
            RtError::Task(_) => 1,
            RtError::Usage(_) => 2,
            RtError::Internal(_) => 70,
            RtError::Environment(_) => 74,
            RtError::TaskExit(code) => *code,
        }
    }
}

impl fmt::Display for RtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RtError::Usage(m) => write!(f, "{m}"),
            RtError::Task(failure) => {
                write!(f, "{}: {}", failure.class, failure.message)?;
                for frame in &failure.backtrace {
                    write!(f, "\n    from {frame}")?;
                }
                Ok(())
            }
            RtError::Internal(m) => write!(f, "internal error: {m}"),
            RtError::Environment(m) => write!(f, "{m}"),
            RtError::TaskExit(code) => write!(f, "task exited with code {code}"),
        }
    }
}

impl std::error::Error for RtError {}
