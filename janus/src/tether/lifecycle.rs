//! Tether lifecycle service - cold-start restart integration (0.3.0 §2.4).
//!
//! On daemon restart, [`LifecycleService::restart_session`] mints a fresh Tether
//! session UUID and re-creates the session from the absurd checkpoint (the last
//! `COMPLETED` step's `result_cache`). Full checkpoint-driven re-exec lands with
//! M4 workflow execution; this provides the session-creation hook the cold-start
//! reconciler calls, decoupled from the backend implementation.

use std::path::Path;

use anyhow::Result;

use super::{DurableBackend, SessionId};

pub struct LifecycleService;

impl LifecycleService {
    /// Re-create a session for a resumed task. Returns the new [`SessionId`].
    ///
    /// In M4 this reads the absurd checkpoint to reconstruct the command; for now
    /// the caller passes the resume command + cwd directly, and we mint a fresh
    /// `tether-janus-task-<task_uuid>` name and create the session on the
    /// isolated `tmux -L metamach-tether` server.
    pub fn restart_session(
        backend: &dyn DurableBackend,
        task_uuid: &str,
        command: &str,
        cwd: Option<&Path>,
    ) -> Result<SessionId> {
        let id = SessionId::new_for_task(task_uuid);
        backend.create_session(&id, command, cwd)?;
        Ok(id)
    }
}
