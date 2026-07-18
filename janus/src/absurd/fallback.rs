//! Disaster-recovery ring buffer (`fallback.db`, SQLite) - Contract 3.8.
//!
//! Hosts Step transition states during PG unreachability. Bounded: max 1000
//! entries; oldest evicted on insert. On PG recovery a batch Log Replay merges
//! these into absurd checkpoint state (`set_task_checkpoint_state`) + the
//! `metamach_step_meta` overlay, then truncates the ring (replay lands with M3/M4).

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

/// Contract 3.8: max 1000 entries (50MB cap is enforced by eviction + the 16KiB
/// per-entry truncation; a physical size guard can be added if abuse is seen).
const MAX_ENTRIES: i64 = 1000;

pub struct FallbackDb(Mutex<Connection>);

impl FallbackDb {
    /// Open (creating the schema if needed) the ring buffer at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS fallback_events (
                seq          INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id      TEXT NOT NULL,
                step_name    TEXT    NOT NULL,
                status       TEXT    NOT NULL,
                result_cache TEXT,
                created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_fe_task ON fallback_events(task_id);",
        )?;
        Ok(Self(Mutex::new(conn)))
    }

    /// Append an event, evicting the oldest beyond MAX_ENTRIES (ring buffer).
    pub fn record(
        &self,
        task_id: &Uuid,
        step_name: &str,
        status: &str,
        result_cache: Option<&str>,
    ) -> Result<()> {
        let cache = result_cache.map(super::truncate_16k);
        let conn = self.0.lock().expect("fallback mutex poisoned");
        conn.execute(
            "INSERT INTO fallback_events (task_id, step_name, status, result_cache) \
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![task_id.to_string(), step_name, status, cache],
        )?;
        conn.execute(
            "DELETE FROM fallback_events WHERE seq NOT IN \
             (SELECT seq FROM fallback_events ORDER BY seq DESC LIMIT ?1)",
            rusqlite::params![MAX_ENTRIES],
        )?;
        Ok(())
    }

    /// Current entry count (for tests / health).
    pub fn count(&self) -> Result<i64> {
        let conn = self.0.lock().expect("fallback mutex poisoned");
        Ok(conn.query_row("SELECT COUNT(*) FROM fallback_events", [], |r| r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::absurd::SIZE_BUDGET;
    use tempfile::NamedTempFile;

    fn tmp() -> NamedTempFile {
        NamedTempFile::new().expect("tmp file")
    }

    #[test]
    fn records_and_counts() {
        let f = tmp();
        let db = FallbackDb::open(f.path()).expect("open");
        assert_eq!(db.count().unwrap(), 0);
        db.record(&Uuid::nil(), "scout", "COMPLETED", None).unwrap();
        db.record(&Uuid::nil(), "code", "RUNNING", Some("{\"x\":1}"))
            .unwrap();
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let f = tmp();
        let db = FallbackDb::open(f.path()).expect("open");
        for i in 0..(MAX_ENTRIES + 50) {
            db.record(&Uuid::from_u128(i as u128), "s", "RUNNING", None)
                .unwrap();
        }
        assert_eq!(db.count().unwrap(), MAX_ENTRIES);
    }

    #[test]
    fn record_truncates_oversized_cache() {
        let f = tmp();
        let db = FallbackDb::open(f.path()).expect("open");
        let big = "y".repeat(SIZE_BUDGET * 2);
        db.record(&Uuid::nil(), "s", "RUNNING", Some(&big)).unwrap();
        let conn = db.0.lock().unwrap();
        let stored: String = conn
            .query_row(
                "SELECT result_cache FROM fallback_events WHERE task_id = ?1",
                rusqlite::params![Uuid::nil().to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert!(stored.len() <= SIZE_BUDGET);
        assert!(stored.ends_with("[MetaMach Log Budget Exceeded]"));
    }
}
