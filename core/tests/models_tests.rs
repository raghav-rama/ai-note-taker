//! Serde round-trip tests for the shared data models. These shapes are the JSON
//! contract the native apps decode across the FFI boundary, so a field rename or
//! type change must be deliberate.

use notetaker::models::{AudioDevice, Meeting, Note, Summary, TranscriptSegment};

#[test]
fn meeting_roundtrip() {
    let m = Meeting {
        id: 7,
        title: "Weekly sync".into(),
        started_at: "2026-10-14T09:00:00.000Z".into(),
        ended_at: Some("2026-10-14T09:30:00.000Z".into()),
        duration_secs: Some(1800),
        summary: Some("done".into()),
    };
    let json = serde_json::to_string(&m).unwrap();
    let back: Meeting = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, m.id);
    assert_eq!(back.title, m.title);
    assert_eq!(back.ended_at, m.ended_at);
    assert_eq!(back.duration_secs, m.duration_secs);
    assert_eq!(back.summary, m.summary);
}

#[test]
fn meeting_with_nulls_roundtrip() {
    let m = Meeting {
        id: 1,
        title: "Untitled Meeting".into(),
        started_at: "2026-10-14T09:00:00.000Z".into(),
        ended_at: None,
        duration_secs: None,
        summary: None,
    };
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains("\"ended_at\":null"));
    let back: Meeting = serde_json::from_str(&json).unwrap();
    assert!(back.ended_at.is_none());
    assert!(back.duration_secs.is_none());
}

#[test]
fn transcript_segment_roundtrip() {
    let s = TranscriptSegment {
        id: 3,
        meeting_id: 7,
        timestamp_ms: 1234,
        text: "hello".into(),
        confidence: Some(0.87),
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: TranscriptSegment = serde_json::from_str(&json).unwrap();
    assert_eq!(back.meeting_id, 7);
    assert_eq!(back.timestamp_ms, 1234);
    assert_eq!(back.text, "hello");
    assert_eq!(back.confidence, Some(0.87));
}

#[test]
fn summary_roundtrip() {
    let s = Summary {
        summary: "narrative".into(),
        key_points: vec!["a".into(), "b".into()],
        action_items: vec!["do x".into()],
        decisions: vec![],
    };
    let json = serde_json::to_string(&s).unwrap();
    // Field names the apps rely on.
    for key in ["summary", "key_points", "action_items", "decisions"] {
        assert!(json.contains(key), "missing key {key} in {json}");
    }
    let back: Summary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.key_points.len(), 2);
    assert_eq!(back.action_items, vec!["do x".to_string()]);
    assert!(back.decisions.is_empty());
}

#[test]
fn note_roundtrip() {
    let n = Note {
        id: 1,
        meeting_id: 2,
        content: "body".into(),
        note_type: "summary".into(),
        created_at: "2026-10-14T09:00:00.000Z".into(),
        updated_at: "2026-10-14T09:05:00.000Z".into(),
    };
    let json = serde_json::to_string(&n).unwrap();
    let back: Note = serde_json::from_str(&json).unwrap();
    assert_eq!(back.content, "body");
    assert_eq!(back.note_type, "summary");
}

#[test]
fn audio_device_roundtrip() {
    let d = AudioDevice {
        id: "hw:0".into(),
        name: "Built-in Mic".into(),
        is_default: true,
    };
    let json = serde_json::to_string(&d).unwrap();
    let back: AudioDevice = serde_json::from_str(&json).unwrap();
    assert_eq!(back.id, "hw:0");
    assert!(back.is_default);
}
