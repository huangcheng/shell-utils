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
    let args = Cli::parse();

    let zip_extensions = ["zip"];

    let path = args
        .path
        .unwrap_or_else(|| current_dir().expect("Failed to get current directory"));

    let save_log = args.log.is_some();

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

    let log = args.log.unwrap_or_else(|| PathBuf::from(file_name.clone()));

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

    let corrupted = Arc::new(Mutex::new(Vec::<PathBuf>::new()));

    for _ in 0..cores {
        let path_lock = path_mutex.clone();
        let result_lock = result_mutex.clone();
        let log_lock = log_mutex.clone();
        let corrupted = corrupted.clone();

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

                                    {
                                        let mut corrupted_files = corrupted.lock().unwrap();
                                        corrupted_files.push(path.clone());
                                    }

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

    println!();

    yellow!("Do you want to delete all corrupted zip files? (y/N): ");

    use std::io::{self, Write};
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    if input.trim().eq_ignore_ascii_case("y") {
        let handle = thread::spawn(move || {
            let corrupted_files = corrupted.lock().unwrap();
            for file in corrupted_files.iter() {
                match std::fs::remove_file(file) {
                    Ok(_) => {
                        green!("🗑️ Deleted corrupted file: {}\n", file.display());
                    }
                    Err(e) => {
                        red!("❌ Failed to delete file {}: {}\n", file.display(), e);
                    }
                }
            }
        });

        handle.join().expect("Failed to join deletion thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    /// Helper function to create a valid ZIP file with test content
    fn create_valid_zip(path: &PathBuf) -> std::io::Result<()> {
        let file = File::create(path)?;
        let mut zip = ZipWriter::new(file);

        let options: FileOptions<()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("test.txt", options)?;
        zip.write_all(b"Hello, World!")?;

        zip.start_file("folder/nested.txt", options)?;
        zip.write_all(b"Nested content")?;

        zip.finish()?;
        Ok(())
    }

    /// Helper function to create an empty but valid ZIP file
    fn create_empty_zip(path: &PathBuf) -> std::io::Result<()> {
        let file = File::create(path)?;
        let zip = ZipWriter::new(file);
        zip.finish()?;
        Ok(())
    }

    /// Helper function to create a corrupted ZIP file
    fn create_corrupted_zip(path: &PathBuf) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        // Write invalid ZIP data
        file.write_all(b"PK\x03\x04")?; // ZIP header
        file.write_all(&[0u8; 100])?; // Corrupted data
        Ok(())
    }

    /// Helper function to create a non-ZIP file
    fn create_non_zip_file(path: &PathBuf) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        file.write_all(b"This is not a ZIP file")?;
        Ok(())
    }

    #[test]
    fn test_check_valid_zip() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("valid.zip");

        create_valid_zip(&zip_path).unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for valid ZIP file"),
        }
    }

    #[test]
    fn test_check_empty_zip() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("empty.zip");

        create_empty_zip(&zip_path).unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Empty ZIPs are valid
            }
            _ => panic!("Expected Valid status for empty ZIP file"),
        }
    }

    #[test]
    fn test_check_corrupted_zip() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("corrupted.zip");

        create_corrupted_zip(&zip_path).unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Corrupted(msg) => {
                assert!(msg.contains("Invalid zip format") || msg.contains("Cannot read"));
            }
            _ => panic!("Expected Corrupted status for corrupted ZIP file"),
        }
    }

    #[test]
    fn test_check_non_zip_file() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("notzip.zip");

        create_non_zip_file(&zip_path).unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Corrupted(msg) => {
                assert!(msg.contains("Invalid zip format"));
            }
            _ => panic!("Expected Corrupted status for non-ZIP file"),
        }
    }

    #[test]
    fn test_check_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("nonexistent.zip");

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Corrupted(msg) => {
                assert!(msg.contains("Cannot open file"));
            }
            _ => panic!("Expected Corrupted status for nonexistent file"),
        }
    }

    #[test]
    fn test_check_result_default() {
        let result = CheckResult::default();

        assert_eq!(result.total, 0);
        assert_eq!(result.valid, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.corrupted, 0);
    }

    #[test]
    fn test_check_result_counters() {
        let mut result = CheckResult::default();

        result.total = 10;
        result.valid = 7;
        result.corrupted = 2;
        result.skipped = 1;

        assert_eq!(result.total, 10);
        assert_eq!(result.valid, 7);
        assert_eq!(result.corrupted, 2);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.valid + result.corrupted + result.skipped, 10);
    }

    #[test]
    fn test_zip_with_multiple_files() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("multi.zip");

        // Create ZIP with multiple files
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options: FileOptions<()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        // Add multiple files
        for i in 0..10 {
            zip.start_file(&format!("file{}.txt", i), options).unwrap();
            zip.write_all(format!("Content {}", i).as_bytes()).unwrap();
        }

        zip.finish().unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for multi-file ZIP"),
        }
    }

    #[test]
    fn test_zip_with_nested_folders() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("nested.zip");

        // Create ZIP with nested folder structure
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options: FileOptions<()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        zip.start_file("folder1/file1.txt", options).unwrap();
        zip.write_all(b"File 1").unwrap();

        zip.start_file("folder1/folder2/file2.txt", options)
            .unwrap();
        zip.write_all(b"File 2").unwrap();

        zip.start_file("folder1/folder2/folder3/file3.txt", options)
            .unwrap();
        zip.write_all(b"File 3").unwrap();

        zip.finish().unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for nested folder ZIP"),
        }
    }

    #[test]
    fn test_large_zip_file() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("large.zip");

        // Create ZIP with larger content
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options: FileOptions<()> =
            FileOptions::default().compression_method(CompressionMethod::Deflated);

        // Add file with 1MB of data
        zip.start_file("large.txt", options).unwrap();
        let data = vec![b'A'; 1024 * 1024]; // 1MB
        zip.write_all(&data).unwrap();

        zip.finish().unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for large ZIP file"),
        }
    }

    #[test]
    fn test_zip_with_no_compression() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("stored.zip");

        // Create ZIP with stored (no compression) method
        let file = File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options: FileOptions<()> =
            FileOptions::default().compression_method(CompressionMethod::Stored);

        zip.start_file("stored.txt", options).unwrap();
        zip.write_all(b"Stored without compression").unwrap();

        zip.finish().unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for stored ZIP file"),
        }
    }

    #[test]
    fn test_print_summary_no_panic() {
        // Test that print_summary doesn't panic with various inputs
        let result = CheckResult {
            total: 100,
            valid: 80,
            corrupted: 15,
            skipped: 5,
        };

        // This should not panic
        print_summary(&result);
    }

    #[test]
    fn test_print_summary_zero_values() {
        let result = CheckResult::default();

        // This should not panic even with zeros
        print_summary(&result);
    }

    #[test]
    fn test_path_with_spaces() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("file with spaces.zip");

        create_valid_zip(&zip_path).unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for path with spaces"),
        }
    }

    #[test]
    fn test_path_with_special_chars() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("file-with_special.chars.zip");

        create_valid_zip(&zip_path).unwrap();

        let result = check_zip_file(&zip_path);

        match result {
            ZipFileStatus::Valid => {
                // Test passed
            }
            _ => panic!("Expected Valid status for path with special chars"),
        }
    }
}
