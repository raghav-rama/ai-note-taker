//! SQLite storage layer for meetings, transcripts, notes, and settings.
//!
//! Uses rusqlite with the bundled SQLite build (includes FTS5).
//! The database file is created and migrated automatically on first open.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::sync::Mutex;

use crate::models::{Meeting, Note, TranscriptSegment};

/// Thread-safe wrapper around a rusqlite `Connection`.
///
/// `Connection` is `Send` but not `Sync`, so we wrap it in a `Mutex`
/// to allow shared access from the FFI handle.
pub struct Database {
    conn: Mutex<Connection>,
}

/// Ordered schema migrations. The database's `PRAGMA user_version` records how
/// many of these have been applied; [`Database::open`] runs any that are pending.
///
/// **Append-only.** Never edit or reorder an existing entry — a shipped database
/// has already applied it and will not re-run it. Add new schema changes as new
/// array elements. Each migration must be safe to apply to a database that a
/// previous app version created (hence `IF NOT EXISTS` on migration 1, which also
/// upgrades Phase-1 databases that have the tables but `user_version = 0`).
const MIGRATIONS: &[&str] = &[
    // ── Migration 1: initial schema ──────────────────────────────────────
    "
    CREATE TABLE IF NOT EXISTS meetings (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title TEXT NOT NULL DEFAULT 'Untitled Meeting',
        started_at TEXT NOT NULL,
        ended_at TEXT,
        duration_secs INTEGER,
        summary TEXT
    );

    CREATE TABLE IF NOT EXISTS transcript_segments (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id INTEGER NOT NULL REFERENCES meetings(id),
        timestamp_ms INTEGER NOT NULL,
        text TEXT NOT NULL,
        confidence REAL
    );

    CREATE TABLE IF NOT EXISTS notes (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id INTEGER NOT NULL REFERENCES meetings(id),
        content TEXT NOT NULL,
        note_type TEXT NOT NULL DEFAULT 'summary',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );

    -- FTS5 virtual table for full-text search over transcript segments.
    CREATE VIRTUAL TABLE IF NOT EXISTS transcript_fts USING fts5(
        text,
        content='transcript_segments',
        content_rowid='id'
    );

    -- Triggers to keep the FTS index in sync.
    CREATE TRIGGER IF NOT EXISTS transcript_fts_ai AFTER INSERT ON transcript_segments BEGIN
        INSERT INTO transcript_fts(rowid, text) VALUES (new.id, new.text);
    END;

    CREATE TRIGGER IF NOT EXISTS transcript_fts_ad AFTER DELETE ON transcript_segments BEGIN
        INSERT INTO transcript_fts(transcript_fts, rowid, text) VALUES('delete', old.id, old.text);
    END;

    CREATE TRIGGER IF NOT EXISTS transcript_fts_au AFTER UPDATE ON transcript_segments BEGIN
        INSERT INTO transcript_fts(transcript_fts, rowid, text) VALUES('delete', old.id, old.text);
        INSERT INTO transcript_fts(rowid, text) VALUES (new.id, new.text);
    END;
    ",
    // ── Migration 2: structured AI summaries ─────────────────────────────
    // One row per meeting. The narrative lives in `summary`; the three lists are
    // stored as JSON arrays so the structured shape survives without widening the
    // `meetings` row (which is the JSON contract the native apps decode).
    "
    CREATE TABLE IF NOT EXISTS meeting_summaries (
        meeting_id   INTEGER PRIMARY KEY REFERENCES meetings(id),
        summary      TEXT NOT NULL,
        key_points   TEXT NOT NULL DEFAULT '[]',
        action_items TEXT NOT NULL DEFAULT '[]',
        decisions    TEXT NOT NULL DEFAULT '[]'
    );
    ",
];

impl Database {
    /// Open (or create) a database at the given path and apply migrations.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {}", path))?;

        // Enable WAL mode for better concurrent read performance.
        // (On `:memory:` databases this is a no-op and journal_mode stays
        // "memory"; execute_batch ignores the returned row either way.)
        conn.execute_batch("PRAGMA journal_mode = WAL;")?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }

    /// Return the current schema version (`PRAGMA user_version`).
    pub fn schema_version(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        Ok(v)
    }

    /// Apply any migrations the database has not yet seen, advancing
    /// `user_version` after each. Each migration runs in its own transaction so a
    /// failure leaves the version pointing at the last fully-applied migration.
    fn run_migrations(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mut current: usize = conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?
            .max(0) as usize;

        while current < MIGRATIONS.len() {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(MIGRATIONS[current])
                .with_context(|| format!("Failed to apply migration {}", current + 1))?;
            // `user_version` takes an integer literal (it cannot be parameterized);
            // the value is an internal counter, never user input, so this is safe.
            tx.execute_batch(&format!("PRAGMA user_version = {};", current + 1))?;
            tx.commit()?;
            current += 1;
        }
        Ok(())
    }

    // ── Meetings ──────────────────────────────────────────────────────

    /// Create a new meeting and return its id.
    pub fn create_meeting(&self, title: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO meetings (title, started_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![title],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Mark a meeting as ended and compute its duration.
    pub fn end_meeting(&self, meeting_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET
                ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                duration_secs = CAST((julianday('now') - julianday(started_at)) * 86400 AS INTEGER)
             WHERE id = ?1",
            params![meeting_id],
        )?;
        Ok(())
    }

    /// Update the summary text for a meeting.
    pub fn update_meeting_summary(&self, meeting_id: i64, summary: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET summary = ?1 WHERE id = ?2",
            params![summary, meeting_id],
        )?;
        Ok(())
    }

    /// Update the title for a meeting.
    pub fn update_meeting_title(&self, meeting_id: i64, title: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE meetings SET title = ?1 WHERE id = ?2",
            params![title, meeting_id],
        )?;
        Ok(())
    }

    /// List meetings ordered by most recent first.
    pub fn list_meetings(&self, limit: i32, offset: i32) -> Result<Vec<Meeting>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, started_at, ended_at, duration_secs, summary
             FROM meetings ORDER BY started_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(Meeting {
                id: row.get(0)?,
                title: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                duration_secs: row.get(4)?,
                summary: row.get(5)?,
            })
        })?;
        let mut meetings = Vec::new();
        for row in rows {
            meetings.push(row?);
        }
        Ok(meetings)
    }

    /// Get a single meeting by id.
    pub fn get_meeting(&self, meeting_id: i64) -> Result<Option<Meeting>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, started_at, ended_at, duration_secs, summary
             FROM meetings WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![meeting_id], |row| {
            Ok(Meeting {
                id: row.get(0)?,
                title: row.get(1)?,
                started_at: row.get(2)?,
                ended_at: row.get(3)?,
                duration_secs: row.get(4)?,
                summary: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Delete a meeting and all associated transcript segments and notes.
    pub fn delete_meeting(&self, meeting_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM notes WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        // FTS delete triggers fire automatically from the transcript_segments delete trigger.
        tx.execute(
            "DELETE FROM transcript_segments WHERE meeting_id = ?1",
            params![meeting_id],
        )?;
        tx.execute("DELETE FROM meetings WHERE id = ?1", params![meeting_id])?;
        tx.commit()?;
        Ok(())
    }

    // ── Transcript Segments ───────────────────────────────────────────

    /// Insert a transcript segment for a meeting.
    pub fn insert_segment(
        &self,
        meeting_id: i64,
        timestamp_ms: i64,
        text: &str,
        confidence: Option<f64>,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transcript_segments (meeting_id, timestamp_ms, text, confidence)
             VALUES (?1, ?2, ?3, ?4)",
            params![meeting_id, timestamp_ms, text, confidence],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all transcript segments for a meeting, ordered by timestamp.
    pub fn get_segments(&self, meeting_id: i64) -> Result<Vec<TranscriptSegment>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, meeting_id, timestamp_ms, text, confidence
             FROM transcript_segments WHERE meeting_id = ?1 ORDER BY timestamp_ms ASC",
        )?;
        let rows = stmt.query_map(params![meeting_id], |row| {
            Ok(TranscriptSegment {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                timestamp_ms: row.get(2)?,
                text: row.get(3)?,
                confidence: row.get(4)?,
            })
        })?;
        let mut segments = Vec::new();
        for row in rows {
            segments.push(row?);
        }
        Ok(segments)
    }

    // ── Notes ─────────────────────────────────────────────────────────

    /// Insert a note for a meeting.
    pub fn insert_note(&self, meeting_id: i64, content: &str, note_type: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO notes (meeting_id, content, note_type, created_at, updated_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![meeting_id, content, note_type],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all notes for a meeting.
    pub fn get_notes(&self, meeting_id: i64) -> Result<Vec<Note>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, meeting_id, content, note_type, created_at, updated_at
             FROM notes WHERE meeting_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![meeting_id], |row| {
            Ok(Note {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                content: row.get(2)?,
                note_type: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        let mut notes = Vec::new();
        for row in rows {
            notes.push(row?);
        }
        Ok(notes)
    }

    // ── Full-Text Search ──────────────────────────────────────────────

    /// Search meetings whose transcripts match the given query.
    /// Returns meeting ids that contain matching transcript segments.
    pub fn search_meetings(&self, query: &str) -> Result<Vec<i64>> {
        // An empty/whitespace query would become MATCH '""', which is not a
        // meaningful search; return no results rather than erroring.
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ts.meeting_id
             FROM transcript_fts fts
             JOIN transcript_segments ts ON ts.id = fts.rowid
             WHERE transcript_fts MATCH ?1
             ORDER BY ts.meeting_id DESC",
        )?;
        // Quote the whole query so FTS5 operator characters are matched
        // literally; embedded double-quotes are escaped by doubling them.
        let match_query = format!("\"{}\"", query.replace('"', "\"\""));
        let rows = stmt.query_map(params![match_query], |row| row.get::<_, i64>(0))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row?);
        }
        Ok(ids)
    }

    /// Search meetings by transcript text and return the full [`Meeting`] rows,
    /// newest first. This is what the FFI search surface returns so callers get
    /// decodable meeting objects rather than bare ids.
    pub fn search_meetings_full(&self, query: &str) -> Result<Vec<Meeting>> {
        let ids = self.search_meetings(query)?;
        let mut meetings = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(m) = self.get_meeting(id)? {
                meetings.push(m);
            }
        }
        Ok(meetings)
    }

    // ── Settings ──────────────────────────────────────────────────────

    /// Set a setting value (upsert).
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Get a setting value by key.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    // ── Export ─────────────────────────────────────────────────────────

    /// Export a meeting in the specified format ("markdown", "text", or "json").
    pub fn export_meeting(&self, meeting_id: i64, format: &str) -> Result<String> {
        let meeting = self
            .get_meeting(meeting_id)?
            .ok_or_else(|| anyhow::anyhow!("Meeting {} not found", meeting_id))?;
        let segments = self.get_segments(meeting_id)?;
        let notes = self.get_notes(meeting_id)?;

        match format {
            "markdown" | "md" => Ok(Self::export_markdown(&meeting, &segments, &notes)),
            "text" | "txt" => Ok(Self::export_text(&meeting, &segments)),
            "json" => {
                let export = serde_json::json!({
                    "meeting": meeting,
                    "segments": segments,
                    "notes": notes,
                });
                Ok(serde_json::to_string_pretty(&export)?)
            }
            _ => Err(anyhow::anyhow!("Unsupported export format: {}", format)),
        }
    }

    fn export_markdown(
        meeting: &Meeting,
        segments: &[TranscriptSegment],
        notes: &[Note],
    ) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n\n", meeting.title));
        out.push_str(&format!("**Date:** {}\n", meeting.started_at));
        if let Some(dur) = meeting.duration_secs {
            let mins = dur / 60;
            let secs = dur % 60;
            out.push_str(&format!("**Duration:** {}m {}s\n", mins, secs));
        }
        out.push('\n');

        if let Some(summary) = &meeting.summary {
            out.push_str("## Summary\n\n");
            out.push_str(summary);
            out.push_str("\n\n");
        }

        for note in notes {
            out.push_str(&format!("## {}\n\n", note.note_type));
            out.push_str(&note.content);
            out.push_str("\n\n");
        }

        out.push_str("## Transcript\n\n");
        for seg in segments {
            let secs = seg.timestamp_ms / 1000;
            let mins = secs / 60;
            let s = secs % 60;
            out.push_str(&format!("[{:02}:{:02}] {}\n", mins, s, seg.text));
        }
        out
    }

    fn export_text(meeting: &Meeting, segments: &[TranscriptSegment]) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\n", meeting.title));
        out.push_str(&format!("Date: {}\n", meeting.started_at));
        if let Some(dur) = meeting.duration_secs {
            let mins = dur / 60;
            let secs = dur % 60;
            out.push_str(&format!("Duration: {}m {}s\n", mins, secs));
        }
        out.push('\n');
        if let Some(summary) = &meeting.summary {
            out.push_str("Summary:\n");
            out.push_str(summary);
            out.push_str("\n\n");
        }
        out.push_str("Transcript:\n");
        for seg in segments {
            let secs = seg.timestamp_ms / 1000;
            let mins = secs / 60;
            let s = secs % 60;
            out.push_str(&format!("[{:02}:{:02}] {}\n", mins, s, seg.text));
        }
        out
    }
}
