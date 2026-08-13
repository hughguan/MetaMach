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
        self.active_keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Returns key string for task if present.
    pub fn get_key(&self, task_id: &Uuid) -> Option<String> {
        let st = self.active_keys.lock().unwrap_or_else(|e| e.into_inner());
        st.get(task_id).map(|c| c.key.clone())
    }
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
                .unwrap_or_else(|e| e.into_inner())
                .insert(task_id, cred.clone());
            Ok(cred)
        })
    }

    fn revoke<'a>(&'a self, task_id: Uuid) -> BoxFut<'a, Result<()>> {
        Box::pin(async move {
            self.active_keys
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&task_id);
            Ok(())
        })
    }

    fn cleanup_sweep<'a>(&'a self, active_task_ids: &'a [Uuid]) -> BoxFut<'a, Result<usize>> {
        Box::pin(async move {
            let mut keys = self.active_keys.lock().unwrap_or_else(|e| e.into_inner());
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
