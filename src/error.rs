use serde::Deserialize;
use std::fmt;

#[derive(Debug, Clone, Deserialize)]
pub struct ExceptionDetail {
    pub class: String,
    pub message: String,
    #[serde(default)]
    pub backtrace: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessFailureKind {
    TaskException,
    Environment,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HarnessFailure {
    pub kind: HarnessFailureKind,
    pub class: String,
    pub message: String,
    #[serde(default)]
    pub backtrace: Vec<String>,
}

impl HarnessFailure {
    pub fn into_rt_error(self) -> RtError {
        let failure = ExceptionDetail {
            class: self.class,
            message: self.message,
            backtrace: self.backtrace,
        };
        match self.kind {
            HarnessFailureKind::TaskException => RtError::Task(failure),
            HarnessFailureKind::Environment => RtError::EnvironmentFailure(failure),
        }
    }
}

#[derive(Debug)]
pub enum RtError {
    Usage(String),
    Task(ExceptionDetail),
    Internal(String),
    Environment(String),
    EnvironmentFailure(ExceptionDetail),
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
            RtError::EnvironmentFailure(_) => 74,
            RtError::TaskExit(code) => *code,
        }
    }
}

impl fmt::Display for RtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RtError::Usage(m) => write!(f, "{m}"),
            RtError::Task(failure) | RtError::EnvironmentFailure(failure) => {
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
