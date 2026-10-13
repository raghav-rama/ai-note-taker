package com.system.service.helper.ui.screens

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.system.service.helper.native.CoreBridge
import com.system.service.helper.native.TranscriptSegment
import kotlinx.coroutines.delay

@Composable
fun RecordingScreen(
    onStartRecording: () -> Unit,
    onStopRecording: () -> Unit,
) {
    val isRecording by CoreBridge.isRecording.collectAsStateWithLifecycle()
    val audioLevel by CoreBridge.audioLevel.collectAsStateWithLifecycle()

    val segments = remember { mutableStateListOf<TranscriptSegment>() }
    val listState = rememberLazyListState()

    // Elapsed time
    var elapsedSeconds by remember { mutableLongStateOf(0L) }

    // Collect transcript segments
    LaunchedEffect(Unit) {
        CoreBridge.transcriptSegments.collect { segment ->
            segments.add(segment)
        }
    }

    // Timer + clear segments on new recording
    LaunchedEffect(isRecording) {
        if (isRecording) {
            segments.clear()
            elapsedSeconds = 0L
            while (true) {
                delay(1000L)
                elapsedSeconds++
            }
        }
    }

    // Auto-scroll to bottom
    LaunchedEffect(segments.size) {
        if (segments.isNotEmpty()) {
            listState.animateScrollToItem(segments.size - 1)
        }
    }

    // Poll audio level while recording
    LaunchedEffect(isRecording) {
        while (isRecording) {
            CoreBridge.getAudioLevel()
            delay(100L)
        }
    }

    Scaffold(
        floatingActionButton = {
            FloatingActionButton(
                onClick = {
                    if (isRecording) onStopRecording() else onStartRecording()
                },
                containerColor = if (isRecording) {
                    MaterialTheme.colorScheme.error
                } else {
                    MaterialTheme.colorScheme.primary
                },
            ) {
                Icon(
                    imageVector = if (isRecording) Icons.Default.Stop else Icons.Default.Mic,
                    contentDescription = if (isRecording) "Stop recording" else "Start recording",
                    tint = if (isRecording) {
                        MaterialTheme.colorScheme.onError
                    } else {
                        MaterialTheme.colorScheme.onPrimary
                    },
                )
            }
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .padding(horizontal = 16.dp),
        ) {
            Spacer(modifier = Modifier.height(16.dp))

            // Timer and status
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (isRecording) {
                    RecordingIndicator()
                    Spacer(modifier = Modifier.width(12.dp))
                }
                Text(
                    text = formatDuration(elapsedSeconds),
                    style = MaterialTheme.typography.headlineLarge,
                    fontFamily = FontFamily.Monospace,
                    color = if (isRecording) {
                        MaterialTheme.colorScheme.error
                    } else {
                        MaterialTheme.colorScheme.onSurface
                    },
                )
            }

            Spacer(modifier = Modifier.height(16.dp))

            // Audio level meter
            if (isRecording) {
                AudioLevelBar(
                    level = audioLevel,
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(8.dp),
                )
                Spacer(modifier = Modifier.height(16.dp))
            }

            // Status text
            Text(
                text = when {
                    isRecording && segments.isEmpty() -> "Listening..."
                    isRecording -> "Transcribing..."
                    segments.isEmpty() -> "Tap the microphone to start recording"
                    else -> "Recording complete"
                },
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )

            Spacer(modifier = Modifier.height(12.dp))

            // Live transcript
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(segments) { segment ->
                    TranscriptItem(segment)
                }
            }
        }
    }
}

@Composable
private fun TranscriptItem(segment: TranscriptSegment) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Text(
            text = formatTimestamp(segment.timestampMs),
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = segment.text,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurface,
        )
    }
}

@Composable
private fun RecordingIndicator() {
    val infiniteTransition = rememberInfiniteTransition(label = "recording")
    val alpha by infiniteTransition.animateFloat(
        initialValue = 1f,
        targetValue = 0.2f,
        animationSpec = infiniteRepeatable(
            animation = tween(800),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "pulse",
    )
    val color = MaterialTheme.colorScheme.error
    Canvas(modifier = Modifier.size(12.dp)) {
        drawCircle(
            color = color.copy(alpha = alpha),
            radius = size.minDimension / 2,
        )
    }
}

@Composable
private fun AudioLevelBar(level: Float, modifier: Modifier = Modifier) {
    val color = MaterialTheme.colorScheme.primary
    val trackColor = MaterialTheme.colorScheme.surfaceVariant

    Canvas(modifier = modifier) {
        // Background track
        drawLine(
            color = trackColor,
            start = Offset(0f, size.height / 2),
            end = Offset(size.width, size.height / 2),
            strokeWidth = size.height,
            cap = StrokeCap.Round,
        )
        // Level fill
        val fillWidth = size.width * level.coerceIn(0f, 1f)
        if (fillWidth > 0f) {
            drawLine(
                color = color,
                start = Offset(0f, size.height / 2),
                end = Offset(fillWidth, size.height / 2),
                strokeWidth = size.height,
                cap = StrokeCap.Round,
            )
        }
    }
}

private fun formatDuration(totalSeconds: Long): String {
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    val seconds = totalSeconds % 60
    return if (hours > 0) {
        "%d:%02d:%02d".format(hours, minutes, seconds)
    } else {
        "%02d:%02d".format(minutes, seconds)
    }
}

private fun formatTimestamp(ms: Long): String {
    val totalSeconds = ms / 1000
    val minutes = totalSeconds / 60
    val seconds = totalSeconds % 60
    return "%02d:%02d".format(minutes, seconds)
}
