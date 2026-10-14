//! Integration tests for the SQLite storage layer.
//!
//! Most tests use an in-memory database; the migration-idempotency test needs a
//! real file so it can be reopened.

use notetaker::models::Summary;
use notetaker::storage::Database;

/// Open a fresh in-memory database.
fn mem() -> Database {
    Database::open(":memory:").expect("open in-memory db")
}

/// A unique temp file path for tests that must reopen the same database.
fn tmp_path(tag: &str) -> String {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!(
        "notetaker_storage_{}_{}_{}.db",
        tag,
        std::process::id(),
        nanos
    ));
    p.to_string_lossy().into_owned()
}

#[test]
fn fresh_db_is_at_latest_schema_version() {
    let db = mem();
    // Two migrations defined → version 2 after open.
    assert_eq!(db.schema_version().unwrap(), 2);
}

#[test]
fn migrations_are_idempotent_across_opens() {
    let path = tmp_path("idem");
    {
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        let id = db.create_meeting("persisted").unwrap();
        db.insert_segment(id, 0, "data survives", None).unwrap();
    }
    {
        // Reopening must not re-run migrations or lose data.
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        assert_eq!(db.list_meetings(10, 0).unwrap().len(), 1);
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", &path));
    let _ = std::fs::remove_file(format!("{}-shm", &path));
}

#[test]
fn meeting_crud_and_segments() {
    let db = mem();
    let id = db.create_meeting("My Meeting").unwrap();
    assert!(id > 0);

    let got = db.get_meeting(id).unwrap().expect("meeting exists");
    assert_eq!(got.title, "My Meeting");
    assert!(got.ended_at.is_none());

    db.update_meeting_title(id, "Renamed").unwrap();
    assert_eq!(db.get_meeting(id).unwrap().unwrap().title, "Renamed");

    db.insert_segment(id, 0, "hello world", Some(0.9)).unwrap();
    db.insert_segment(id, 1000, "second segment", None).unwrap();
    let segs = db.get_segments(id).unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].text, "hello world");
    assert_eq!(segs[0].confidence, Some(0.9));
    assert_eq!(segs[1].confidence, None);

    db.end_meeting(id).unwrap();
    assert!(db.get_meeting(id).unwrap().unwrap().ended_at.is_some());

    assert_eq!(db.list_meetings(10, 0).unwrap().len(), 1);
    assert!(db.get_meeting(99999).unwrap().is_none());
}

#[test]
fn fts_search_and_injection_safety() {
    let db = mem();
    let m1 = db.create_meeting("m1").unwrap();
    db.insert_segment(m1, 0, "the quarterly budget review", None)
        .unwrap();
    let m2 = db.create_meeting("m2").unwrap();
    db.insert_segment(m2, 0, "cat pictures and memes", None).unwrap();

    assert_eq!(db.search_meetings("budget").unwrap(), vec![m1]);

    let full = db.search_meetings_full("budget").unwrap();
    assert_eq!(full.len(), 1);
    assert_eq!(full[0].id, m1);

    // Queries with FTS operators / quotes must be matched literally, never error.
    for q in ["\"", "budget OR", "a AND b", "NEAR(x y)", "foo*", "budget review"] {
        let _ = db
            .search_meetings(q)
            .unwrap_or_else(|e| panic!("search({q:?}) errored: {e}"));
    }

    // Empty / whitespace query returns nothing (not an error).
    assert!(db.search_meetings("   ").unwrap().is_empty());
    assert!(db.search_meetings_full("nomatch").unwrap().is_empty());
}

#[test]
fn delete_meeting_cascades_including_fts() {
    let db = mem();
    let id = db.create_meeting("m").unwrap();
    db.insert_segment(id, 0, "words here", None).unwrap();
    db.insert_note(id, "a note", "summary").unwrap();

    db.delete_meeting(id).unwrap();

    assert!(db.get_meeting(id).unwrap().is_none());
    assert!(db.get_segments(id).unwrap().is_empty());
    assert!(db.get_notes(id).unwrap().is_empty());
    // The FTS index no longer returns the deleted meeting's text.
    assert!(db.search_meetings("words").unwrap().is_empty());
}

#[test]
fn summary_upsert_and_get() {
    let db = mem();
    let id = db.create_meeting("m").unwrap();
    assert!(db.get_summary(id).unwrap().is_none());

    let s = Summary {
        summary: "narrative".into(),
        key_points: vec!["kp1".into(), "kp2".into()],
        action_items: vec!["ai1".into()],
        decisions: vec![],
    };
    db.upsert_summary(id, &s).unwrap();
    let got = db.get_summary(id).unwrap().unwrap();
    assert_eq!(got.summary, "narrative");
    assert_eq!(got.key_points, vec!["kp1".to_string(), "kp2".to_string()]);
    assert_eq!(got.action_items.len(), 1);
    assert!(got.decisions.is_empty());

    // Upsert replaces the existing row.
    let s2 = Summary {
        summary: "updated".into(),
        key_points: vec![],
        action_items: vec![],
        decisions: vec!["d".into()],
    };
    db.upsert_summary(id, &s2).unwrap();
    let got2 = db.get_summary(id).unwrap().unwrap();
    assert_eq!(got2.summary, "updated");
    assert_eq!(got2.decisions, vec!["d".to_string()]);
    assert!(got2.key_points.is_empty());
}

#[test]
fn export_formats() {
    let db = mem();
    let id = db.create_meeting("Export Me").unwrap();
    db.insert_segment(id, 0, "first", None).unwrap();
    db.insert_segment(id, 65000, "later", None).unwrap();
    db.update_meeting_summary(id, "the summary").unwrap();

    let md = db.export_meeting(id, "markdown").unwrap();
    assert!(md.contains("# Export Me"));
    assert!(md.contains("## Transcript"));
    assert!(md.contains("[00:00] first"));
    assert!(md.contains("[01:05] later"));
    assert!(md.contains("the summary"));

    let txt = db.export_meeting(id, "text").unwrap();
    assert!(txt.contains("Export Me"));
    assert!(txt.contains("first"));

    let json = db.export_meeting(id, "json").unwrap();
    assert!(json.contains("\"segments\""));
    assert!(json.contains("first"));

    assert!(db.export_meeting(id, "bogus").is_err());
}

#[test]
fn settings_upsert_and_get() {
    let db = mem();
    assert!(db.get_setting("whisper_model").unwrap().is_none());
    db.set_setting("whisper_model", "tiny").unwrap();
    assert_eq!(
        db.get_setting("whisper_model").unwrap().as_deref(),
        Some("tiny")
    );
    db.set_setting("whisper_model", "base").unwrap();
    assert_eq!(
        db.get_setting("whisper_model").unwrap().as_deref(),
        Some("base")
    );
}
