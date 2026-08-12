//! Pluggable Credential Provisioning SPI (ADR-036 Phase 1).
//!
//! Provides a vendor-agnostic Service Provider Interface (SPI) for dynamic, scoped
//! API key and credential generation, automatic revocation, and cold-start cleanup.
//!
//! Conforms to `BoxFut` async conventions used across `janus::absurd::adapter`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Re-export manual Pin<Box<dyn Future>> alias matching codebase conventions.
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Dynamic, scoped credential payload issued for task execution.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Credential {
    /// Associated task UUID.
    pub task_id: Uuid,
    /// Public credential identifier / API Key string.
    pub key: String,
    /// Secret key or access token.
    pub secret: String,
    /// Optional bearer or session token.
    pub token: Option<String>,
    /// Expiration timestamp in UTC.
    pub expires_at: DateTime<Utc>,
    /// Allowed authorization scopes.
    pub scopes: Vec<String>,
}

impl Credential {
    /// Returns true if the credential has passed its expiration timestamp.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

/// Service Provider Interface for pluggable credential management.
pub trait CredentialProvider: Send + Sync {
    /// Provisions a new short-lived, scoped credential for `task_id`.
    fn provision<'a>(
        &'a self,
        task_id: Uuid,
        scopes: &'a [String],
        ttl_seconds: i64,
    ) -> BoxFut<'a, Result<Credential>>;

    /// Revokes an active credential associated with `task_id`.
    fn revoke<'a>(&'a self, task_id: Uuid) -> BoxFut<'a, Result<()>>;

    /// Performs a cleanup sweep revoking expired or orphaned credentials.
    fn cleanup_sweep<'a>(&'a self, active_task_ids: &'a [Uuid]) -> BoxFut<'a, Result<usize>>;
}

/// No-op fallback credential provider that issues mock environment credentials.
#[derive(Debug, Default)]
pub struct NoopCredentialProvider;

impl CredentialProvider for NoopCredentialProvider {
    fn provision<'a>(
        &'a self,
        task_id: Uuid,
        scopes: &'a [String],
        ttl_seconds: i64,
    ) -> BoxFut<'a, Result<Credential>> {
        Box::pin(async move {
            let now = Utc::now();
            let expires_at = now + chrono::Duration::seconds(ttl_seconds);
            Ok(Credential {
                task_id,
                key: format!("noop-key-{}", task_id.simple()),
                secret: format!("noop-secret-{}", task_id.simple()),
                token: None,
                expires_at,
                scopes: scopes.to_vec(),
            })
        })
    }

    fn revoke<'a>(&'a self, _task_id: Uuid) -> BoxFut<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn cleanup_sweep<'a>(&'a self, _active_task_ids: &'a [Uuid]) -> BoxFut<'a, Result<usize>> {
        Box::pin(async move { Ok(0) })
    }
}

/// Thread-safe in-memory credential provider for local testing and task tracking.
#[derive(Debug, Default, Clone)]
pub struct MemoryCredentialProvider {
    active_keys: Arc<Mutex<HashMap<Uuid, Credential>>>,
}

impl MemoryCredentialProvider {
    pub fn new() -> Self {
        Self {
            active_keys: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active_keys.lock().unwrap().len()
    }

    /// Returns key string for task if present.
    pub fn get_key(&self, task_id: &Uuid) -> Option<String> {
        let st = self.active_keys.lock().expect("mutex lock");
        st.get(task_id).map(|c| c.key.clone())
    }
}

// =========================================================================
// Phase 2: Herdr Harvest Pipeline (ADR-036 Phase 2)
// =========================================================================

/// Captures sandbox workspace changes and stores them under Git ref `refs/sandbox/<task_id>-<step_name>`.
/// Returns the full ref name (e.g. `refs/sandbox/<task_id>-<step_name>`) on success.
pub fn harvest_sandbox_output(
    repo_root: &std::path::Path,
    task_id: Uuid,
    step_name: &str,
) -> Result<String> {
    let ref_name = format!("refs/sandbox/{}-{}", task_id.simple(), step_name);

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
        let msg = format!("metamach harvest snapshot for step '{}'", step_name);
        let commit = if !tree_sha.is_empty() {
            match std::process::Command::new("git")
                .args(["commit-tree", &tree_sha, "-p", "HEAD", "-m", &msg])
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
        .args(["update-ref", &ref_name, &target_ref])
        .current_dir(repo_root)
        .output()?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to create harvest ref {}: {}",
            ref_name,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(ref_name)
}

/// Merges a harvested sandbox ref (`refs/sandbox/<task_id>-<step_name>`) into `HEAD`.
pub fn merge_harvest_ref(
    repo_root: &std::path::Path,
    task_id: Uuid,
    step_name: &str,
) -> Result<()> {
    let ref_name = format!("refs/sandbox/{}-{}", task_id.simple(), step_name);

    let out = std::process::Command::new("git")
        .args(["checkout", &ref_name, "--", "."])
        .current_dir(repo_root)
        .output()?;

    if !out.status.success() {
        anyhow::bail!(
            "Failed to merge harvest ref {}: {}",
            ref_name,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    Ok(())
}

/// Lists all harvested sandbox refs under `refs/sandbox/*`.
pub fn list_harvest_refs(repo_root: &std::path::Path) -> Result<Vec<String>> {
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

impl CredentialProvider for MemoryCredentialProvider {
    fn provision<'a>(
        &'a self,
        task_id: Uuid,
        scopes: &'a [String],
        ttl_seconds: i64,
    ) -> BoxFut<'a, Result<Credential>> {
        Box::pin(async move {
            let expires_at = Utc::now() + chrono::Duration::seconds(ttl_seconds);
            let cred = Credential {
                task_id,
                key: format!("mem-key-{}", task_id.simple()),
                secret: format!("mem-secret-{}", task_id.simple()),
                token: Some("mem-token".to_string()),
                expires_at,
                scopes: scopes.to_vec(),
            };
            self.active_keys
                .lock()
                .unwrap()
                .insert(task_id, cred.clone());
            Ok(cred)
        })
    }

    fn revoke<'a>(&'a self, task_id: Uuid) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            self.active_keys.lock().unwrap().remove(&task_id);
            Ok(())
        })
    }

    fn cleanup_sweep<'a>(&'a self, active_task_ids: &'a [Uuid]) -> BoxFut<'a, Result<usize>> {
        Box::pin(async move {
            let mut keys = self.active_keys.lock().unwrap();
            let now = Utc::now();
            let mut revoked = 0usize;
            keys.retain(|id, cred| {
                let is_active_task = active_task_ids.contains(id);
                let is_valid_ttl = now <= cred.expires_at;
                let keep = is_active_task && is_valid_ttl;
                if !keep {
                    revoked += 1;
                }
                keep
            });
            Ok(revoked)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_credential_provider_lifecycle() {
        let provider = MemoryCredentialProvider::new();
        let task_1 = Uuid::new_v4();
        let task_2 = Uuid::new_v4();

        // Provision task 1 key with 600s TTL
        let cred_1 = provider
            .provision(task_1, &["read".into(), "write".into()], 600)
            .await
            .unwrap();
        assert_eq!(cred_1.task_id, task_1);
        assert!(!cred_1.is_expired());
        assert_eq!(provider.active_count(), 1);

        // Provision task 2 key with -1s TTL (expired immediately)
        let _cred_2 = provider
            .provision(task_2, &["read".into()], -1)
            .await
            .unwrap();
        assert_eq!(provider.active_count(), 2);

        // Cleanup sweep passing only task_1 as active
        let revoked = provider.cleanup_sweep(&[task_1]).await.unwrap();
        assert_eq!(revoked, 1);
        assert_eq!(provider.active_count(), 1);
        assert!(provider.get_key(&task_1).is_some());
        assert!(provider.get_key(&task_2).is_none());

        // Revoke task 1 explicitly
        provider.revoke(task_1).await.unwrap();
        assert_eq!(provider.active_count(), 0);
    }
}
