use crate::error::RtError;
use crate::metadata::{OptionType, Task};
use crate::output;
use serde_json::{Map, Value};

#[derive(Debug, PartialEq)]
pub struct ParsedArgs {
    pub params: Map<String, Value>,
    pub options: Map<String, Value>,
    pub dry_run: bool,
}

/// Parse task-specific params/options against a task's declared schema. This
/// is deliberately separate from clap: the schema is dynamic and we want full
/// control over validation messages.
pub fn parse(task: &Task, raw: &[String]) -> Result<ParsedArgs, RtError> {
    let usage = || output::usage_line(task);
    let fail = |msg: String| RtError::Usage(format!("{msg}\n\nusage: {}", usage()));

    let mut options: Map<String, Value> = Map::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut dry_run = false;

    let mut i = 0;
    while i < raw.len() {
        let token = &raw[i];
        if token == "--dry-run" {
            dry_run = true;
            i += 1;
            continue;
        }

        if let Some(body) = token.strip_prefix("--") {
            let (name, inline) = match body.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (body, None),
            };
            let opt = task
                .options
                .iter()
                .find(|o| o.name == name)
                .ok_or_else(|| fail(format!("unknown option --{name}")))?;

            let value = match opt.option_type {
                OptionType::Boolean => match inline {
                    None => Value::Bool(true),
                    Some(v) => Value::Bool(parse_bool(&v).map_err(|_| {
                        fail(format!("option --{name} expects true or false, got {v:?}"))
                    })?),
                },
                OptionType::Integer => {
                    let v = take_value(inline, raw, &mut i, name, &fail)?;
                    let n: i64 = v.parse().map_err(|_| {
                        fail(format!("option --{name} expects an integer, got {v:?}"))
                    })?;
                    Value::Number(n.into())
                }
                OptionType::String => {
                    let v = take_value(inline, raw, &mut i, name, &fail)?;
                    Value::String(v)
                }
            };
            options.insert(name.to_string(), value);
            i += 1;
            continue;
        }

        positionals.push(token.clone());
        i += 1;
    }

    if positionals.len() > task.params.len() {
        let extra = &positionals[task.params.len()];
        return Err(fail(format!("unexpected argument {extra:?}")));
    }

    let mut params: Map<String, Value> = Map::new();
    for (idx, param) in task.params.iter().enumerate() {
        match positionals.get(idx) {
            Some(raw_value) => {
                if let Some(allowed) = &param.enum_values {
                    if !allowed.iter().any(|a| a == raw_value) {
                        return Err(fail(format!(
                            "argument {:?} for <{}> must be one of: {}",
                            raw_value,
                            param.name,
                            allowed.join(", ")
                        )));
                    }
                }
                params.insert(param.name.clone(), Value::String(raw_value.clone()));
            }
            None => {
                if param.required {
                    return Err(fail(format!("missing required argument <{}>", param.name)));
                }
                params.insert(param.name.clone(), param.default.clone());
            }
        }
    }

    for opt in &task.options {
        options
            .entry(opt.name.clone())
            .or_insert_with(|| default_option(opt));
    }

    Ok(ParsedArgs {
        params,
        options,
        dry_run,
    })
}

fn default_option(opt: &crate::metadata::TaskOption) -> Value {
    if !opt.default.is_null() {
        return opt.default.clone();
    }
    match opt.option_type {
        OptionType::Boolean => Value::Bool(false),
        _ => Value::Null,
    }
}

fn take_value(
    inline: Option<String>,
    raw: &[String],
    i: &mut usize,
    name: &str,
    fail: &dyn Fn(String) -> RtError,
) -> Result<String, RtError> {
    if let Some(v) = inline {
        return Ok(v);
    }
    if *i + 1 < raw.len() {
        *i += 1;
        return Ok(raw[*i].clone());
    }
    Err(fail(format!("option --{name} requires a value")))
}

fn parse_bool(s: &str) -> Result<bool, ()> {
    match s {
        "true" | "1" | "yes" => Ok(true),
        "false" | "0" | "no" => Ok(false),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{OptionType, Param, Task, TaskOption};
    use serde_json::json;

    fn task() -> Task {
        Task {
            name: "deploy".to_string(),
            description: None,
            file: "tasks/deploy.rb".to_string(),
            params: vec![Param {
                name: "environment".to_string(),
                required: true,
                default: Value::Null,
                enum_values: Some(vec!["staging".to_string(), "production".to_string()]),
                description: None,
            }],
            options: vec![
                TaskOption {
                    name: "workers".to_string(),
                    option_type: OptionType::Integer,
                    default: json!(2),
                    description: None,
                },
                TaskOption {
                    name: "force".to_string(),
                    option_type: OptionType::Boolean,
                    default: json!(false),
                    description: None,
                },
                TaskOption {
                    name: "tag".to_string(),
                    option_type: OptionType::String,
                    default: Value::Null,
                    description: None,
                },
            ],
            gems: Vec::new(),
            source: crate::metadata::Source::Project,
        }
    }

    fn parse_ok(args: &[&str]) -> ParsedArgs {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&task(), &owned).unwrap()
    }

    fn parse_err(args: &[&str]) -> RtError {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&task(), &owned).unwrap_err()
    }

    #[test]
    fn positional_param_and_defaults() {
        let p = parse_ok(&["staging"]);
        assert_eq!(p.params["environment"], json!("staging"));
        assert_eq!(p.options["workers"], json!(2));
        assert_eq!(p.options["force"], json!(false));
        assert_eq!(p.options["tag"], Value::Null);
        assert!(!p.dry_run);
    }

    #[test]
    fn separate_and_equals_option_forms() {
        let a = parse_ok(&["staging", "--workers", "8"]);
        assert_eq!(a.options["workers"], json!(8));
        let b = parse_ok(&["staging", "--workers=8"]);
        assert_eq!(b.options["workers"], json!(8));
    }

    #[test]
    fn boolean_flag_and_explicit_value() {
        assert_eq!(
            parse_ok(&["staging", "--force"]).options["force"],
            json!(true)
        );
        assert_eq!(
            parse_ok(&["staging", "--force=false"]).options["force"],
            json!(false)
        );
    }

    #[test]
    fn dry_run_flag() {
        assert!(parse_ok(&["staging", "--dry-run"]).dry_run);
    }

    #[test]
    fn string_option() {
        assert_eq!(
            parse_ok(&["staging", "--tag", "v1"]).options["tag"],
            json!("v1")
        );
    }

    #[test]
    fn missing_required_param_is_usage_error() {
        let e = parse_err(&[]);
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("missing required argument"));
        assert!(e.to_string().contains("usage:"));
    }

    #[test]
    fn enum_violation_is_usage_error() {
        let e = parse_err(&["dev"]);
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("must be one of"));
    }

    #[test]
    fn bad_integer_is_usage_error() {
        let e = parse_err(&["staging", "--workers", "lots"]);
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("integer"));
    }

    #[test]
    fn unknown_option_is_usage_error() {
        let e = parse_err(&["staging", "--nope"]);
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("unknown option"));
    }

    #[test]
    fn extra_positional_is_usage_error() {
        let e = parse_err(&["staging", "extra"]);
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("unexpected argument"));
    }

    #[test]
    fn missing_value_for_option_is_usage_error() {
        let e = parse_err(&["staging", "--workers"]);
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("requires a value"));
    }
}
