use clap::Subcommand;

mod command;
mod suggest;

#[derive(Subcommand)]
pub enum GitCommands {
    Uncommitted,
    Staged,
    Branch {
        #[arg(help = "Base branch/ref (defaults to repo default)")]
        base: Option<String>,
    },
    Suggest,
    PrTitle,
}

pub use command::git_command;
