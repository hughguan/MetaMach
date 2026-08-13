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
    // Capture actual working tree diff (tracked + untracked files) into a tree commit
    let _ = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo_root)
        .output();

    let tree_sha = match std::process::Command::new("git")
        .args(["write-tree"])
        .current_dir(repo_root)
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    };

    let commit_sha = if !tree_sha.is_empty() {
        match std::process::Command::new("git")
            .args(["commit-tree", &tree_sha, "-p", "HEAD", "-m", commit_msg])
            .current_dir(repo_root)
            .output()
        {
            Ok(out) if out.status.success() => {
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
            _ => String::new(),
        }
    } else {
        String::new()
    };

    let _ = std::process::Command::new("git")
        .args(["reset"])
        .current_dir(repo_root)
        .output();

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
    let ref_name = format!("refs/sandbox/{}-{step_name}", task_id.simple());
    let msg = format!("metamach harvest snapshot for step '{step_name}'");
    snapshot_working_tree_to_ref(repo_root, &ref_name, &msg)?;
    Ok(ref_name)
}

/// Applies a harvested sandbox ref by full ref name to the working tree.
pub fn apply_harvest_ref_by_name(repo_root: &Path, ref_name: &str) -> Result<bool> {
    let out = std::process::Command::new("git")
        .args(["checkout", ref_name, "--", "."])
        .current_dir(repo_root)
        .output()?;

    // Check out all files present in ref_name's commit tree (including paths not in current index)
    let ls_out = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", ref_name])
        .current_dir(repo_root)
        .output();

    if let Ok(ls) = ls_out
        && ls.status.success()
    {
        let files: Vec<&str> = std::str::from_utf8(&ls.stdout)
            .unwrap_or("")
            .lines()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if !files.is_empty() {
            let mut cmd = std::process::Command::new("git");
            cmd.arg("checkout")
                .arg(ref_name)
                .arg("--")
                .args(&files)
                .current_dir(repo_root);
            let _ = cmd.output();
        }
    }

    if !out.status.success() {
        anyhow::bail!(
            "Failed to apply harvest ref {}: {}",
            ref_name,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(true)
}

/// Applies a harvested sandbox ref (`refs/sandbox/<task_id>-<step_name>`) to the working tree.
pub fn apply_harvest_ref(repo_root: &Path, task_id: Uuid, step_name: &str) -> Result<()> {
    let ref_name = format!("refs/sandbox/{}-{step_name}", task_id.simple());
    apply_harvest_ref_by_name(repo_root, &ref_name).map(|_| ())
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

/// Creates an isolated Git worktree for sandboxed step execution at `worktree_dir` (ADR-033 Phase 2).
pub fn create_sandbox_worktree(
    repo_root: &Path,
    worktree_dir: &Path,
    task_id: Uuid,
    step_name: &str,
) -> Result<()> {
    if worktree_dir.exists() {
        let _ = std::fs::remove_dir_all(worktree_dir);
    }
    let branch_name = format!("sandbox-{}-{step_name}", task_id.simple());
    let out = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            &branch_name,
            worktree_dir.to_str().unwrap_or(""),
            "HEAD",
        ])
        .current_dir(repo_root)
        .output()?;

    if !out.status.success() {
        // Fallback: detach worktree if branch already exists
        let out_detach = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "--detach",
                worktree_dir.to_str().unwrap_or(""),
                "HEAD",
            ])
            .current_dir(repo_root)
            .output()?;
        if !out_detach.status.success() {
            anyhow::bail!(
                "Failed to create sandbox worktree at {}: {}",
                worktree_dir.display(),
                String::from_utf8_lossy(&out_detach.stderr)
            );
        }
    }

    Ok(())
}

/// Cleans up an isolated Git worktree and its associated sandbox branch.
pub fn cleanup_sandbox_worktree(
    repo_root: &Path,
    worktree_dir: &Path,
    task_id: Uuid,
    step_name: &str,
) -> Result<()> {
    let branch_name = format!("sandbox-{}-{step_name}", task_id.simple());

    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_dir.to_str().unwrap_or(""),
        ])
        .current_dir(repo_root)
        .output();

    let _ = std::process::Command::new("git")
        .args(["branch", "-D", &branch_name])
        .current_dir(repo_root)
        .output();

    if worktree_dir.exists() {
        let _ = std::fs::remove_dir_all(worktree_dir);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_temp_git_repo() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let repo_dir = temp_dir.path();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(repo_dir)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.name", "MetaMach Test"])
            .current_dir(repo_dir)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["config", "user.email", "test@metamach.internal"])
            .current_dir(repo_dir)
            .output()
            .unwrap();

        std::fs::write(repo_dir.join("README.md"), "# Test Repo").unwrap();

        std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(repo_dir)
            .output()
            .unwrap();

        std::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(repo_dir)
            .output()
            .unwrap();

        temp_dir
    }

    #[test]
    fn test_snapshot_and_list_harvest_refs() {
        let repo = setup_temp_git_repo();
        let task_id = Uuid::new_v4();

        // Modify file
        std::fs::write(repo.path().join("README.md"), "# Modified Test Repo").unwrap();
        // Add untracked file
        std::fs::write(repo.path().join("output.txt"), "harvest result").unwrap();

        let ref_name = harvest_sandbox_output(repo.path(), task_id, "build").unwrap();
        assert!(ref_name.starts_with("refs/sandbox/"));

        let refs = list_harvest_refs(repo.path()).unwrap();
        assert!(refs.contains(&ref_name));

        // Clean working tree
        std::process::Command::new("git")
            .args(["reset", "--hard", "HEAD"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        std::fs::remove_file(repo.path().join("output.txt")).ok();

        // Apply harvest ref
        apply_harvest_ref(repo.path(), task_id, "build").unwrap();
        assert!(repo.path().join("output.txt").exists());
    }
}
