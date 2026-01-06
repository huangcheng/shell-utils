use std::path::PathBuf;

use clap::{Parser};

#[derive(Parser)]
#[command(name = "check-zip", version = "0.1.0", about = "A tool for checking integrity of zip archives")]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Folder to operate on
    #[arg(short, long, value_name = "FOLDER")]
    pub path: Option<PathBuf>,
}
