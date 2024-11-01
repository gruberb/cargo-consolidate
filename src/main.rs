use anyhow::Result;
use log::error;

mod cli;
mod dependency;
mod workspace;

fn main() {
    if let Err(err) = run() {
        error!("{:?}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let opt = cli::parse_args();
    cli::setup_logging(opt.verbose);

    workspace::consolidate_dependencies(opt.manifest_path, opt.group_all)
}
