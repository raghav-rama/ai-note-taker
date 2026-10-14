import Foundation

// MARK: - Custom ISO8601 Date Decoding

/// Rust chrono emits RFC 3339 with fractional seconds, which the built-in
/// `.iso8601` JSONDecoder strategy rejects. This strategy tries both forms.
struct FlexibleISO8601DateDecodingStrategy {
    private static let withFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static let withoutFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    static func decode(_ decoder: Decoder) throws -> Date {
        let container = try decoder.singleValueContainer()
        let string = try container.decode(String.self)
        if let date = withFractional.date(from: string) {
            return date
        }
        if let date = withoutFractional.date(from: string) {
            return date
        }
        throw DecodingError.dataCorrupted(
            .init(codingPath: decoder.codingPath,
                  debugDescription: "Cannot parse date: \(string)")
        )
    }
}

// MARK: - Meeting

struct Meeting: Codable, Identifiable, Hashable {
    let id: Int64
    var title: String
    var startedAt: Date
    var endedAt: Date?
    var durationSecs: Int?
    var summary: String?

    enum CodingKeys: String, CodingKey {
        case id
        case title
        case started_at
        case ended_at
        case duration_secs
        case summary
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        title = try c.decode(String.self, forKey: .title)
        startedAt = try c.decode(Date.self, forKey: .started_at)
        endedAt = try c.decodeIfPresent(Date.self, forKey: .ended_at)
        durationSecs = try c.decodeIfPresent(Int.self, forKey: .duration_secs)
        summary = try c.decodeIfPresent(String.self, forKey: .summary)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(title, forKey: .title)
        try c.encode(startedAt, forKey: .started_at)
        try c.encodeIfPresent(endedAt, forKey: .ended_at)
        try c.encodeIfPresent(durationSecs, forKey: .duration_secs)
        try c.encodeIfPresent(summary, forKey: .summary)
    }

    init(id: Int64, title: String, startedAt: Date, endedAt: Date? = nil,
         durationSecs: Int? = nil, summary: String? = nil) {
        self.id = id
        self.title = title
        self.startedAt = startedAt
        self.endedAt = endedAt
        self.durationSecs = durationSecs
        self.summary = summary
    }
}

// MARK: - Transcript Segment

struct TranscriptSegment: Codable, Identifiable, Hashable {
    let id: Int64
    let meetingId: Int64
    let timestampMs: Int64
    let text: String
    let confidence: Double?

    enum CodingKeys: String, CodingKey {
        case id
        case meeting_id
        case timestamp_ms
        case text
        case confidence
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        meetingId = try c.decode(Int64.self, forKey: .meeting_id)
        timestampMs = try c.decode(Int64.self, forKey: .timestamp_ms)
        text = try c.decode(String.self, forKey: .text)
        confidence = try c.decodeIfPresent(Double.self, forKey: .confidence)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(meetingId, forKey: .meeting_id)
        try c.encode(timestampMs, forKey: .timestamp_ms)
        try c.encode(text, forKey: .text)
        try c.encodeIfPresent(confidence, forKey: .confidence)
    }

    init(id: Int64, meetingId: Int64, timestampMs: Int64, text: String,
         confidence: Double? = nil) {
        self.id = id
        self.meetingId = meetingId
        self.timestampMs = timestampMs
        self.text = text
        self.confidence = confidence
    }
}

// MARK: - Note

struct Note: Codable, Identifiable, Hashable {
    let id: Int64
    let meetingId: Int64
    var content: String
    var noteType: String
    var createdAt: Date
    var updatedAt: Date

    enum CodingKeys: String, CodingKey {
        case id
        case meeting_id
        case content
        case note_type
        case created_at
        case updated_at
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int64.self, forKey: .id)
        meetingId = try c.decode(Int64.self, forKey: .meeting_id)
        content = try c.decode(String.self, forKey: .content)
        noteType = try c.decode(String.self, forKey: .note_type)
        createdAt = try c.decode(Date.self, forKey: .created_at)
        updatedAt = try c.decode(Date.self, forKey: .updated_at)
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(meetingId, forKey: .meeting_id)
        try c.encode(content, forKey: .content)
        try c.encode(noteType, forKey: .note_type)
        try c.encode(createdAt, forKey: .created_at)
        try c.encode(updatedAt, forKey: .updated_at)
    }

    init(id: Int64, meetingId: Int64, content: String, noteType: String,
         createdAt: Date, updatedAt: Date) {
        self.id = id
        self.meetingId = meetingId
        self.content = content
        self.noteType = noteType
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

// MARK: - Summary (parsed from AI-generated JSON)

struct Summary: Codable, Hashable {
    let summary: String
    let keyPoints: [String]
    let actionItems: [String]
    let decisions: [String]

    enum CodingKeys: String, CodingKey {
        case summary
        case key_points
        case action_items
        case decisions
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        summary = try c.decode(String.self, forKey: .summary)
        keyPoints = try c.decodeIfPresent([String].self, forKey: .key_points) ?? []
        actionItems = try c.decodeIfPresent([String].self, forKey: .action_items) ?? []
        decisions = try c.decodeIfPresent([String].self, forKey: .decisions) ?? []
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(summary, forKey: .summary)
        try c.encode(keyPoints, forKey: .key_points)
        try c.encode(actionItems, forKey: .action_items)
        try c.encode(decisions, forKey: .decisions)
    }

    init(summary: String, keyPoints: [String], actionItems: [String], decisions: [String]) {
        self.summary = summary
        self.keyPoints = keyPoints
        self.actionItems = actionItems
        self.decisions = decisions
    }
}

// MARK: - Meeting Detail (returned by note_get_meeting)

/// Full meeting payload including transcript segments and notes.
struct MeetingDetail: Codable {
    let meeting: Meeting
    let segments: [TranscriptSegment]?
    let notes: [Note]?
}

// MARK: - Audio Device

struct AudioDevice: Codable, Identifiable, Hashable {
    let id: String
    let name: String
    let isDefault: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case is_default
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        name = try c.decode(String.self, forKey: .name)
        isDefault = try c.decodeIfPresent(Bool.self, forKey: .is_default) ?? false
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(id, forKey: .id)
        try c.encode(name, forKey: .name)
        try c.encode(isDefault, forKey: .is_default)
    }
}

// MARK: - JSON Decoder configured for the core library

extension JSONDecoder {
    /// Decoder configured for notetaker-core JSON output.
    static let noteCore: JSONDecoder = {
        let d = JSONDecoder()
        d.dateDecodingStrategy = .custom(FlexibleISO8601DateDecodingStrategy.decode)
        return d
    }()
}
