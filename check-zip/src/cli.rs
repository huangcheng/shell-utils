use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "check-zip",
    about = "A tool for checking integrity of zip archives"
)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Folder to operate on
    pub path: Option<PathBuf>,

    // Log file to write results to
    #[arg(short, long, value_name = "LOG_FILE")]
    pub log: Option<PathBuf>,
}
