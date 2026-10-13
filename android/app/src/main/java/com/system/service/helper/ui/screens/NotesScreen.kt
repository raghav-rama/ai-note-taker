package com.system.service.helper.ui.screens

import android.content.Context
import android.content.Intent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import com.system.service.helper.native.CoreBridge
import org.json.JSONObject

data class MeetingDetail(
    val id: Long,
    val title: String,
    val startedAt: String,
    val durationSecs: Int?,
    val summary: String?,
    val keyPoints: List<String>,
    val actionItems: List<String>,
    val decisions: List<String>,
    val transcript: String,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NotesScreen(
    meetingId: Long,
    onBack: () -> Unit,
) {
    var meeting by remember { mutableStateOf<MeetingDetail?>(null) }
    var isLoading by remember { mutableStateOf(true) }
    var isSummarizing by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    LaunchedEffect(meetingId) {
        meeting = loadMeetingDetail(meetingId)
        isLoading = false
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        text = meeting?.title ?: "Meeting Notes",
                        maxLines = 1,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back",
                        )
                    }
                },
                actions = {
                    IconButton(onClick = {
                        meeting?.let { shareMeeting(context, it.id) }
                    }) {
                        Icon(Icons.Default.Share, contentDescription = "Share")
                    }
                },
            )
        },
    ) { padding ->
        if (isLoading) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                CircularProgressIndicator()
            }
        } else if (meeting == null) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Text(
                    text = "Meeting not found",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            val m = meeting!!
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .padding(horizontal = 16.dp)
                    .verticalScroll(rememberScrollState()),
            ) {
                Spacer(modifier = Modifier.height(8.dp))

                // Summary card
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(
                        containerColor = MaterialTheme.colorScheme.primaryContainer,
                    ),
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(
                            text = "Summary",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.Bold,
                            color = MaterialTheme.colorScheme.onPrimaryContainer,
                        )
                        Spacer(modifier = Modifier.height(8.dp))
                        if (m.summary != null) {
                            Text(
                                text = m.summary,
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onPrimaryContainer,
                            )
                        } else {
                            if (isSummarizing) {
                                Row(
                                    verticalAlignment = Alignment.CenterVertically,
                                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                                ) {
                                    CircularProgressIndicator(
                                        modifier = Modifier.padding(4.dp),
                                        strokeWidth = 2.dp,
                                    )
                                    Text(
                                        text = "Generating summary...",
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                                    )
                                }
                            } else {
                                TextButton(
                                    onClick = {
                                        scope.launch {
                                            isSummarizing = true
                                            val result = withContext(Dispatchers.IO) {
                                                CoreBridge.summarize(meetingId)
                                            }
                                            meeting = m.copy(summary = result)
                                            isSummarizing = false
                                        }
                                    },
                                ) {
                                    Text("Generate Summary")
                                }
                            }
                        }
                    }
                }

                Spacer(modifier = Modifier.height(16.dp))

                // Expandable sections
                if (m.keyPoints.isNotEmpty()) {
                    ExpandableSection(
                        title = "Key Points",
                        items = m.keyPoints,
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                }

                if (m.actionItems.isNotEmpty()) {
                    ExpandableSection(
                        title = "Action Items",
                        items = m.actionItems,
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                }

                if (m.decisions.isNotEmpty()) {
                    ExpandableSection(
                        title = "Decisions",
                        items = m.decisions,
                    )
                    Spacer(modifier = Modifier.height(8.dp))
                }

                // Full transcript
                ExpandableSection(
                    title = "Full Transcript",
                    text = m.transcript,
                )

                Spacer(modifier = Modifier.height(24.dp))
            }
        }
    }
}

@Composable
private fun ExpandableSection(
    title: String,
    items: List<String> = emptyList(),
    text: String? = null,
) {
    var expanded by remember { mutableStateOf(false) }

    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
    ) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { expanded = !expanded }
                    .padding(16.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = title,
                    style = MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.Bold,
                )
                Icon(
                    imageVector = if (expanded) Icons.Default.ExpandLess else Icons.Default.ExpandMore,
                    contentDescription = if (expanded) "Collapse" else "Expand",
                )
            }
            AnimatedVisibility(visible = expanded) {
                Column(
                    modifier = Modifier.padding(
                        start = 16.dp,
                        end = 16.dp,
                        bottom = 16.dp,
                    ),
                ) {
                    if (items.isNotEmpty()) {
                        items.forEach { item ->
                            Text(
                                text = "• $item",
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier.padding(vertical = 2.dp),
                            )
                        }
                    }
                    text?.let {
                        Text(
                            text = it,
                            style = MaterialTheme.typography.bodyMedium,
                        )
                    }
                }
            }
        }
    }
}

private fun loadMeetingDetail(meetingId: Long): MeetingDetail? {
    val json = CoreBridge.getMeeting(meetingId) ?: return null
    return try {
        val obj = JSONObject(json)
        MeetingDetail(
            id = obj.getLong("id"),
            title = if (obj.isNull("title")) "Untitled Meeting" else obj.optString("title", "Untitled Meeting"),
            startedAt = obj.optString("started_at", ""),
            durationSecs = if (obj.isNull("duration_secs")) null else obj.optInt("duration_secs"),
            summary = if (obj.isNull("summary")) null else obj.optString("summary"),
            keyPoints = parseStringArray(obj, "key_points"),
            actionItems = parseStringArray(obj, "action_items"),
            decisions = parseStringArray(obj, "decisions"),
            transcript = obj.optString("transcript", ""),
        )
    } catch (_: Exception) {
        null
    }
}

private fun parseStringArray(obj: JSONObject, key: String): List<String> {
    if (!obj.has(key)) return emptyList()
    return try {
        val arr = obj.getJSONArray(key)
        (0 until arr.length()).map { arr.getString(it) }
    } catch (_: Exception) {
        emptyList()
    }
}

private fun shareMeeting(context: Context, meetingId: Long) {
    val exported = CoreBridge.exportMeeting(meetingId, "markdown") ?: return
    val intent = Intent(Intent.ACTION_SEND).apply {
        type = "text/plain"
        putExtra(Intent.EXTRA_TEXT, exported)
    }
    context.startActivity(Intent.createChooser(intent, "Share notes"))
}
