//! Open the current pane's launcher-managed Devsy workspace in Zed.

use std::process::Command;

use crate::{discover, report, workspace};

const ZED_WORKSPACE_FOLDER: &str = "/workspaces/platform";

pub fn run() -> i32 {
    let cwd = report::pane_cwd();
    let Some(found) = discover::find(&cwd) else {
        eprintln!("open-zed: the current pane is not inside a devcontainer project");
        return 1;
    };

    let workspaces = workspace::for_root(&found.project_root)
        .into_iter()
        .filter(|workspace| workspace.engine == "devsy")
        .collect::<Vec<_>>();
    if workspaces.is_empty() {
        eprintln!(
            "open-zed: no Devsy workspace is registered for {}",
            found.project_root.display()
        );
        eprintln!("Start one with bin/devco start docker or bin/devco start ssh.");
        return 1;
    }

    // `for_root` sorts by provider. Prefer a running Docker workspace if both
    // Docker and SSH workspaces are running.
    let mut stopped = Vec::new();
    let mut errors = Vec::new();
    for workspace in workspaces {
        let status = workspace::status(workspace);
        if status.status == "running" {
            let url = zed_url(&status.workspace.id);
            return launch(&url);
        }
        if status.status == "stopped" {
            stopped.push(status.workspace.id);
        } else if let Some(error) = status.error {
            errors.push(error);
        }
    }

    if !stopped.is_empty() {
        eprintln!("open-zed: no Devsy workspace is running for this project");
        eprintln!("Stopped workspace(s): {}", stopped.join(", "));
        eprintln!("Start one with bin/devco start docker or bin/devco start ssh.");
    } else {
        eprintln!("open-zed: could not get Devsy status for this project");
        for error in errors {
            eprintln!("  {error}");
        }
    }
    1
}

fn zed_url(workspace_id: &str) -> String {
    format!("zed://ssh/{workspace_id}.devsy{ZED_WORKSPACE_FOLDER}")
}

fn launch(url: &str) -> i32 {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match Command::new(opener).arg(url).spawn() {
        Ok(_) => {
            println!("Opening Devsy workspace in Zed: {url}");
            0
        }
        Err(error) => {
            eprintln!("open-zed: could not launch {opener}: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_zed_uri_used_by_the_devco_launcher() {
        assert_eq!(
            zed_url("feature-a-1234-ssh"),
            "zed://ssh/feature-a-1234-ssh.devsy/workspaces/platform"
        );
    }
}
