import Foundation
import NoteCore

// MARK: - Core Bridge

/// Swift bridge to the notetaker-core Rust library via C FFI.
///
/// All methods that return JSON strings take ownership of the pointer and free
/// it via `note_free_string`. The transcript callback arrives on the Rust audio
/// thread; it is bounced to `MainActor` before publishing state.
@MainActor
final class CoreBridge: ObservableObject {

    // MARK: Published state

    @Published private(set) var isRecording = false
    @Published private(set) var isPaused = false
    @Published private(set) var liveSegments: [LiveSegment] = []

    struct LiveSegment: Identifiable, Hashable {
        let id = UUID()
        let text: String
        let timestamp: Double
    }

    // MARK: Private

    private var handle: OpaquePointer?

    // MARK: Lifecycle

    init() {}

    /// Initialize the core library with database and model paths.
    func initialize(dbPath: String, modelPath: String) -> Bool {
        guard handle == nil else { return true }
        handle = note_init(dbPath, modelPath)
        guard handle != nil else { return false }
        installTranscriptCallback()
        return true
    }

    deinit {
        // note_destroy is safe to call on the same thread as init.
        // MainActor deinit is guaranteed in Swift 6 mode.
        if let h = handle {
            note_destroy(h)
            handle = nil
        }
    }

    // MARK: Recording

    func startRecording(deviceId: String? = nil) -> Bool {
        guard let h = handle else { return false }
        let result = note_start_recording(h, deviceId)
        if result == 0 {
            isRecording = true
            isPaused = false
            liveSegments = []
        }
        return result == 0
    }

    func stopRecording() -> Bool {
        guard let h = handle else { return false }
        let result = note_stop_recording(h)
        if result == 0 {
            isRecording = false
            isPaused = false
        }
        return result == 0
    }

    func pauseRecording() -> Bool {
        guard let h = handle else { return false }
        let result = note_pause_recording(h)
        if result == 0 {
            isPaused = true
        }
        return result == 0
    }

    // MARK: Summarization

    /// Runs summarization off the main thread since the core may call a remote API.
    func summarize(meetingId: Int64) async -> Summary? {
        guard let h = handle else { return nil }
        // OpaquePointer is not Sendable but the core handle is thread-safe.
        let safeHandle = UnsafeSendable(h)
        let safeMeetingId = meetingId
        let json: String? = await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(
                    returning: takeString(note_summarize(safeHandle.value, safeMeetingId))
                )
            }
        }
        guard let json else { return nil }
        return try? JSONDecoder.noteCore.decode(Summary.self, from: Data(json.utf8))
    }

    // MARK: Storage

    func listMeetings(limit: Int32 = 50, offset: Int32 = 0) -> [Meeting] {
        guard let h = handle else { return [] }
        guard let json = takeString(note_list_meetings(h, limit, offset)) else { return [] }
        return (try? JSONDecoder.noteCore.decode([Meeting].self, from: Data(json.utf8))) ?? []
    }

    func getMeeting(id: Int64) -> MeetingDetail? {
        guard let h = handle else { return nil }
        guard let json = takeString(note_get_meeting(h, id)) else { return nil }
        return try? JSONDecoder.noteCore.decode(MeetingDetail.self, from: Data(json.utf8))
    }

    func deleteMeeting(id: Int64) -> Bool {
        guard let h = handle else { return false }
        return note_delete_meeting(h, id) == 0
    }

    func exportMeeting(id: Int64, format: String) -> String? {
        guard let h = handle else { return nil }
        return takeString(note_export_meeting(h, id, format))
    }

    // MARK: Settings

    func setSetting(key: String, value: String) -> Bool {
        guard let h = handle else { return false }
        return note_set_setting(h, key, value) == 0
    }

    func getSetting(key: String) -> String? {
        guard let h = handle else { return nil }
        return takeString(note_get_setting(h, key))
    }

    // MARK: Audio devices

    func listAudioDevices() -> [AudioDevice] {
        guard let h = handle else { return [] }
        guard let json = takeString(note_list_audio_devices(h)) else { return [] }
        return (try? JSONDecoder.noteCore.decode([AudioDevice].self, from: Data(json.utf8))) ?? []
    }

    // MARK: Transcript callback

    /// Append a live transcript segment. Called from the callback trampoline.
    fileprivate func appendSegment(text: String, timestamp: Double) {
        liveSegments.append(LiveSegment(text: text, timestamp: timestamp))
    }

    private func installTranscriptCallback() {
        guard let h = handle else { return }
        // Pass an unretained pointer to self as the context.
        let ctx = Unmanaged.passUnretained(self).toOpaque()
        note_set_transcript_callback(h, transcriptCallbackTrampoline, ctx)
    }
}

// MARK: - C callback trampoline (file-private, @convention(c))

/// This function is called on the Rust audio thread. It recovers the
/// CoreBridge instance from the opaque context and dispatches to MainActor.
private let transcriptCallbackTrampoline: TranscriptCallback = {
    (text: UnsafePointer<CChar>?, timestamp: Double, ctx: UnsafeMutableRawPointer?) in
    guard let ctx = ctx, let text = text else { return }
    let string = String(cString: text)
    let bridge = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
    Task { @MainActor in
        bridge.appendSegment(text: string, timestamp: timestamp)
    }
}

// MARK: - String helper

/// Copies a C string returned by the core library and frees it.
private func takeString(_ ptr: UnsafeMutablePointer<CChar>?) -> String? {
    guard let ptr = ptr else { return nil }
    let string = String(cString: ptr)
    note_free_string(ptr)
    return string
}

// MARK: - Sendable wrapper for pointers

/// Wraps a non-Sendable value to cross isolation boundaries when the caller
/// guarantees thread safety (e.g. the Rust core handle is internally synchronized).
private struct UnsafeSendable<T>: @unchecked Sendable {
    let value: T
    init(_ value: T) { self.value = value }
}
