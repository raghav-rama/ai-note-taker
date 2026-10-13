import Foundation

/// Swift wrapper around the notetaker-core C FFI.
///
/// All `char*` returns from the Rust core are freed through `note_free_string`,
/// and every callback invocation hops to the main actor before touching view state.
final class CoreBridge: @unchecked Sendable {

    // MARK: - Private state

    private var handle: OpaquePointer?
    private var transcriptHandler: (@Sendable (String, Double) -> Void)?

    // MARK: - Lifecycle

    init(dbPath: String, modelPath: String) {
        handle = note_init(dbPath, modelPath)
    }

    deinit {
        // Clear the callback first so Rust cannot call back into a freed object.
        if let handle {
            note_set_transcript_callback(handle, nil, nil)
        }
        if let handle {
            note_destroy(handle)
        }
    }

    // MARK: - Helpers

    /// Consume a `char*` returned by the Rust core, converting to `String` and freeing the pointer.
    private static func consumeString(_ ptr: UnsafeMutablePointer<CChar>?) -> String? {
        guard let ptr else { return nil }
        let string = String(cString: ptr)
        note_free_string(ptr)
        return string
    }

    // MARK: - Recording

    func startRecording(deviceId: String?) -> Int32 {
        guard let handle else { return -1 }
        if let deviceId {
            return note_start_recording(handle, deviceId)
        } else {
            return note_start_recording(handle, nil)
        }
    }

    func stopRecording() -> Int32 {
        guard let handle else { return -1 }
        return note_stop_recording(handle)
    }

    func pauseRecording() -> Int32 {
        guard let handle else { return -1 }
        return note_pause_recording(handle)
    }

    // MARK: - Transcription callback

    /// Register a handler that is called on the main actor whenever the Rust core emits a transcript segment.
    func setTranscriptCallback(_ handler: @escaping @Sendable (String, Double) -> Void) {
        self.transcriptHandler = handler

        guard let handle else { return }

        // Pass an unretained reference to self as the opaque context pointer.
        let ctx = Unmanaged.passUnretained(self).toOpaque()

        // The C callback cannot capture; it receives `ctx` and reconstructs `self`.
        let callback: TranscriptCallback = { textPtr, timestamp, ctx in
            guard let ctx, let textPtr else { return }
            let bridge = Unmanaged<CoreBridge>.fromOpaque(ctx).takeUnretainedValue()
            let text = String(cString: textPtr)
            // Hop to main actor before touching view state.
            let handler = bridge.transcriptHandler
            Task { @MainActor in
                handler?(text, timestamp)
            }
        }

        note_set_transcript_callback(handle, callback, ctx)
    }

    // MARK: - Summarization

    func summarize(meetingId: Int64) -> String? {
        guard let handle else { return nil }
        return Self.consumeString(note_summarize(handle, meetingId))
    }

    // MARK: - Storage

    func listMeetings(limit: Int32 = 50, offset: Int32 = 0) -> String? {
        guard let handle else { return nil }
        return Self.consumeString(note_list_meetings(handle, limit, offset))
    }

    func getMeeting(id: Int64) -> String? {
        guard let handle else { return nil }
        return Self.consumeString(note_get_meeting(handle, id))
    }

    func deleteMeeting(id: Int64) -> Int32 {
        guard let handle else { return -1 }
        return note_delete_meeting(handle, id)
    }

    func exportMeeting(id: Int64, format: String) -> String? {
        guard let handle else { return nil }
        return Self.consumeString(note_export_meeting(handle, id, format))
    }

    // MARK: - Settings

    func setSetting(key: String, value: String) -> Int32 {
        guard let handle else { return -1 }
        return note_set_setting(handle, key, value)
    }

    func getSetting(key: String) -> String? {
        guard let handle else { return nil }
        return Self.consumeString(note_get_setting(handle, key))
    }

    // MARK: - Audio devices

    func listAudioDevices() -> String? {
        guard let handle else { return nil }
        return Self.consumeString(note_list_audio_devices(handle))
    }
}
