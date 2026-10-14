import Foundation
import SwiftUI

/// Represents the current state of a recording session.
enum RecordingState: Equatable {
    case idle
    case recording
    case paused
}

/// A single transcript segment received from the core engine.
struct TranscriptSegment: Identifiable, Equatable {
    let id = UUID()
    let text: String
    let timestamp: Double
}

/// A meeting stored in the database.
struct Meeting: Identifiable, Equatable {
    let id: Int64
    var title: String
    let startedAt: Date
    var endedAt: Date?
    var durationSeconds: Int?
    var summary: String?
    var segments: [TranscriptSegment] = []
    var notes: [MeetingNote] = []
}

/// A structured note attached to a meeting.
struct MeetingNote: Identifiable, Equatable {
    let id: Int64
    let meetingId: Int64
    var content: String
    let noteType: String
    let createdAt: Date
    var updatedAt: Date
}

/// Main view model that manages app state, recording, and interaction with the Rust core.
@MainActor @Observable
final class MeetingViewModel {

    // MARK: - State

    var recordingState: RecordingState = .idle
    var isSummarizing: Bool = false
    var meetingTitle: String = "Untitled Meeting"
    var transcriptSegments: [TranscriptSegment] = []
    var elapsedSeconds: Int = 0
    var audioLevel: Float = 0.0

    var meetings: [Meeting] = []
    var selectedMeeting: Meeting?
    var selectedMeetingNotes: [MeetingNote] = []
    var searchQuery: String = ""

    // Settings
    var useCloudTranscription: Bool = false
    var whisperApiKey: String = ""
    var claudeApiKey: String = ""
    var selectedModelName: String = "tiny"
    var appDisplayName: String = "System Helper"

    var errorMessage: String?
    var showError: Bool = false

    // MARK: - Internal — the shared core bridge

    let coreBridge: CoreBridge

    // MARK: - Private

    private var timer: Timer?
    private var recordingStartDate: Date?
    private var currentMeetingId: Int64?

    // MARK: - Initialization

    init(core: CoreBridge) {
        self.coreBridge = core
        coreBridge.setTranscriptCallback { [weak self] text, timestamp in
            Task { @MainActor [weak self] in
                self?.handleTranscriptSegment(text: text, timestamp: timestamp)
            }
        }

        // Load persisted settings.
        useCloudTranscription = (coreBridge.getSetting(key: "use_cloud") == "true")
        selectedModelName = coreBridge.getSetting(key: "whisper_model") ?? "tiny"
        appDisplayName = coreBridge.getSetting(key: "app_display_name") ?? "System Helper"
    }

    // MARK: - Recording

    func startRecording(deviceId: String?) {
        guard recordingState == .idle else { return }

        let result = coreBridge.startRecording(deviceId: deviceId)
        guard result == 0 else {
            showErrorMessage("Failed to start recording. Check microphone permissions.")
            return
        }

        recordingState = .recording
        meetingTitle = "Untitled Meeting"
        transcriptSegments = []
        elapsedSeconds = 0
        recordingStartDate = Date()

        timer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, self.recordingState == .recording else { return }
                if let start = self.recordingStartDate {
                    self.elapsedSeconds = Int(Date().timeIntervalSince(start))
                }
            }
        }
    }

    func stopRecording() {
        guard recordingState == .recording || recordingState == .paused else { return }

        _ = coreBridge.stopRecording()
        recordingState = .idle
        timer?.invalidate()
        timer = nil
        recordingStartDate = nil

        refreshMeetings()
    }

    func pauseRecording() {
        guard recordingState == .recording else { return }
        _ = coreBridge.pauseRecording()
        recordingState = .paused
        timer?.invalidate()
        timer = nil
    }

    func resumeRecording(deviceId: String?) {
        guard recordingState == .paused else { return }
        let result = coreBridge.startRecording(deviceId: deviceId)
        guard result == 0 else {
            showErrorMessage("Failed to resume recording.")
            return
        }
        recordingState = .recording
        timer = Timer.scheduledTimer(withTimeInterval: 1.0, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, self.recordingState == .recording else { return }
                if let start = self.recordingStartDate {
                    self.elapsedSeconds = Int(Date().timeIntervalSince(start))
                }
            }
        }
    }

    // MARK: - Transcript

    private func handleTranscriptSegment(text: String, timestamp: Double) {
        let segment = TranscriptSegment(text: text, timestamp: timestamp)
        transcriptSegments.append(segment)

        // Auto-title from first words of the transcript.
        if transcriptSegments.count == 1 {
            let words = text.split(separator: " ").prefix(6).joined(separator: " ")
            if !words.isEmpty {
                meetingTitle = words
            }
        }
    }

    // MARK: - Meetings

    func refreshMeetings() {
        guard let json = coreBridge.listMeetings() else {
            meetings = []
            return
        }
        meetings = parseMeetings(json: json)
    }

    func loadMeeting(id: Int64) {
        guard let json = coreBridge.getMeeting(id: id) else { return }
        selectedMeeting = parseSingleMeeting(json: json)
        selectedMeetingNotes = selectedMeeting?.notes ?? []
    }

    func deleteMeeting(id: Int64) {
        _ = coreBridge.deleteMeeting(id: id)
        if selectedMeeting?.id == id {
            selectedMeeting = nil
            selectedMeetingNotes = []
        }
        refreshMeetings()
    }

    // MARK: - Summarization

    func summarizeMeeting(id: Int64) {
        guard !isSummarizing else { return }
        isSummarizing = true
        let bridge = coreBridge
        Task.detached {
            let summary = bridge.summarize(meetingId: id)
            await MainActor.run { [weak self] in
                guard let self else { return }
                if let idx = self.meetings.firstIndex(where: { $0.id == id }) {
                    self.meetings[idx].summary = summary
                }
                if self.selectedMeeting?.id == id {
                    self.selectedMeeting?.summary = summary
                }
                self.isSummarizing = false
            }
        }
    }

    // MARK: - Export

    func exportMeeting(id: Int64, format: String) -> String? {
        return coreBridge.exportMeeting(id: id, format: format)
    }

    // MARK: - Settings persistence

    func saveSetting(key: String, value: String) {
        _ = coreBridge.setSetting(key: key, value: value)
    }

    func loadSetting(key: String) -> String? {
        return coreBridge.getSetting(key: key)
    }

    // MARK: - Errors

    private func showErrorMessage(_ message: String) {
        errorMessage = message
        showError = true
    }

    // MARK: - JSON parsing helpers

    private func parseMeetings(json: String) -> [Meeting] {
        guard let data = json.data(using: .utf8),
              let array = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return [] }
        return array.compactMap { parseMeetingDict($0) }
    }

    private func parseSingleMeeting(json: String) -> Meeting? {
        guard let data = json.data(using: .utf8),
              let dict = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return nil }
        return parseMeetingDict(dict)
    }

    private func parseMeetingDict(_ dict: [String: Any]) -> Meeting? {
        guard let id = dict["id"] as? Int64,
              let title = dict["title"] as? String,
              let startedAtStr = dict["started_at"] as? String
        else { return nil }

        let formatter = ISO8601DateFormatter()
        let startedAt = formatter.date(from: startedAtStr) ?? Date()
        let endedAt = (dict["ended_at"] as? String).flatMap { formatter.date(from: $0) }
        let duration = dict["duration_secs"] as? Int
        let summary = dict["summary"] as? String

        // Parse transcript segments embedded in the meeting JSON.
        var segments: [TranscriptSegment] = []
        if let segArray = dict["segments"] as? [[String: Any]] {
            for seg in segArray {
                let text = seg["text"] as? String ?? ""
                let timestampMs = seg["timestamp_ms"] as? Double ?? 0
                segments.append(TranscriptSegment(text: text, timestamp: timestampMs / 1000.0))
            }
        }

        // Parse structured notes embedded in the meeting JSON.
        var notes: [MeetingNote] = []
        if let notesArray = dict["notes"] as? [[String: Any]] {
            for note in notesArray {
                let noteId = note["id"] as? Int64 ?? 0
                let content = note["content"] as? String ?? ""
                let noteType = note["note_type"] as? String ?? "summary"
                let createdAtStr = note["created_at"] as? String ?? ""
                let updatedAtStr = note["updated_at"] as? String ?? ""
                notes.append(MeetingNote(
                    id: noteId,
                    meetingId: id,
                    content: content,
                    noteType: noteType,
                    createdAt: formatter.date(from: createdAtStr) ?? Date(),
                    updatedAt: formatter.date(from: updatedAtStr) ?? Date()
                ))
            }
        }

        return Meeting(
            id: id,
            title: title,
            startedAt: startedAt,
            endedAt: endedAt,
            durationSeconds: duration,
            summary: summary,
            segments: segments,
            notes: notes
        )
    }
}
