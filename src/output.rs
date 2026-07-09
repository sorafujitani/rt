use crate::error::RtError;
use crate::metadata::{Metadata, OptionType, Source, Task};
use owo_colors::OwoColorize;
use std::io::IsTerminal;

fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
}

pub fn print_error(err: &RtError) {
    let prefix = "error:";
    if use_color() {
        eprintln!("{} {err}", prefix.red().bold());
    } else {
        eprintln!("{prefix} {err}");
    }
}

pub fn print_warning(msg: &str) {
    let prefix = "warning:";
    if use_color() {
        eprintln!("{} {msg}", prefix.yellow().bold());
    } else {
        eprintln!("{prefix} {msg}");
    }
}

/// Emit load errors from discovery as human-readable warnings on stderr.
pub fn warn_load_errors(meta: &Metadata) {
    for e in &meta.errors {
        print_warning(&format!("{} ({}): {}", e.file, e.class, e.message));
    }
}

pub fn print_list(meta: &Metadata) {
    if meta.tasks.is_empty() {
        println!("No tasks defined.");
        return;
    }
    // A single common column width keeps names aligned across both sections.
    let width = meta.tasks.iter().map(|t| t.name.len()).max().unwrap_or(0);
    let project: Vec<&Task> = meta
        .tasks
        .iter()
        .filter(|t| t.source == Source::Project)
        .collect();
    let global: Vec<&Task> = meta
        .tasks
        .iter()
        .filter(|t| t.source == Source::Global)
        .collect();

    // Label the sections only when both are present; a single source reads
    // better as a plain list.
    let labeled = !project.is_empty() && !global.is_empty();
    if labeled {
        println!("Project tasks:");
    }
    print_task_rows(&project, width);
    if labeled {
        println!();
        println!("Global tasks:");
    }
    print_task_rows(&global, width);
}

fn print_task_rows(tasks: &[&Task], width: usize) {
    let mut tasks: Vec<&&Task> = tasks.iter().collect();
    tasks.sort_by(|a, b| a.name.cmp(&b.name));
    let color = use_color() && std::io::stdout().is_terminal();
    for t in tasks {
        let desc = t.description.as_deref().unwrap_or("");
        if color {
            println!("  {:<width$}  {}", t.name.cyan(), desc, width = width);
        } else {
            println!("  {:<width$}  {}", t.name, desc, width = width);
        }
    }
}

pub fn print_help(task: &Task, source_path: Option<&std::path::Path>) {
    let mut usage = format!("Usage: rt run {}", task.name);
    for p in &task.params {
        if p.required {
            usage.push_str(&format!(" <{}>", p.name));
        } else {
            usage.push_str(&format!(" [{}]", p.name));
        }
    }
    if !task.options.is_empty() {
        usage.push_str(" [options]");
    }
    println!("{usage}");

    if let Some(d) = &task.description {
        println!();
        println!("{d}");
    }

    match task.source {
        Source::Global => match source_path {
            Some(p) => println!("Source: global ({})", p.display()),
            None => println!("Source: global"),
        },
        Source::Project => {}
    }

    if !task.gems.is_empty() {
        let list: Vec<String> = task
            .gems
            .iter()
            .map(|g| {
                if g.requirements.is_empty() {
                    g.name.clone()
                } else {
                    format!("{} ({})", g.name, g.requirements.join(", "))
                }
            })
            .collect();
        println!("Gems: {}", list.join(", "));
    }

    if !task.params.is_empty() {
        println!();
        println!("Params:");
        for p in &task.params {
            let mut line = format!("  {}", p.name);
            if p.required {
                line.push_str(" (required)");
            }
            if let Some(values) = &p.enum_values {
                line.push_str(&format!(" [{}]", values.join(", ")));
            }
            if !p.default.is_null() {
                line.push_str(&format!(" (default: {})", p.default));
            }
            if let Some(desc) = &p.description {
                line.push_str(&format!(" - {desc}"));
            }
            println!("{line}");
        }
    }

    if !task.options.is_empty() {
        println!();
        println!("Options:");
        for o in &task.options {
            let ty = match o.option_type {
                OptionType::String => "string",
                OptionType::Integer => "integer",
                OptionType::Boolean => "boolean",
            };
            let mut line = format!("  --{} <{}>", o.name, ty);
            if !o.default.is_null() {
                line.push_str(&format!(" (default: {})", o.default));
            }
            if let Some(desc) = &o.description {
                line.push_str(&format!(" - {desc}"));
            }
            println!("{line}");
        }
        println!("  --dry-run  Preview without side effects");
    }
}

/// Build a one-line usage string for validation error messages.
pub fn usage_line(task: &Task) -> String {
    let mut usage = format!("rt run {}", task.name);
    for p in &task.params {
        if p.required {
            usage.push_str(&format!(" <{}>", p.name));
        } else {
            usage.push_str(&format!(" [{}]", p.name));
        }
    }
    for o in &task.options {
        usage.push_str(&format!(" [--{}]", o.name));
    }
    usage
}
