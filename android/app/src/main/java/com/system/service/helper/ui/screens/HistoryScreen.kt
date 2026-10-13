package com.system.service.helper.ui.screens

import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SearchBar
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.system.service.helper.native.CoreBridge
import org.json.JSONArray
import org.json.JSONObject

data class MeetingSummary(
    val id: Long,
    val title: String,
    val startedAt: String,
    val durationSecs: Int?,
    val summary: String?,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HistoryScreen(
    onMeetingClick: (Long) -> Unit,
) {
    var searchQuery by remember { mutableStateOf("") }
    var searchActive by remember { mutableStateOf(false) }
    val meetings = remember { mutableStateListOf<MeetingSummary>() }

    // Load meetings from core
    LaunchedEffect(Unit) {
        loadMeetings(meetings)
    }

    val filteredMeetings = if (searchQuery.isBlank()) {
        meetings
    } else {
        meetings.filter { meeting ->
            meeting.title.contains(searchQuery, ignoreCase = true) ||
                meeting.summary?.contains(searchQuery, ignoreCase = true) == true
        }
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp),
    ) {
        Spacer(modifier = Modifier.height(16.dp))

        // Search bar
        SearchBar(
            inputField = {
                SearchBarDefaults.InputField(
                    query = searchQuery,
                    onQueryChange = { searchQuery = it },
                    onSearch = { searchActive = false },
                    expanded = false,
                    onExpandedChange = { searchActive = it },
                    placeholder = { Text("Search meetings") },
                    leadingIcon = {
                        Icon(Icons.Default.Search, contentDescription = "Search")
                    },
                )
            },
            expanded = false,
            onExpandedChange = { searchActive = it },
            modifier = Modifier.fillMaxWidth(),
        ) {}

        Spacer(modifier = Modifier.height(16.dp))

        if (filteredMeetings.isEmpty()) {
            Box(
                modifier = Modifier.fillMaxSize(),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = if (meetings.isEmpty()) {
                        "No meetings yet.\nStart recording to capture your first meeting."
                    } else {
                        "No meetings match your search."
                    },
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            LazyColumn(
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(
                    items = filteredMeetings,
                    key = { it.id },
                ) { meeting ->
                    SwipeToDismissMeetingItem(
                        meeting = meeting,
                        onClick = { onMeetingClick(meeting.id) },
                        onDismiss = {
                            CoreBridge.deleteMeeting(meeting.id)
                            meetings.remove(meeting)
                        },
                    )
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SwipeToDismissMeetingItem(
    meeting: MeetingSummary,
    onClick: () -> Unit,
    onDismiss: () -> Unit,
) {
    val dismissState = rememberSwipeToDismissBoxState(
        confirmValueChange = { value ->
            if (value == SwipeToDismissBoxValue.EndToStart) {
                onDismiss()
                true
            } else {
                false
            }
        },
    )

    SwipeToDismissBox(
        state = dismissState,
        backgroundContent = {
            val color by animateColorAsState(
                targetValue = when (dismissState.targetValue) {
                    SwipeToDismissBoxValue.EndToStart -> MaterialTheme.colorScheme.errorContainer
                    else -> MaterialTheme.colorScheme.surface
                },
                label = "dismiss-bg",
            )
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .background(color)
                    .padding(horizontal = 24.dp),
                contentAlignment = Alignment.CenterEnd,
            ) {
                Icon(
                    Icons.Default.Delete,
                    contentDescription = "Delete",
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        },
        enableDismissFromStartToEnd = false,
    ) {
        MeetingCard(meeting = meeting, onClick = onClick)
    }
}

@Composable
private fun MeetingCard(
    meeting: MeetingSummary,
    onClick: () -> Unit,
) {
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
        ),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = meeting.title,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )

            Spacer(modifier = Modifier.height(4.dp))

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                Text(
                    text = meeting.startedAt,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                meeting.durationSecs?.let { secs ->
                    Text(
                        text = formatMeetingDuration(secs),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            meeting.summary?.let { summary ->
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    text = summary,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

private fun loadMeetings(target: MutableList<MeetingSummary>) {
    val json = CoreBridge.listMeetings() ?: return
    try {
        val array = JSONArray(json)
        target.clear()
        for (i in 0 until array.length()) {
            val obj: JSONObject = array.getJSONObject(i)
            target.add(
                MeetingSummary(
                    id = obj.getLong("id"),
                    title = if (obj.isNull("title")) "Untitled Meeting" else obj.optString("title", "Untitled Meeting"),
                    startedAt = obj.optString("started_at", ""),
                    durationSecs = if (obj.isNull("duration_secs")) null else obj.optInt("duration_secs"),
                    summary = if (obj.isNull("summary")) null else obj.optString("summary"),
                ),
            )
        }
    } catch (_: Exception) {
        // JSON parsing failed; leave list empty
    }
}

private fun formatMeetingDuration(totalSeconds: Int): String {
    val hours = totalSeconds / 3600
    val minutes = (totalSeconds % 3600) / 60
    return if (hours > 0) {
        "${hours}h ${minutes}m"
    } else {
        "${minutes}m"
    }
}
