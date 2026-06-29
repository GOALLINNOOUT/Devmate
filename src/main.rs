use anyhow::Result;
use clap::Parser;
use devmate::{cli::Cli, run};

fn main() -> Result<()> {
    run(Cli::parse())
}
