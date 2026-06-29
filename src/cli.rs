use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "devmate",
    version,
    about = "A polished developer companion CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Analyze(AnalyzeArgs),
    Json(JsonArgs),
    Env(EnvArgs),
    Git(GitArgs),
    Files(FilesArgs),
    Jwt(JwtArgs),
    System(SystemArgs),
    Doctor(DoctorArgs),
    Setup(SetupArgs),
    Kill(KillArgs),
}

#[derive(Debug, Args)]
pub struct AnalyzeArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub json: bool,
    #[arg(long, default_value_t = 512 * 1024)]
    pub large_file_bytes: u64,
}

#[derive(Debug, Args)]
pub struct JsonArgs {
    #[command(subcommand)]
    pub command: JsonCommand,
}

#[derive(Debug, Subcommand)]
pub enum JsonCommand {
    Validate {
        file: PathBuf,
    },
    Format {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    Minify {
        file: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    Diff {
        left: PathBuf,
        right: PathBuf,
    },
}

#[derive(Debug, Args)]
pub struct EnvArgs {
    #[command(subcommand)]
    pub command: Option<EnvCommand>,
}

#[derive(Debug, Subcommand)]
pub enum EnvCommand {
    Inspect {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long, default_value = ".env")]
        file: PathBuf,
        #[arg(short, long)]
        example: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
pub struct GitArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub json: bool,
    #[arg(short, long, default_value_t = 8)]
    pub commits: usize,
}

#[derive(Debug, Args)]
pub struct FilesArgs {
    #[command(subcommand)]
    pub command: FilesCommand,
}

#[derive(Debug, Subcommand)]
pub enum FilesCommand {
    Search {
        pattern: String,
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        regex: bool,
        #[arg(long)]
        json: bool,
    },
    Tree {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
        #[arg(long)]
        json: bool,
    },
    Stats {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Dupes {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Args)]
pub struct JwtArgs {
    #[command(subcommand)]
    pub command: JwtCommand,
}

#[derive(Debug, Subcommand)]
pub enum JwtCommand {
    Generate {
        #[arg(short, long)]
        secret: String,
        #[arg(short, long, value_enum, default_value_t = JwtAlgorithmArg::Hs256)]
        algorithm: JwtAlgorithmArg,
        #[arg(long)]
        claim: Vec<String>,
        #[arg(long)]
        expires_in: Option<i64>,
    },
    Decode {
        token: String,
        #[arg(short, long)]
        secret: Option<String>,
        #[arg(short, long, value_enum, default_value_t = JwtAlgorithmArg::Hs256)]
        algorithm: JwtAlgorithmArg,
    },
    Verify {
        token: String,
        #[arg(short, long)]
        secret: String,
        #[arg(short, long, value_enum, default_value_t = JwtAlgorithmArg::Hs256)]
        algorithm: JwtAlgorithmArg,
    },
    Interactive,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum JwtAlgorithmArg {
    Hs256,
    Hs384,
    Hs512,
}

#[derive(Debug, Args)]
pub struct SystemArgs {
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub watch: bool,
    #[arg(
        long,
        default_value_t = 1,
        help = "Seconds between samples in watch mode"
    )]
    pub interval: u64,
    #[arg(long, hide = true)]
    pub ticks: Option<usize>,
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SetupArgs {
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct KillArgs {
    #[arg(long, default_value_t = 5)]
    pub top: usize,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub yes: bool,
    #[arg(long)]
    pub all_listed: bool,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub json: bool,
}
