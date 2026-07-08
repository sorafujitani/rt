use std::fmt;

#[derive(Debug)]
pub enum RtError {
    Usage(String),
    Task {
        message: String,
    },
    Internal(String),
    Environment(String),
    /// A task called `exit n`; rt propagates the same code.
    TaskExit(i32),
}

impl RtError {
    pub fn exit_code(&self) -> i32 {
        match self {
            RtError::Task { .. } => 1,
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
            RtError::Task { message } => write!(f, "{message}"),
            RtError::Internal(m) => write!(f, "internal error: {m}"),
            RtError::Environment(m) => write!(f, "{m}"),
            RtError::TaskExit(code) => write!(f, "task exited with code {code}"),
        }
    }
}

impl std::error::Error for RtError {}
