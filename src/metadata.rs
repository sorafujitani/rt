use serde::{Deserialize, Serialize};

/// Version of the public `list/help --json` metadata schema.
pub const METADATA_SCHEMA_VERSION: u32 = 2;

/// Version of the private Rust/Ruby harness wire contract.
pub const HARNESS_PROTOCOL_VERSION: u32 = 2;

/// Contract between the Rust CLI and the Ruby harness. Parsed strictly at the
/// process boundary; the rest of the code trusts these types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(rename = "protocol_version")]
    pub schema_version: u32,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub errors: Vec<LoadError>,
}

/// Private payload emitted by the embedded Ruby harness. It deliberately does
/// not carry the public metadata schema version.
#[derive(Debug, Deserialize)]
pub(crate) struct HarnessMetadata {
    pub harness_protocol_version: u32,
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub errors: Vec<LoadError>,
}

impl HarnessMetadata {
    pub(crate) fn into_metadata(self) -> Metadata {
        Metadata {
            schema_version: METADATA_SCHEMA_VERSION,
            tasks: self.tasks,
            errors: self.errors,
        }
    }
}

/// Where a task was discovered. Emitted by the Rust merge step, not the
/// harness, so it defaults to `project` when absent from harness JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Project,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub file: String,
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub options: Vec<TaskOption>,
    #[serde(default)]
    pub gems: Vec<GemRequirement>,
    #[serde(default)]
    pub source: Source,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GemRequirement {
    pub name: String,
    #[serde(default)]
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: serde_json::Value,
    #[serde(default, rename = "enum")]
    pub enum_values: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OptionType {
    String,
    Integer,
    Boolean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOption {
    pub name: String,
    #[serde(rename = "type")]
    pub option_type: OptionType,
    #[serde(default)]
    pub default: serde_json::Value,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadError {
    pub file: String,
    pub class: String,
    pub message: String,
    #[serde(default)]
    pub source: Source,
}

impl Metadata {
    pub fn find_task(&self, name: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn public_metadata_json_contract() {
        let metadata = Metadata {
            schema_version: METADATA_SCHEMA_VERSION,
            tasks: Vec::new(),
            errors: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(metadata).unwrap(),
            json!({ "protocol_version": 2, "tasks": [], "errors": [] })
        );
    }
}
