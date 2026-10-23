//! Workspace discovery for worktrees started by `bin/devco`.
//!
//! The state files written by the project launcher map a host worktree to its
//! workspace manager, id, and provider. Devsy or legacy DevPod supplies live state.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub engine: String,
    pub id: String,
    pub provider: String,
    pub root: PathBuf,
    pub portless_alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceStatus {
    pub workspace: Workspace,
    /// "running", "stopped", or "error".
    pub status: &'static str,
    pub error: Option<String>,
}

/// Find launcher-managed workspaces rooted at `root`.
pub fn for_root(root: &Path) -> Vec<Workspace> {
    let Some(state_dir) = state_dir() else {
        return Vec::new();
    };
    let wanted = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let Ok(entries) = std::fs::read_dir(state_dir) else {
        return Vec::new();
    };

    let mut workspaces = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("env") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        let fields = parse_env(&contents);
        let Some(workspace_root) = fields.get("WORKSPACE") else {
            continue;
        };
        let workspace_root = PathBuf::from(workspace_root);
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        if workspace_root != wanted {
            continue;
        }
        let Some(provider) = fields.get("PROVIDER") else {
            continue;
        };
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        workspaces.push(Workspace {
            engine: fields
                .get("ENGINE")
                .cloned()
                .unwrap_or_else(|| "devpod".to_string()),
            id: id.to_string(),
            provider: provider.to_string(),
            root: workspace_root,
            portless_alias: fields
                .get("PORTLESS_ALIAS")
                .filter(|alias| !alias.is_empty())
                .cloned(),
        });
    }
    workspaces.sort_by(|a, b| {
        a.engine
            .cmp(&b.engine)
            .then(a.provider.cmp(&b.provider))
            .then(a.id.cmp(&b.id))
    });
    workspaces
}

pub fn workspace_for(root: &Path, provider: &str, engine: &str) -> Option<Workspace> {
    for_root(root)
        .into_iter()
        .find(|workspace| workspace.engine == engine && workspace.provider == provider)
}

/// Ask the workspace manager for the live state of a known workspace.
pub fn status(workspace: Workspace) -> WorkspaceStatus {
    let engine = workspace.engine.clone();
    let Some(cli) = cli_path(&engine) else {
        return WorkspaceStatus {
            workspace,
            status: "error",
            error: Some(format!("{engine} CLI not found on PATH")),
        };
    };
    let args = if engine == "devsy" {
        vec![
            "workspace",
            "status",
            &workspace.id,
            "--provider",
            &workspace.provider,
            "--timeout",
            "5s",
            "--result-format",
            "json",
        ]
    } else {
        vec![
            "--provider",
            &workspace.provider,
            "status",
            "--timeout",
            "5s",
            "--output",
            "json",
            &workspace.id,
        ]
    };
    let output = Command::new(cli).args(args).output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            return WorkspaceStatus {
                workspace,
                status: "error",
                error: Some(format!("cannot run {engine}: {error}")),
            };
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let state = serde_json::from_str::<serde_json::Value>(&stdout)
        .ok()
        .and_then(|value| {
            value
                .get("state")
                .and_then(|s| s.as_str())
                .map(str::to_owned)
        })
        .map(|state| state.to_ascii_lowercase());
    match state.as_deref() {
        Some("running") => WorkspaceStatus {
            workspace,
            status: "running",
            error: None,
        },
        Some("stopped" | "exited") => WorkspaceStatus {
            workspace,
            status: "stopped",
            error: None,
        },
        Some(other) => WorkspaceStatus {
            workspace,
            status: "error",
            error: Some(format!("unexpected {engine} state: {other}")),
        },
        None => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            WorkspaceStatus {
                workspace,
                status: "error",
                error: Some(if stderr.is_empty() {
                    format!("{engine} returned no workspace state")
                } else {
                    stderr
                }),
            }
        }
    }
}

pub fn cli_path(engine: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let from_path = std::env::split_paths(&path)
        .map(|directory| directory.join(engine))
        .find(|candidate| is_executable(candidate));
    from_path.or_else(|| {
        [
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(format!(".local/bin/{engine}")),
            PathBuf::from(format!("/opt/homebrew/bin/{engine}")),
            PathBuf::from(format!("/usr/local/bin/{engine}")),
        ]
        .into_iter()
        .find(|candidate| is_executable(candidate))
    })
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn state_dir() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(root.join("revaesor/devco"))
}

fn parse_env(contents: &str) -> std::collections::HashMap<String, String> {
    contents
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_launcher_env_without_splitting_paths_on_equals() {
        let fields = parse_env(
            "ENGINE=devsy\nPROVIDER=ssh\nWORKSPACE=/tmp/a=b/c\nPORTLESS_ALIAS=feature-ssh\n",
        );
        assert_eq!(fields.get("ENGINE").map(String::as_str), Some("devsy"));
        assert_eq!(fields.get("PROVIDER").map(String::as_str), Some("ssh"));
        assert_eq!(
            fields.get("PORTLESS_ALIAS").map(String::as_str),
            Some("feature-ssh")
        );
        assert_eq!(
            fields.get("WORKSPACE").map(String::as_str),
            Some("/tmp/a=b/c")
        );
    }
}
