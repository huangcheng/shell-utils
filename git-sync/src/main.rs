mod cli;

use clap::Parser;
use cli::Cli;
use colour::*;
use shellexpand::tilde;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{available_parallelism, spawn, JoinHandle};
use which::which;

fn check_git_exists() -> bool {
    which("git").is_ok()
}

fn pull_repo(repo: &PathBuf) {
    use std::process::{Command, Stdio};

    // Safe handling - use repo path directly if parent extraction fails
    let parent = match repo.parent() {
        Some(p) => p,
        None => {
            red!("❌ [Error] {:?} - No parent directory\n", repo);
            return;
        }
    };

    let path = match repo.strip_prefix(parent) {
        Ok(p) => p,
        Err(_) => {
            // Fallback to full path if strip fails
            repo.as_path()
        }
    };

    // Show progress BEFORE starting the git operation
    // yellow!("🔄 [Updating] {:?}...\n", path);

    // Set GIT_TERMINAL_PROMPT=0 to prevent git from prompting for credentials
    // This will cause git to fail immediately if authentication is required
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("pull")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "echo") // Additional safety: return empty on password prompt
        .env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        ) // Skip SSH prompts
        .stdin(Stdio::null()) // Prevent any stdin prompts
        .stdout(Stdio::null())
        .stderr(Stdio::piped()) // Capture stderr to detect auth errors
        .output();

    match output {
        Ok(output) => {
            if output.status.success() {
                // println!("✅ Successfully pulled repository at {:?}", path);
                green!("✅ [Updated] {:?}\n", path);
            } else {
                // Convert stderr to string to check for authentication errors
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stderr_lower = stderr.to_lowercase();

                // Check for common authentication-related error messages
                let is_auth_error = stderr_lower.contains("authentication failed")
                    || stderr_lower.contains("could not read username")
                    || stderr_lower.contains("could not read password")
                    || stderr_lower.contains("terminal prompts disabled")
                    || stderr_lower.contains("credentials")
                    || stderr_lower.contains("access denied")
                    || stderr_lower.contains("permission denied");

                let exit_code = output.status.code().unwrap_or(-1);

                if is_auth_error || exit_code == 128 {
                    // Authentication required - skip with info message
                    // println!("⏭️  Skipped {:?} (authentication required)", path);
                    yellow!("⏭️ [Skipped - Auth Required] {:?}\n", path);
                } else if stderr_lower.contains("already up to date")
                    || stderr_lower.contains("already up-to-date")
                {
                    // Repository is already up to date (not an error)
                    // println!("✅ Repository {:?} is already up to date", path);
                    blue!("✅ [Up to Date] {:?}\n", path);
                } else {
                    // Other error
                    // eprintln!(
                    //     "❌ Failed to pull repository at {:?}. Exit status: {}",
                    //     path, output.status
                    // );

                    red!("❌ [Error] {:?}\n", path);
                    // if !stderr.is_empty() {
                    //     eprintln!("   Error: {}", stderr.trim());
                    // }
                }
            }
        }
        Err(_e) => {
            // eprintln!("❌ Error executing git pull for {:?}: {}", path, e);
            red!("❌ [Error] {:?}\n", path);
        }
    }
}

fn find_git_repos(dir: &PathBuf, git_repos: Arc<Mutex<Vec<PathBuf>>>) {
    find_git_repos_recursive(dir, git_repos, 0);
}

fn find_git_repos_recursive(dir: &PathBuf, git_repos: Arc<Mutex<Vec<PathBuf>>>, depth: usize) {
    // Prevent infinite recursion and stack overflow
    const MAX_DEPTH: usize = 50;

    if depth > MAX_DEPTH {
        eprintln!("Warning: Maximum recursion depth reached at {:?}", dir);
        return;
    }

    // Skip if it's a symlink to prevent infinite loops
    if dir.is_symlink() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();

            // Skip symlinks
            if path.is_symlink() {
                continue;
            }

            if path.is_dir() {
                if path.join(".git").is_dir() {
                    {
                        let mut repos = git_repos.lock().unwrap();
                        repos.push(path.clone());
                    }
                } else {
                    find_git_repos_recursive(&path, Arc::clone(&git_repos), depth + 1);
                }
            }
        }
    } else {
        eprintln!("Failed to read directory: {:?}", dir);
    }
}

fn main() {
    if !check_git_exists() {
        red!("Error: 'git' command not found. Please install Git to use this tool.\n");
        std::process::exit(1);
    }

    let args = Cli::parse();

    let path = args
        .path
        .unwrap_or_else(|| env::current_dir().expect("Failed to get current directory"));

    let path = if path.starts_with("~") {
        PathBuf::from(tilde(&path.to_string_lossy()).to_string())
    } else {
        path
    };

    let mutex = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let _mutex = Arc::clone(&mutex);

    let number_of_threads = available_parallelism().map(|n| n.get()).unwrap_or(1);

    let path_clone = path.clone();
    let thread = spawn(move || {
        find_git_repos(&path_clone, Arc::clone(&mutex));
    });

    thread.join().unwrap();

    let total_repos = {
        let repos = _mutex.lock().unwrap();
        repos.len()
    };

    if total_repos == 0 {
        yellow!("No Git repositories found in {:?}\n", path);
        return;
    }

    green!("Found {} repositories. Starting sync...\n\n", total_repos);

    let mut threads: Vec<JoinHandle<()>> = Vec::new();

    for _ in 0..number_of_threads {
        let mutex = Arc::clone(&_mutex);

        threads.push(spawn(move || {
            loop {
                let repo_path = {
                    let mut repos = mutex.lock().unwrap();
                    repos.pop()
                };

                match repo_path {
                    Some(path) => {
                        pull_repo(&path);
                    }
                    None => break,
                }
            }
        }));
    }

    for thread in threads {
        // Handle potential panics in worker threads gracefully
        if let Err(e) = thread.join() {
            eprintln!("Warning: A worker thread panicked: {:?}", e);
        }
    }

    green!("\n✅ All repositories synced!\n");
}
