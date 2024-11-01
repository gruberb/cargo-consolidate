use clap::Parser;
use log::LevelFilter;
use std::path::PathBuf;

#[derive(Parser)]
pub struct Opt {
    /// Path to the workspace root Cargo.toml
    /// of the project you want to consolidate
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Group dependencies of all members into workspace.dependencies
    /// If set to false, just dependencies which are used by 2 or more
    /// members are being grouped into workspace.dependencies
    #[arg(long)]
    pub group_all: bool,

    /// Increase output verbosity (can be used multiple times)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

pub fn parse_args() -> Opt {
    Opt::parse()
}

pub fn setup_logging(verbose: u8) {
    let log_level = match verbose {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };
    env_logger::Builder::new().filter_level(log_level).init();
}
