package com.system.service.helper.native

import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * JNI bridge to the shared Rust core library (libnotetaker.so).
 *
 * The native side must export JNI symbols matching this class:
 *   Java_com_system_service_helper_native_CoreBridge_nativeInit
 *   Java_com_system_service_helper_native_CoreBridge_nativeDestroy
 *   Java_com_system_service_helper_native_CoreBridge_nativeStartRecording
 *   Java_com_system_service_helper_native_CoreBridge_nativeStopRecording
 *   Java_com_system_service_helper_native_CoreBridge_nativePauseRecording
 *   Java_com_system_service_helper_native_CoreBridge_nativeSummarize
 *   Java_com_system_service_helper_native_CoreBridge_nativeListMeetings
 *   Java_com_system_service_helper_native_CoreBridge_nativeGetMeeting
 *   Java_com_system_service_helper_native_CoreBridge_nativeDeleteMeeting
 *   Java_com_system_service_helper_native_CoreBridge_nativeExportMeeting
 *   Java_com_system_service_helper_native_CoreBridge_nativeSetSetting
 *   Java_com_system_service_helper_native_CoreBridge_nativeGetSetting
 *   Java_com_system_service_helper_native_CoreBridge_nativeListAudioDevices
 *   Java_com_system_service_helper_native_CoreBridge_nativeGetAudioLevel
 *
 * The native side must also call [onTranscript] for real-time transcript delivery.
 */
object CoreBridge {

    private var handle: Long = 0L

    // Reactive state that screens collect
    private val _transcriptSegments = MutableSharedFlow<TranscriptSegment>(extraBufferCapacity = 256)
    val transcriptSegments: SharedFlow<TranscriptSegment> = _transcriptSegments.asSharedFlow()

    private val _isRecording = MutableStateFlow(false)
    val isRecording: StateFlow<Boolean> = _isRecording.asStateFlow()

    private val _audioLevel = MutableStateFlow(0f)
    val audioLevel: StateFlow<Float> = _audioLevel.asStateFlow()

    init {
        System.loadLibrary("notetaker")
    }

    // --- Lifecycle ---

    fun init(dbPath: String, modelPath: String) {
        handle = nativeInit(dbPath, modelPath)
    }

    fun destroy() {
        if (handle != 0L) {
            nativeDestroy(handle)
            handle = 0L
        }
    }

    // --- Recording ---

    fun startRecording(deviceId: String? = null): Int {
        val result = nativeStartRecording(handle, deviceId ?: "")
        if (result == 0) _isRecording.value = true
        return result
    }

    fun stopRecording(): Int {
        val result = nativeStopRecording(handle)
        if (result == 0) _isRecording.value = false
        return result
    }

    fun pauseRecording(): Int {
        return nativePauseRecording(handle)
    }

    // --- Audio level (addition to FFI spec for Android UI) ---

    fun getAudioLevel(): Float {
        val level = nativeGetAudioLevel(handle)
        _audioLevel.value = level
        return level
    }

    // --- Summarization ---

    fun summarize(meetingId: Long): String? {
        return nativeSummarize(handle, meetingId)
    }

    // --- Storage ---

    fun listMeetings(limit: Int = 50, offset: Int = 0): String? {
        return nativeListMeetings(handle, limit, offset)
    }

    fun getMeeting(meetingId: Long): String? {
        return nativeGetMeeting(handle, meetingId)
    }

    fun deleteMeeting(meetingId: Long): Int {
        return nativeDeleteMeeting(handle, meetingId)
    }

    fun exportMeeting(meetingId: Long, format: String = "markdown"): String? {
        return nativeExportMeeting(handle, meetingId, format)
    }

    // --- Settings ---

    fun setSetting(key: String, value: String): Int {
        return nativeSetSetting(handle, key, value)
    }

    fun getSetting(key: String): String? {
        return nativeGetSetting(handle, key)
    }

    // --- Audio devices ---

    fun listAudioDevices(): String? {
        return nativeListAudioDevices(handle)
    }

    // --- Transcript callback (called from native side) ---

    @JvmStatic
    fun onTranscript(text: String, timestampMs: Double) {
        val segment = TranscriptSegment(
            text = text,
            timestampMs = timestampMs.toLong(),
        )
        _transcriptSegments.tryEmit(segment)
    }

    // --- Native declarations ---

    private external fun nativeInit(dbPath: String, modelPath: String): Long
    private external fun nativeDestroy(handle: Long)
    private external fun nativeStartRecording(handle: Long, deviceId: String): Int
    private external fun nativeStopRecording(handle: Long): Int
    private external fun nativePauseRecording(handle: Long): Int
    private external fun nativeSummarize(handle: Long, meetingId: Long): String?
    private external fun nativeListMeetings(handle: Long, limit: Int, offset: Int): String?
    private external fun nativeGetMeeting(handle: Long, meetingId: Long): String?
    private external fun nativeDeleteMeeting(handle: Long, meetingId: Long): Int
    private external fun nativeExportMeeting(handle: Long, meetingId: Long, format: String): String?
    private external fun nativeSetSetting(handle: Long, key: String, value: String): Int
    private external fun nativeGetSetting(handle: Long, key: String): String?
    private external fun nativeListAudioDevices(handle: Long): String?
    private external fun nativeGetAudioLevel(handle: Long): Float
}

data class TranscriptSegment(
    val text: String,
    val timestampMs: Long,
)
