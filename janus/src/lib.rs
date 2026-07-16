//! Janus shared library.
//!
//! Backs all four binaries:
//!   - `janus-daemon` (resident brain, server) - hosts the UDS listener + AbsurdDb.
//!   - `herdr-janus` (shadow client) - UDS client + lazy-start + TUI.
//!   - `janus` (unified CLI) - UDS client for `status` / `daemon`.
//!   - `janus-sh` (proxy shell) - UDS client; synchronously reconciles each
//!     command with the Daemon's Tool Guard before exec (Feature-Spec §2.2).
//!
//! Architecture (Feature-Spec §2.1, ARCH §3): the Daemon is the sole owner of
//! state and the DB pool; clients never touch Postgres directly - they ask the
//! Daemon over `janus.sock` via the protocol defined in [`protocol`].

pub mod absurd;
pub mod paths;
pub mod protocol;
pub mod spawn;
pub mod tool_guard;
pub mod uds;
