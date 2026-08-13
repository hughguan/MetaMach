# ADR-036: Pluggable Credential Provisioning & Herdr Harvest Pipeline

* **Status:** 🔄 Phase 1 & Phase 2 Harvest Engine Implemented / TUI Keybindings Spec'd Only (0.7.0)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Amends:** ADR-010 (Cognitive Provider SPI), ADR-019 (Configurable Agents - Provisioning, Quota & Fallback)
* **Depends On:** ADR-033 (Sandbox Track) - for Phase 2 Harvest Pipeline only

## 1. Context & Problem Statement

Agents require dynamic, scoped credentials to interact with external systems. Storing long-lived credentials in plaintext or environment variables is a security risk. Furthermore, we need a mechanism to securely harvest outputs from sandbox environments.

We need a standardized Service Provider Interface (SPI) for dynamic credential provisioning, and a subsequent pipeline (Harvest) to securely extract data.

## 2. Options Considered

1. **Environment Variables**: Simple, but long-lived and insecure for dynamic task execution.
2. **Hardcoded Credential Managers**: Tightly coupling to AWS STS or HashiCorp Vault. Too rigid for our diverse deployment environments.
3. **Pluggable Credential SPI & Harvest Pipeline (Chosen)**: A custom SPI for credential generation, allowing different backends, combined with a secure harvest pipeline for sandboxed data. This provides maximum flexibility and security.

## 3. Decision

We will implement Option 3. To accommodate dependencies, this initiative is explicitly split into two sequenced phases:

1. **Phase 1: Credential SPI**: An independent, standalone module for credential provisioning that can be shipped immediately.
2. **Phase 2: Herdr Harvest Pipeline**: A data extraction pipeline that strictly depends on the sandbox track established in ADR-033. Phase 2 cannot ship until ADR-033 is finalized.

## 4. Detailed Specification

### Phase 1: Credential SPI

The Credential SPI will be placed as a top-level module at `janus/src/credential.rs` (distinct from the `janus/src/cognitive/` module).

It will utilize the `BoxFut` pattern consistent with the existing codebase conventions (e.g. `DurableEngine` in `src/absurd/adapter.rs`):

```rust
use std::future::Future;
use std::pin::Pin;
use anyhow::Result;
use uuid::Uuid;
use chrono::{DateTime, Utc};

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct Credential {
    pub key: String,
    pub secret: String,
    pub token: Option<String>,
    pub expires_at: DateTime<Utc>,
}

pub trait CredentialProvider: Send + Sync {
    fn provision<'a>(
        &'a self,
        task_id: Uuid,
        scopes: &'a [String],
    ) -> BoxFut<'a, Result<Credential>>;

    fn revoke<'a>(
        &'a self,
        task_id: Uuid,
    ) -> BoxFut<'a, Result<()>>;
}
```

#### Crash Recovery & Orphaned Keys

If the daemon crashes mid-task, provisioned credentials might become orphaned and fail to revoke. To handle this, we will introduce a startup sweep and key Time-To-Live (TTL). 

During daemon cold-start reconciliation (in `src/coldstart.rs`), the system will:
1. Scan for all active credential records.
2. Check the running status of their associated `task_id`.
3. Revoke any credentials whose associated task is no longer running or has exceeded its TTL.

### Phase 2: Herdr Harvest Pipeline

*(Note: This phase requires ADR-033 to be completed.)*

The Harvest Pipeline will securely extract artifacts from the sandbox once the task completes, utilizing the scoped credentials provisioned by the SPI to upload artifacts to durable storage and capturing diffs as `refs/sandbox/*`.

## 5. Consequences

* **Positive**: Agents can operate with short-lived, least-privilege credentials.
* **Positive**: Clean separation between credential provisioning and sandbox execution.
* **Positive**: Resilient to daemon crashes with explicit cold-start cleanup.
* **Negative**: Adds complexity to daemon initialization (`coldstart.rs`).
