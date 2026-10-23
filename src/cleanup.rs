//! Delegate full worktree cleanup to the project launcher.

use std::process::Command;

use crate::{discover, report};

pub fn run() -> i32 {
    let cwd = report::pane_cwd();
    let Some(found) = discover::find(&cwd) else {
        eprintln!("cleanup-worktree: the current pane is not inside a devcontainer project");
        return 1;
    };

    let Some(worktree_path) = found.project_root.to_str() else {
        eprintln!("cleanup-worktree: worktree path is not UTF-8");
        return 1;
    };
    let output = Command::new("git")
        .args([
            "-C",
            worktree_path,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output();
    let common_dir = match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        Ok(output) => {
            eprintln!(
                "cleanup-worktree: cannot find the shared Git directory: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return 1;
        }
        Err(error) => {
            eprintln!("cleanup-worktree: cannot run git: {error}");
            return 1;
        }
    };
    let Some(project_root) = std::path::Path::new(&common_dir).parent() else {
        eprintln!("cleanup-worktree: cannot find the main project directory");
        return 1;
    };
    let launcher = project_root.join("bin/devco");
    let output = Command::new(&launcher)
        .args(["cleanup", worktree_path])
        .current_dir(project_root)
        .output();
    match output {
        Ok(output) if output.status.success() => {
            print!("{}", String::from_utf8_lossy(&output.stdout));
            0
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            if !stdout.trim().is_empty() {
                eprint!("{stdout}");
            }
            eprintln!("cleanup-worktree: {}", stderr.trim());
            1
        }
        Err(error) => {
            eprintln!(
                "cleanup-worktree: cannot run {}: {error}",
                launcher.display()
            );
            1
        }
    }
}
