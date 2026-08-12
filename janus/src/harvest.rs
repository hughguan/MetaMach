//! Herdr Harvest Pipeline (ADR-036 Phase 2).
//!
//! Provides utilities for capturing sandbox workspace changes into Git refs (`refs/sandbox/*`
//! and `refs/metamach/rollback/*`), listing harvested refs, and applying harvested refs
//! back to working tree `HEAD`.

use anyhow::Result;
use std::path::Path;
use uuid::Uuid;

/// Snapshots current working tree diffs (including untracked files) to a specified Git ref.
/// Returns the commit SHA pointing to the created snapshot.
pub fn snapshot_working_tree_to_ref(
    repo_root: &Path,
    ref_name: &str,
    commit_msg: &str,
) -> Result<String> {
    // Capture actual working tree diff (including untracked files) into a stash commit
    let stash_out = std::process::Command::new("git")
        .args(["stash", "create", "-u"])
        .current_dir(repo_root)
        .output();

    let commit_sha = match stash_out {
        Ok(out) if out.status.success() && !out.stdout.is_empty() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => String::new(),
    };

    let commit_sha = if !commit_sha.is_empty() {
        commit_sha
    } else {
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(repo_root)
            .output();
        let tree_sha = match std::process::Command::new("git")
            .args(["write-tree"])
            .current_dir(repo_root)
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(_) => String::new(),
        };
        let commit = if !tree_sha.is_empty() {
            match std::process::Command::new("git")
                .args(["commit-tree", &tree_sha, "-p", "HEAD", "-m", commit_msg])
                .current_dir(repo_root)
                .output()
            {
                Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
                Err(_) => String::new(),
            }
        } else {
            String::new()
        };
        let _ = std::process::Command::new("git")
            .args(["reset"])
            .current_dir(repo_root)
            .output();
        commit
    };

    let target_ref = if !commit_sha.is_empty() {
        commit_sha
    } else {
        "HEAD".to_string()
    };

    let out = std::process::Command::new("git")
        .args(["update-ref", ref_name, &target_ref])
        .current_dir(repo_root)
        .output()?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to update ref {}: {}",
            ref_name,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(target_ref)
}

/// Captures sandbox workspace changes and stores them under Git ref `refs/sandbox/<task_id>-<step_name>`.
/// Returns the full ref name (e.g. `refs/sandbox/<task_id>-<step_name>`) on success.
pub fn harvest_sandbox_output(repo_root: &Path, task_id: Uuid, step_name: &str) -> Result<String> {
    let ref_name = format!("refs/sandbox/{}-{}", task_id.simple(), step_name);
    let msg = format!("metamach harvest snapshot for step '{}'", step_name);
    snapshot_working_tree_to_ref(repo_root, &ref_name, &msg)?;
    Ok(ref_name)
}

/// Applies a harvested sandbox ref (`refs/sandbox/<task_id>-<step_name>`) to the working tree.
pub fn apply_harvest_ref(repo_root: &Path, task_id: Uuid, step_name: &str) -> Result<()> {
    let ref_name = format!("refs/sandbox/{}-{}", task_id.simple(), step_name);

    let out = std::process::Command::new("git")
        .args(["checkout", &ref_name, "--", "."])
        .current_dir(repo_root)
        .output()?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to apply harvest ref {}: {}",
            ref_name,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

/// Lists all harvested sandbox refs under `refs/sandbox/*`.
pub fn list_harvest_refs(repo_root: &Path) -> Result<Vec<String>> {
    let out = std::process::Command::new("git")
        .args(["for-each-ref", "--format=%(refname)", "refs/sandbox/*"])
        .current_dir(repo_root)
        .output()?;

    if !out.status.success() {
        return Ok(Vec::new());
    }

    let refs = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(refs)
}
