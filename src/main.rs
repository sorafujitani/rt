mod args;
mod cache;
mod cli;
mod error;
mod metadata;
mod output;
mod project;
mod ruby;
mod run_result;
mod runner;

use clap::Parser;
use cli::{Cli, Command};
use error::RtError;
use std::ffi::{OsStr, OsString};

fn main() {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let command = match Cli::try_parse_from(raw_args.clone()) {
        Ok(cli) => cli.command,
        Err(error) if json_run_requested(&raw_args) && error.exit_code() != 0 => {
            let result = run_result::RunResult::error(
                &run_task_name(&raw_args),
                RtError::Usage(error.to_string().trim().to_string()),
                run_result::CapturedBytes::empty(),
                run_result::CapturedBytes::empty(),
                Vec::new(),
            );
            print_run_result(result);
            return;
        }
        Err(error) => error.exit(),
    };
    match command {
        Command::Run {
            json: true,
            task,
            args,
        } => {
            let result = match project::find_roots() {
                Ok(roots) => runner::run_json(&roots, &task, &args),
                Err(error) => run_result::RunResult::error(
                    &task,
                    error,
                    run_result::CapturedBytes::empty(),
                    run_result::CapturedBytes::empty(),
                    Vec::new(),
                ),
            };
            print_run_result(result);
        }
        command => match dispatch(command) {
            Ok(()) => {}
            Err(err) => {
                // A task's own exit code is passed through without reframing it as
                // an rt error; the task has already spoken for itself.
                if !matches!(err, RtError::TaskExit(_)) {
                    output::print_error(&err);
                }
                std::process::exit(err.exit_code());
            }
        },
    }
}

fn json_run_requested(args: &[OsString]) -> bool {
    args.get(1).is_some_and(|arg| arg == OsStr::new("run"))
        && args
            .iter()
            .skip(2)
            .take_while(|arg| *arg != OsStr::new("--"))
            .any(|arg| arg == OsStr::new("--json"))
}

fn run_task_name(args: &[OsString]) -> String {
    args.iter()
        .skip(2)
        .take_while(|arg| *arg != OsStr::new("--"))
        .find(|arg| *arg != OsStr::new("--json") && !arg.to_string_lossy().starts_with('-'))
        .map(|arg| arg.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn print_run_result(result: run_result::RunResult) {
    let exit_code = result.exit_code;
    println!(
        "{}",
        serde_json::to_string_pretty(&result).expect("run result is serializable")
    );
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn dispatch(command: Command) -> Result<(), RtError> {
    let roots = project::find_roots()?;
    match command {
        Command::List { json } => runner::list(&roots, json),
        Command::Help { task, json } => runner::help(&roots, &task, json),
        Command::Run {
            json: false,
            task,
            args,
        } => runner::run(&roots, &task, &args),
        Command::Run { json: true, .. } => unreachable!("JSON runs are handled in main"),
    }
}
