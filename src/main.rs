mod args;
mod cache;
mod cli;
mod error;
mod metadata;
mod output;
mod project;
mod ruby;
mod runner;

use clap::Parser;
use cli::{Cli, Command};
use error::RtError;

fn main() {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(()) => {}
        Err(err) => {
            // A task's own exit code is passed through without reframing it as
            // an rt error; the task has already spoken for itself.
            if !matches!(err, RtError::TaskExit(_)) {
                output::print_error(&err);
            }
            std::process::exit(err.exit_code());
        }
    }
}

fn dispatch(cli: Cli) -> Result<(), RtError> {
    let root = project::find_root()?;
    match cli.command {
        Command::List { json } => runner::list(&root, json),
        Command::Help { task, json } => runner::help(&root, &task, json),
        Command::Run { task, args } => runner::run(&root, &task, &args),
    }
}
