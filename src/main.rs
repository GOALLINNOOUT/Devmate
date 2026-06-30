use anyhow::Result;
use clap::{CommandFactory, Parser};
use devmate::{cli::Cli, run};

fn main() -> Result<()> {
    if std::env::args_os().len() == 1 {
        Cli::command().print_help()?;
        anstream::println!();
        return Ok(());
    }
    run(Cli::parse())
}
