//! Shared data types for the notetaker core library.
//!
//! All types derive `Serialize` and `Deserialize` for JSON interchange
//! across the C FFI boundary.

use serde::{Deserialize, Serialize};

/// A recorded meeting session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: i64,
    pub title: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_secs: Option<i64>,
    pub summary: Option<String>,
}

/// A single segment of transcribed speech.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub id: i64,
    pub meeting_id: i64,
    /// Timestamp in milliseconds from the start of the recording.
    pub timestamp_ms: i64,
    pub text: String,
    /// Confidence score from the transcription engine (0.0 to 1.0), if available.
    pub confidence: Option<f64>,
}

/// A note attached to a meeting (summary, action items, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub meeting_id: i64,
    pub content: String,
    pub note_type: String,
    pub created_at: String,
    pub updated_at: String,
}

/// An available audio input device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDevice {
    /// Unique identifier for the device (platform-specific).
    pub id: String,
    /// Human-readable device name.
    pub name: String,
    /// Whether this is the system default input device.
    pub is_default: bool,
}

/// AI-generated meeting summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Narrative summary of the meeting.
    pub summary: String,
    /// Key points discussed.
    pub key_points: Vec<String>,
    /// Action items identified.
    pub action_items: Vec<String>,
    /// Decisions made during the meeting.
    pub decisions: Vec<String>,
}

/// Application settings as key-value pairs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub key: String,
    pub value: String,
}
