use chrono::{Datelike, Timelike};
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
    #[allow(dead_code)] // Reserved for future use, currently only in match expressions
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

    yellow!("📊 Validation Complete - Summary Statistics:\n");
    println!("   Total files checked: {}", result.total);
    green!("✅ Intact files: {}\n", result.valid);
    red!("❌ Corrupted files: {}\n", result.corrupted);
    yellow!(
        "⏭️ Skipped files (password protected or unsupported): {}\n",
        result.skipped
    );
}

fn main() {
    let cli = Cli::parse();

    let zip_extensions = ["zip"];

    let path = cli
        .path
        .unwrap_or_else(|| current_dir().expect("Failed to get current directory"));

    let save_log = cli.log.is_some();

    let now = chrono::Local::now();

    let file_name = format!(
        "check-zip_{}{}{}{}{}{}{}.log",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.timestamp_subsec_millis(),
    );

    let log = cli.log.unwrap_or_else(|| PathBuf::from(file_name.clone()));

    let log = if log.is_dir() {
        log.join(file_name)
    } else {
        log
    };

    let cwd = path.clone();

    let cores = match thread::available_parallelism() {
        Ok(cores) => cores.get(),
        Err(_) => 1,
    };

    let path_mutex = Arc::new(Mutex::new(Vec::<PathBuf>::new()));

    let path_lock = path_mutex.clone();

    let result_mutex = Arc::new(Mutex::new(CheckResult::default()));

    let result_lock = result_mutex.clone();

    let log_mutex = Arc::new(Mutex::new(Vec::<String>::new()));

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

        let total = {
            let path = path_lock.lock().unwrap();
            path.len()
        };

        let mut result = result_lock.lock().unwrap();
        result.total = total;
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
        let log_lock = log_mutex.clone();

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

                        let rel_path = path.strip_prefix(&cwd).unwrap_or(&path);

                        let log_line = {
                            let mut result = result_lock.lock().unwrap();

                            match status {
                                ZipFileStatus::Valid => {
                                    result.valid += 1;
                                    format!("✅ [VALID] {}\n", rel_path.display())
                                }
                                ZipFileStatus::PasswordProtected => {
                                    result.skipped += 1;
                                    format!("🔐 [PASSWORD PROTECTED] {}\n", rel_path.display())
                                }
                                ZipFileStatus::Corrupted(ref msg) => {
                                    result.corrupted += 1;
                                    format!("❌ [CORRUPTED] {} - {}\n", rel_path.display(), msg)
                                }
                                ZipFileStatus::Unsupported => {
                                    result.skipped += 1;
                                    format!("⏭️ [UNSUPPORTED] {}\n", rel_path.display())
                                }
                            }
                        }; // result_lock is released here

                        // Print outside of lock to avoid blocking other threads
                        match status {
                            ZipFileStatus::Valid => green!("{}", log_line),
                            ZipFileStatus::PasswordProtected => yellow!("{}", log_line),
                            ZipFileStatus::Corrupted(_) => red!("{}", log_line),
                            ZipFileStatus::Unsupported => yellow!("{}", log_line),
                        }

                        // Acquire log_lock separately after result_lock is released
                        let mut log = log_lock.lock().unwrap();

                        log.push(log_line);
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

    if save_log {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut log_content = log_mutex.lock().unwrap();

        log_content.push("========================================================\n".to_string());
        log_content.push(String::from(
            "📊 Validation Complete - Summary Statistics:\n",
        ));
        log_content.push(format!("   Total files checked: {}\n", result.total));
        log_content.push(format!("✅ Intact files: {}\n", result.valid));
        log_content.push(format!("❌ Corrupted files: {}\n", result.corrupted));
        log_content.push(format!(
            "⏭️ Skipped files (password protected or unsupported): {}\n",
            result.skipped
        ));

        let content = log_content.clone();

        drop(log_content);

        let log_path = log.clone();

        let tx = tx.clone();

        thread::spawn(move || match std::fs::write(&log_path, content.concat()) {
            Ok(_) => {
                tx.send(Ok(())).unwrap();
            }
            Err(e) => {
                tx.send(Err(e)).unwrap();
            }
        });

        match rx.recv().unwrap() {
            Ok(_) => {
                green!("📝 Log file saved successfully at: {}\n", log.display());
            }
            Err(e) => {
                red!("❌ Failed to save log file: {}\n", e);
            }
        }
    }
}
