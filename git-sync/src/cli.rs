use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "git-sync",
    about = "A tool for synchronizing git repositories"
)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Folder to operate on
    pub path: Option<PathBuf>,
}
