#ifndef NOTETAKER_H
#define NOTETAKER_H

#include <stdint.h>

typedef struct NoteHandle NoteHandle;

// Lifecycle
NoteHandle* note_init(const char* db_path, const char* model_path);
void note_destroy(NoteHandle* handle);

// Recording
int32_t note_start_recording(NoteHandle* handle, const char* device_id);
int32_t note_stop_recording(NoteHandle* handle);
int32_t note_pause_recording(NoteHandle* handle);

// Transcription callbacks
typedef void (*TranscriptCallback)(const char* text, double timestamp, void* ctx);
void note_set_transcript_callback(NoteHandle* handle, TranscriptCallback cb, void* ctx);

// Summarization
char* note_summarize(NoteHandle* handle, int64_t meeting_id);
void note_free_string(char* s);

// Storage
char* note_list_meetings(NoteHandle* handle, int32_t limit, int32_t offset);
char* note_get_meeting(NoteHandle* handle, int64_t meeting_id);
int32_t note_delete_meeting(NoteHandle* handle, int64_t meeting_id);
char* note_export_meeting(NoteHandle* handle, int64_t meeting_id, const char* format);

// Settings
int32_t note_set_setting(NoteHandle* handle, const char* key, const char* value);
char* note_get_setting(NoteHandle* handle, const char* key);

// Audio devices
char* note_list_audio_devices(NoteHandle* handle);

#endif /* NOTETAKER_H */
