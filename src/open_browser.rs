//! Open the current worktree's Portless route in a new Chrome tab.

#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
use std::process::Command;

use crate::{discover, report, workspace};

pub fn run() -> i32 {
    let cwd = report::pane_cwd();
    let Some(found) = discover::find(&cwd) else {
        eprintln!("open-browser: the current pane is not inside a devcontainer project");
        return 1;
    };

    let mut workspaces = workspace::for_root(&found.project_root)
        .into_iter()
        .filter(|workspace| workspace.portless_alias.is_some())
        .collect::<Vec<_>>();
    workspaces
        .sort_by_key(|workspace| (workspace.engine != "devsy", workspace.provider != "docker"));

    if workspaces.is_empty() {
        eprintln!(
            "open-browser: no workspace URL is registered for {}",
            found.project_root.display()
        );
        eprintln!("Start one with bin/devco start docker or bin/devco start ssh.");
        return 1;
    }

    let mut stopped = Vec::new();
    let mut errors = Vec::new();
    for candidate in workspaces {
        let status = workspace::status(candidate);
        if status.status == "running" {
            let Some(alias) = status.workspace.portless_alias.as_deref() else {
                continue;
            };
            let Some(url) = workspace_url(alias) else {
                errors.push(format!("invalid Portless alias {alias:?}"));
                continue;
            };
            return launch_chrome(&url);
        }
        if status.status == "stopped" {
            stopped.push(status.workspace.id);
        } else if let Some(error) = status.error {
            errors.push(error);
        }
    }

    if !stopped.is_empty() {
        eprintln!("open-browser: no workspace is running for this project");
        eprintln!("Stopped workspace(s): {}", stopped.join(", "));
        eprintln!("Start one with bin/devco start docker or bin/devco start ssh.");
    } else {
        eprintln!("open-browser: could not get workspace status for this project");
        for error in errors {
            eprintln!("  {error}");
        }
    }
    1
}

fn workspace_url(alias: &str) -> Option<String> {
    if alias.is_empty()
        || !alias
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !alias.as_bytes()[0].is_ascii_alphanumeric()
        || !alias.as_bytes()[alias.len() - 1].is_ascii_alphanumeric()
    {
        return None;
    }
    Some(format!("https://{alias}.localhost"))
}

fn launch_chrome(url: &str) -> i32 {
    #[cfg(target_os = "macos")]
    let mut command = {
        let script = format!(
            r#"tell application "Google Chrome"
activate
if (count of windows) is 0 then make new window
tell front window
make new tab at end of tabs with properties {{URL:"{url}"}}
set active tab index to (count of tabs)
end tell
end tell"#
        );
        let mut command = Command::new("/usr/bin/osascript");
        command.args(["-e", &script]);
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let Some(chrome) = chrome_path() else {
            eprintln!("open-browser: Google Chrome or Chromium was not found on PATH");
            return 1;
        };
        let mut command = Command::new(chrome);
        command.args(["--new-tab", url]);
        command
    };

    match command.spawn() {
        Ok(_) => {
            println!("Opening workspace in Chrome: {url}");
            0
        }
        Err(error) => {
            eprintln!("open-browser: could not open Chrome: {error}");
            1
        }
    }
}

#[cfg(target_os = "linux")]
fn chrome_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ]
    .into_iter()
    .find_map(|name| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(name))
            .find(|candidate| {
                std::fs::metadata(candidate)
                    .map(|metadata| {
                        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                    })
                    .unwrap_or(false)
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_workspace_url_from_a_portless_alias() {
        assert_eq!(
            workspace_url("feature-x-ssh"),
            Some("https://feature-x-ssh.localhost".to_string())
        );
    }

    #[test]
    fn rejects_aliases_that_are_not_launcher_generated() {
        for alias in ["", "-bad", "bad-", "bad/path", "bad.example"] {
            assert_eq!(workspace_url(alias), None, "alias: {alias:?}");
        }
    }
}
