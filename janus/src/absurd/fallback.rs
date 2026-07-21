//! Disaster-recovery ring buffer (`fallback.db`, SQLite) - Contract 3.8.
//!
//! Hosts Step transition states during PG unreachability. Bounded: max 1000
//! entries; oldest evicted on insert. On PG recovery a batch Log Replay
//! (`AbsurdDb::replay_fallback`) drains the ring and merges each event into the
//! routed per-blueprint `metamach_step_meta` overlay, then truncates the ring.
//!
//! Each event carries `blueprint_name` (0.3.0 Contract 3.8 routing column) so
//! replay can target the correct `metamach_blueprint_<name>` DB. `result_cache`
//! is captured for completeness; the absurd checkpoint-state merge
//! (`set_task_checkpoint_state`) lands when the absurd engine is integrated.

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

/// Contract 3.8: max 1000 entries (50MB cap is enforced by eviction + the 16KiB
/// per-entry truncation; a physical size guard can be added if abuse is seen).
const MAX_ENTRIES: i64 = 1000;

/// One buffered Step transition, drained by Log Replay on PG recovery.
pub struct FallbackEvent {
    pub task_id: String,
    pub blueprint_name: String,
    pub step_name: String,
    pub status: String,
    pub result_cache: Option<String>,
}

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
                seq            INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id        TEXT NOT NULL,
                blueprint_name TEXT NOT NULL,
                step_name      TEXT    NOT NULL,
                status         TEXT    NOT NULL,
                result_cache   TEXT,
                created_at     TEXT    NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_fe_task ON fallback_events(task_id);
            CREATE INDEX IF NOT EXISTS idx_fe_blueprint ON fallback_events(blueprint_name);",
        )?;
        // Migrate pre-`blueprint_name` DBs (0.3.0 Contract 3.8 routing column).
        let has_bp: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(fallback_events)")?;
            let names: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))?
                .filter_map(Result::ok)
                .collect();
            names.iter().any(|c| c == "blueprint_name")
        };
        if !has_bp {
            // Older SQLite has no `ADD COLUMN IF NOT EXISTS`; the PRAGMA guard
            // makes this idempotent across versions.
            conn.execute(
                "ALTER TABLE fallback_events ADD COLUMN blueprint_name TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        Ok(Self(Mutex::new(conn)))
    }

    /// Append an event, evicting the oldest beyond MAX_ENTRIES (ring buffer).
    pub fn record(
        &self,
        task_id: &Uuid,
        blueprint_name: &str,
        step_name: &str,
        status: &str,
        result_cache: Option<&str>,
    ) -> Result<()> {
        let cache = result_cache.map(super::truncate_16k);
        let conn = self.0.lock().expect("fallback mutex poisoned");
        conn.execute(
            "INSERT INTO fallback_events (task_id, blueprint_name, step_name, status, result_cache) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![task_id.to_string(), blueprint_name, step_name, status, cache],
        )?;
        conn.execute(
            "DELETE FROM fallback_events WHERE seq NOT IN \
             (SELECT seq FROM fallback_events ORDER BY seq DESC LIMIT ?1)",
            rusqlite::params![MAX_ENTRIES],
        )?;
        Ok(())
    }

    /// Drain all pending events (oldest first) for Log Replay, truncating the
    /// ring. Returns the events to merge into Postgres.
    pub fn drain(&self) -> Result<Vec<FallbackEvent>> {
        let conn = self.0.lock().expect("fallback mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT task_id, blueprint_name, step_name, status, result_cache \
             FROM fallback_events ORDER BY seq ASC",
        )?;
        let events: Vec<FallbackEvent> = stmt
            .query_map([], |r| {
                Ok(FallbackEvent {
                    task_id: r.get(0)?,
                    blueprint_name: r.get(1)?,
                    step_name: r.get(2)?,
                    status: r.get(3)?,
                    result_cache: r.get(4)?,
                })
            })?
            .filter_map(Result::ok)
            .collect();
        conn.execute("DELETE FROM fallback_events", [])?;
        Ok(events)
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
        db.record(&Uuid::nil(), "gatemetric", "scout", "COMPLETED", None)
            .unwrap();
        db.record(
            &Uuid::nil(),
            "gatemetric",
            "code",
            "RUNNING",
            Some("{\"x\":1}"),
        )
        .unwrap();
        assert_eq!(db.count().unwrap(), 2);
    }

    #[test]
    fn ring_buffer_evicts_oldest() {
        let f = tmp();
        let db = FallbackDb::open(f.path()).expect("open");
        for i in 0..(MAX_ENTRIES + 50) {
            db.record(
                &Uuid::from_u128(i as u128),
                "gatemetric",
                "s",
                "RUNNING",
                None,
            )
            .unwrap();
        }
        assert_eq!(db.count().unwrap(), MAX_ENTRIES);
    }

    #[test]
    fn record_truncates_oversized_cache() {
        let f = tmp();
        let db = FallbackDb::open(f.path()).expect("open");
        let big = "y".repeat(SIZE_BUDGET * 2);
        db.record(&Uuid::nil(), "gatemetric", "s", "RUNNING", Some(&big))
            .unwrap();
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

    #[test]
    fn drain_returns_events_in_seq_order_and_empties_ring() {
        let f = tmp();
        let db = FallbackDb::open(f.path()).expect("open");
        db.record(&Uuid::nil(), "gatemetric", "scout", "COMPLETED", None)
            .unwrap();
        db.record(&Uuid::nil(), "joyrobots", "code", "SUSPENDED", None)
            .unwrap();
        let events = db.drain().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].step_name, "scout");
        assert_eq!(events[0].blueprint_name, "gatemetric");
        assert_eq!(events[1].blueprint_name, "joyrobots");
        assert_eq!(events[1].status, "SUSPENDED");
        // Ring is truncated after drain.
        assert_eq!(db.count().unwrap(), 0);
        // A second drain is empty.
        assert!(db.drain().unwrap().is_empty());
    }
}
