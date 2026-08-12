# ADR-036: Pluggable Credential Provisioning & Herdr Harvest Pipeline

* **Status:** Proposed (0.7.0 Candidate)
* **Date:** 2026-08-12
* **Target Version:** MetaMach 0.7.0
* **Author:** MetaMach Architecture Group
* **Amends/Extends:** ADR-019 (Agent Provisioning), ADR-029 (.janus Directory Consolidation), ADR-033 (Dual-Track Isolation)

---

## 1. Context & Problem Statement

Running concurrent software sandboxes (ADR-033) introduces two security and workflow requirements:
1. **Credential Exposure Risk**: Passing long-lived host API keys directly to untrusted or isolated sandbox containers risks key leaks and unbudgeted API overuse.
2. **Branch Pollution**: Parallel Best-of-N sandbox runs modifying local host Git branches directly cause merge conflicts and dirty workspace history.

---

## 2. Decision

We decide to implement **Pluggable Credential Provisioning (`CredentialProvider`)** and the **Herdr Harvest Pipeline (`refs/sandbox/*`)** in MetaMach 0.7.0.

### Key Resolution Points:

1. **Pluggable Credential Provider SPI**: Abstract credential generation behind a `CredentialProvider` trait in `janus/src/cognitive/credential.rs`. Supports ephemeral key provisioning backends (e.g. OpenRouter capped keys, Volcengine temp tokens, local environment keys) with configurable usage limits (e.g. `$50` cap).
2. **Ephemeral Key Lifecycle**: `janus-daemon` provisions temporary credentials when spawning a sandbox task and automatically revokes them upon step completion or termination.
3. **Read-Only Harvest Git References**: Modifications produced inside sandboxes are collected as read-only Git references under `refs/sandbox/<sandbox-id>`.
4. **Herdr TUI Harvest Viewport**: Extend `herdr-janus` TUI with a Harvest diff preview card (`[H]`) and one-key merge control (`[M]`) to review and merge candidate sandbox runs into the main branch.

---

## 3. Detailed Specification

### 3.1 Credential Provider SPI (`janus/src/cognitive/credential.rs`)

```rust
#[async_trait::async_trait]
pub trait CredentialProvider: Send + Sync {
    async fn provision_key(&self, max_cost_usd: f64) -> Result<EphemeralKey, CredentialError>;
    async fn revoke_key(&self, key_id: &str) -> Result<(), CredentialError>;
}

pub struct EphemeralKey {
    pub key_id: String,
    pub secret: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}
```

### 3.2 Harvest Ref Collection & TUI Integration

```text
 [Host Central Control]
   │
   ├── 1. Provision Key ──► CredentialProvider::provision_key($50 Cap) ──► Inject to Sandbox Env
   │
   ├── 2. Run Sandbox ──► Parallel execution (sandbox-01, sandbox-02)
   │
   └── 3. Harvest ──────► Collect diff as `git fetch refs/sandbox/sandbox-01`
                               │
                               ▼
                   ┌────────────────────────┐
                   │ Herdr TUI Viewport     │
                   │ [H] Harvest Diff Card  │
                   │ [M] Merge to Main      │
                   └────────────────────────┘
```

---

## 4. Consequences

### Positive:
- **Zero Host Key Exposure**: Ephemeral, capped API keys isolate sandbox compute from host billing credentials.
- **Clean Git History**: Main workspace remains pristine until changes are visually inspected and merged via Herdr TUI.

### Negative / Cost:
- **SPI Implementation**: Requires cloud provider credential management backends.
