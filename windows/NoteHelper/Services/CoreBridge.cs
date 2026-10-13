using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

namespace NoteHelper.Services;

/// <summary>
/// P/Invoke bridge to the shared Rust core library (notetaker.dll).
/// Provides managed wrappers around all C FFI functions.
/// </summary>
public sealed class CoreBridge : IDisposable
{
    private IntPtr _handle;
    private bool _disposed;

    // Store delegate references to prevent garbage collection during callbacks
    private NativeTranscriptCallback? _nativeCallback;

    private const string DllName = "notetaker";

    #region Native Function Imports

    // --- Lifecycle ---

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
        CharSet = CharSet.Ansi)]
    private static extern IntPtr note_init(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string dbPath,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string modelPath);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void note_destroy(IntPtr handle);

    // --- Recording ---

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
        CharSet = CharSet.Ansi)]
    private static extern int note_start_recording(IntPtr handle,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string? deviceId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int note_stop_recording(IntPtr handle);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int note_pause_recording(IntPtr handle);

    // --- Transcript Callback ---

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void NativeTranscriptCallback(
        IntPtr text, double timestamp, IntPtr ctx);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void note_set_transcript_callback(
        IntPtr handle, NativeTranscriptCallback? cb, IntPtr ctx);

    // --- Summarization ---

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr note_summarize(IntPtr handle, long meetingId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern void note_free_string(IntPtr s);

    // --- Storage ---

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr note_list_meetings(IntPtr handle, int limit, int offset);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr note_get_meeting(IntPtr handle, long meetingId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern int note_delete_meeting(IntPtr handle, long meetingId);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
        CharSet = CharSet.Ansi)]
    private static extern IntPtr note_export_meeting(IntPtr handle, long meetingId,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string format);

    // --- Settings ---

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
        CharSet = CharSet.Ansi)]
    private static extern int note_set_setting(IntPtr handle,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string key,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string value);

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl,
        CharSet = CharSet.Ansi)]
    private static extern IntPtr note_get_setting(IntPtr handle,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string key);

    // --- Audio Devices ---

    [DllImport(DllName, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr note_list_audio_devices(IntPtr handle);

    #endregion

    #region Managed Wrappers

    /// <summary>
    /// Initializes the core library with paths to the database and whisper model.
    /// </summary>
    /// <param name="dbPath">Path to the SQLite database file.</param>
    /// <param name="modelPath">Path to the whisper model file (.bin).</param>
    /// <exception cref="InvalidOperationException">Thrown if initialization fails.</exception>
    public void Initialize(string dbPath, string modelPath)
    {
        ThrowIfDisposed();

        if (_handle != IntPtr.Zero)
            throw new InvalidOperationException("CoreBridge is already initialized.");

        _handle = note_init(dbPath, modelPath);
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException(
                "Failed to initialize notetaker core. Check db_path and model_path.");
    }

    /// <summary>
    /// Starts recording audio from the specified device (or default if null).
    /// </summary>
    /// <returns>0 on success, non-zero on error.</returns>
    public int StartRecording(string? deviceId = null)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return note_start_recording(_handle, deviceId);
    }

    /// <summary>
    /// Stops the current recording.
    /// </summary>
    /// <returns>0 on success, non-zero on error.</returns>
    public int StopRecording()
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return note_stop_recording(_handle);
    }

    /// <summary>
    /// Pauses the current recording.
    /// </summary>
    /// <returns>0 on success, non-zero on error.</returns>
    public int PauseRecording()
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return note_pause_recording(_handle);
    }

    /// <summary>
    /// Managed callback delegate for transcript events.
    /// </summary>
    public delegate void TranscriptHandler(string text, double timestamp);

    /// <summary>
    /// Sets the transcript callback. The handler is called on the native thread;
    /// callers must dispatch to the UI thread if needed.
    /// </summary>
    public void SetTranscriptCallback(TranscriptHandler? handler)
    {
        ThrowIfDisposed();
        EnsureInitialized();

        if (handler is null)
        {
            _nativeCallback = null;
            note_set_transcript_callback(_handle, null, IntPtr.Zero);
            return;
        }

        _nativeCallback = (textPtr, timestamp, ctx) =>
        {
            string text = Marshal.PtrToStringUTF8(textPtr) ?? string.Empty;
            handler(text, timestamp);
        };

        note_set_transcript_callback(_handle, _nativeCallback, IntPtr.Zero);
    }

    /// <summary>
    /// Generates a summary for the specified meeting.
    /// </summary>
    /// <returns>The summary text, or null on error.</returns>
    public string? Summarize(long meetingId)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return ConsumeNativeString(note_summarize(_handle, meetingId));
    }

    /// <summary>
    /// Lists meetings from storage.
    /// </summary>
    /// <returns>JSON array of meetings, or null on error.</returns>
    public string? ListMeetings(int limit = 50, int offset = 0)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return ConsumeNativeString(note_list_meetings(_handle, limit, offset));
    }

    /// <summary>
    /// Gets a single meeting by ID.
    /// </summary>
    /// <returns>JSON object of the meeting, or null on error.</returns>
    public string? GetMeeting(long meetingId)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return ConsumeNativeString(note_get_meeting(_handle, meetingId));
    }

    /// <summary>
    /// Deletes a meeting by ID.
    /// </summary>
    /// <returns>0 on success, non-zero on error.</returns>
    public int DeleteMeeting(long meetingId)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return note_delete_meeting(_handle, meetingId);
    }

    /// <summary>
    /// Exports a meeting in the specified format (markdown, text, json).
    /// </summary>
    /// <returns>Exported content as a string, or null on error.</returns>
    public string? ExportMeeting(long meetingId, string format = "markdown")
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return ConsumeNativeString(note_export_meeting(_handle, meetingId, format));
    }

    /// <summary>
    /// Sets a setting key-value pair.
    /// </summary>
    /// <returns>0 on success, non-zero on error.</returns>
    public int SetSetting(string key, string value)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return note_set_setting(_handle, key, value);
    }

    /// <summary>
    /// Gets a setting value by key.
    /// </summary>
    /// <returns>The setting value, or null if not found.</returns>
    public string? GetSetting(string key)
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return ConsumeNativeString(note_get_setting(_handle, key));
    }

    /// <summary>
    /// Lists available audio input devices.
    /// </summary>
    /// <returns>JSON array of audio devices, or null on error.</returns>
    public string? ListAudioDevices()
    {
        ThrowIfDisposed();
        EnsureInitialized();
        return ConsumeNativeString(note_list_audio_devices(_handle));
    }

    #endregion

    #region Helpers

    /// <summary>
    /// Reads and frees a native UTF-8 string returned by the core library.
    /// </summary>
    private static string? ConsumeNativeString(IntPtr ptr)
    {
        if (ptr == IntPtr.Zero) return null;

        string result = Marshal.PtrToStringUTF8(ptr) ?? string.Empty;
        note_free_string(ptr);
        return result;
    }

    private void EnsureInitialized()
    {
        if (_handle == IntPtr.Zero)
            throw new InvalidOperationException(
                "CoreBridge is not initialized. Call Initialize() first.");
    }

    private void ThrowIfDisposed()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);
    }

    #endregion

    #region IDisposable

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;

        if (_handle != IntPtr.Zero)
        {
            // Remove callback before destroying to avoid calling into freed memory
            _nativeCallback = null;
            note_set_transcript_callback(_handle, null, IntPtr.Zero);
            note_destroy(_handle);
            _handle = IntPtr.Zero;
        }
    }

    #endregion
}
