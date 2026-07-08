use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "rt",
    about = "Run Ruby-defined tasks from a discoverable CLI",
    version,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List available tasks
    List {
        /// Emit machine-readable JSON on stdout
        #[arg(long)]
        json: bool,
    },
    /// Show usage for a single task
    Help {
        /// Task name
        task: String,
        /// Emit machine-readable JSON on stdout
        #[arg(long)]
        json: bool,
    },
    /// Run a task
    Run {
        /// Task name
        task: String,
        /// Task-specific params and options (parsed by rt, not clap)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
