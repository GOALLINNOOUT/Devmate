pub mod cli;
pub mod commands;
pub mod errors;
pub mod fswalk;
pub mod models;
pub mod output;

use anyhow::Result;
use cli::{Cli, Commands};

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Analyze(args) => commands::analyze::run(args),
        Commands::Json(args) => commands::json::run(args),
        Commands::Env(args) => commands::env::run(args),
        Commands::Git(args) => commands::git::run(args),
        Commands::Files(args) => commands::files::run(args),
        Commands::Jwt(args) => commands::jwt::run(args),
        Commands::System(args) => commands::system::run(args),
        Commands::Doctor(args) => commands::doctor::run(args),
        Commands::Setup(args) => commands::setup::run(args),
        Commands::Kill(args) => commands::kill::run(args),
    }
}
