use crate::metadata::{LoadError, Metadata, OptionType, Source, Task};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub const TOOL_CATALOG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct ToolCatalog {
    schema_version: u32,
    tools: Vec<ToolDefinition>,
    errors: Vec<LoadError>,
}

#[derive(Debug, Serialize)]
struct ToolDefinition {
    task: String,
    description: Option<String>,
    source: Source,
    input_schema: InputSchema,
}

#[derive(Debug, Serialize)]
struct InputSchema {
    #[serde(rename = "type")]
    schema_type: JsonType,
    properties: BTreeMap<String, InputProperty>,
    required: Vec<String>,
    #[serde(rename = "additionalProperties")]
    additional_properties: bool,
}

#[derive(Debug, Serialize)]
struct InputProperty {
    #[serde(rename = "type")]
    schema_type: JsonType,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    enum_values: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum JsonType {
    Object,
    String,
    Integer,
    Boolean,
}

impl ToolCatalog {
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            schema_version: TOOL_CATALOG_SCHEMA_VERSION,
            tools: metadata
                .tasks
                .iter()
                .map(ToolDefinition::from_task)
                .collect(),
            errors: metadata.errors.clone(),
        }
    }
}

impl ToolDefinition {
    fn from_task(task: &Task) -> Self {
        let mut properties = BTreeMap::new();
        let mut required = Vec::new();

        for param in &task.params {
            properties.insert(
                param.name.clone(),
                InputProperty {
                    schema_type: JsonType::String,
                    description: param.description.clone(),
                    enum_values: param.enum_values.clone(),
                    default: non_null_default(&param.default),
                },
            );
            if param.required {
                required.push(param.name.clone());
            }
        }

        for option in &task.options {
            let default = match option.option_type {
                OptionType::Boolean if option.default.is_null() => Some(Value::Bool(false)),
                _ => non_null_default(&option.default),
            };
            properties.insert(
                option.name.clone(),
                InputProperty {
                    schema_type: JsonType::from(option.option_type),
                    description: option.description.clone(),
                    enum_values: None,
                    default,
                },
            );
        }

        properties.insert(
            "dry_run".to_string(),
            InputProperty {
                schema_type: JsonType::Boolean,
                description: Some("Set the task's dry-run flag".to_string()),
                enum_values: None,
                default: Some(Value::Bool(false)),
            },
        );

        Self {
            task: task.name.clone(),
            description: task.description.clone(),
            source: task.source,
            input_schema: InputSchema {
                schema_type: JsonType::Object,
                properties,
                required,
                additional_properties: false,
            },
        }
    }
}

impl From<OptionType> for JsonType {
    fn from(option_type: OptionType) -> Self {
        match option_type {
            OptionType::String => Self::String,
            OptionType::Integer => Self::Integer,
            OptionType::Boolean => Self::Boolean,
        }
    }
}

fn non_null_default(value: &Value) -> Option<Value> {
    (!value.is_null()).then(|| value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{LoadError, OptionType, Param, Task, TaskOption};
    use serde_json::json;

    #[test]
    fn complete_catalog_json_contract_preserves_raw_task_name() {
        let metadata = Metadata {
            schema_version: crate::metadata::METADATA_SCHEMA_VERSION,
            tasks: vec![Task {
                name: "deploy:prod/v1".to_string(),
                description: Some("Deploy the application".to_string()),
                file: "tasks/deploy.rb".to_string(),
                params: vec![
                    Param {
                        name: "environment".to_string(),
                        required: true,
                        default: Value::Null,
                        enum_values: Some(vec!["staging".to_string(), "production".to_string()]),
                        description: Some("Target environment".to_string()),
                    },
                    Param {
                        name: "revision".to_string(),
                        required: false,
                        default: json!("main"),
                        enum_values: None,
                        description: None,
                    },
                ],
                options: vec![
                    TaskOption {
                        name: "workers".to_string(),
                        option_type: OptionType::Integer,
                        default: json!(2),
                        description: Some("Worker count".to_string()),
                    },
                    TaskOption {
                        name: "force".to_string(),
                        option_type: OptionType::Boolean,
                        default: Value::Null,
                        description: None,
                    },
                    TaskOption {
                        name: "label".to_string(),
                        option_type: OptionType::String,
                        default: Value::Null,
                        description: None,
                    },
                ],
                gems: Vec::new(),
                source: Source::Global,
            }],
            errors: vec![LoadError {
                file: "tasks/broken.rb".to_string(),
                class: "SyntaxError".to_string(),
                message: "unexpected end-of-input".to_string(),
                source: Source::Global,
            }],
        };

        assert_eq!(
            serde_json::to_value(ToolCatalog::from_metadata(&metadata)).unwrap(),
            json!({
                "schema_version": 1,
                "tools": [{
                    "task": "deploy:prod/v1",
                    "description": "Deploy the application",
                    "source": "global",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "environment": {
                                "type": "string",
                                "description": "Target environment",
                                "enum": ["staging", "production"]
                            },
                            "revision": { "type": "string", "default": "main" },
                            "workers": {
                                "type": "integer",
                                "description": "Worker count",
                                "default": 2
                            },
                            "force": { "type": "boolean", "default": false },
                            "label": { "type": "string" },
                            "dry_run": {
                                "type": "boolean",
                                "description": "Set the task's dry-run flag",
                                "default": false
                            }
                        },
                        "required": ["environment"],
                        "additionalProperties": false
                    }
                }],
                "errors": [{
                    "file": "tasks/broken.rb",
                    "class": "SyntaxError",
                    "message": "unexpected end-of-input",
                    "source": "global"
                }]
            })
        );
    }
}
