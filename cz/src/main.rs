use clap::Parser;
use colour::{green, red, yellow};
use std::env::current_dir;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use zip::ZipArchive;

mod cli;
use cli::Cli;

enum ZipFileStatus {
    Valid,
    PasswordProtected,
    Corrupted(String),
    #[allow(dead_code)]
    Unsupported,
}

#[derive(Default)]
struct CheckResult {
    pub total: usize,
    pub valid: usize,
    pub skipped: usize,
    pub corrupted: usize,
}


fn check_zip_file(path: &PathBuf) -> ZipFileStatus {
    // Try to open the file
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => return ZipFileStatus::Corrupted(format!("Cannot open file: {}", e)),
    };

    // Try to read the zip archive
    let mut archive = match ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(e) => return ZipFileStatus::Corrupted(format!("Invalid zip format: {}", e)),
    };

    // Check if any file in the archive is password protected
    for i in 0..archive.len() {
        match archive.by_index(i) {
            Ok(file) => {
                // Check if the file is encrypted
                if file.encrypted() {
                    return ZipFileStatus::PasswordProtected;
                }
            }
            Err(e) => {
                let message = format!("{}", e);

                if message.contains("Password required to decrypt file") {
                    return ZipFileStatus::PasswordProtected;
                }

                return ZipFileStatus::Corrupted(format!("Cannot read file at index {}: {}", i, e));
            }
        }
    }

    ZipFileStatus::Valid
}

fn print_summary(result: &CheckResult) {
    println!("========================================================");

    yellow!("📊 Validation Complete - Summary Statistics:!\n");
    println!("  Total files checked: {}", result.total);
    green!("✅ Intact files: {}\n", result.valid);
    red!("❌ Corrupted files: {}\n", result.corrupted);
    yellow!(
        "⏭️ Skipped files (password protected or unsupported): {}\n",
        result.skipped
    );
    println!("========================================================");
}

fn main() {
    let cli = Cli::parse();

    let zip_extensions = ["zip"];

    let path = cli
        .path
        .unwrap_or_else(|| current_dir().expect("Failed to get current directory"));

    let cwd = path.clone();

    let cores = match thread::available_parallelism() {
        Ok(cores) => cores.get(),
        Err(_) => 1,
    };

    let path_mutex = Arc::new(Mutex::new(Vec::<PathBuf>::new()));

    let path_lock = path_mutex.clone();

    let result_mutex = Arc::new(Mutex::new(CheckResult::default()));

    let result_lock = result_mutex.clone();

    let walker_thread = thread::spawn(move || {
        let mut stack = vec![path.clone()];

        while let Some(current_path) = stack.pop() {
            let ext = if let Some(ext) = current_path.extension() {
                ext.to_str().unwrap()
            } else {
                ""
            };

            if current_path.is_dir() {
                if let Ok(entries) = current_path.read_dir() {
                    for entry in entries.flatten() {
                        stack.push(entry.path());
                    }
                }
            } else if zip_extensions.contains(&ext) {
                let mut vec = path_lock.lock().unwrap();

                vec.push(current_path);
            }
        }

        let path = path_lock.lock().unwrap();

        let mut result = result_lock.lock().unwrap();
        result.total = path.len();
    });

    walker_thread.join().expect("Walker thread panicked");

    yellow!(
        "🔍 Recursively checking all ZIP files in current directory ({:})...\n",
        cwd.clone().display()
    );

    let mut children: Vec<JoinHandle<()>> = vec![];

    for _ in 0..cores {
        let path_lock = path_mutex.clone();
        let result_lock = result_mutex.clone();

        let cwd = cwd.clone();

        children.push(thread::spawn(move || {
            loop {
                let path_option = {
                    let mut vec = path_lock.lock().unwrap();
                    vec.pop()
                };

                match path_option {
                    Some(path) => {
                        // Process the zip file at 'path'
                        let status = check_zip_file(&path);

                        let mut result = result_lock.lock().unwrap();

                        let rel_path = path.strip_prefix(&cwd).unwrap_or(&path);

                        match status {
                            ZipFileStatus::Valid => {
                                green!("✅ [VALID] {}\n", rel_path.display());

                                result.valid += 1;
                            }
                            ZipFileStatus::PasswordProtected => {
                                yellow!("🔐 [PASSWORD PROTECTED] {}\n", rel_path.display());

                                result.skipped += 1;
                            }
                            ZipFileStatus::Corrupted(_msg) => {
                                red!("❌ [CORRUPTED] {}\n", rel_path.display());

                                result.corrupted += 1;
                            }
                            ZipFileStatus::Unsupported => {
                                yellow!("⏭️ [UNSUPPORTED] {}\n", rel_path.display());

                                result.skipped += 1;
                            }
                        }
                    }
                    None => break,
                }
            }
        }))
    }

    for child in children {
        let id = child.thread().id();

        child
            .join()
            .unwrap_or_else(|_| panic!("Failed to join thread: {:?}", id));
    }

    let result = result_mutex.lock().unwrap();

    println!();
    print_summary(&result);
}
