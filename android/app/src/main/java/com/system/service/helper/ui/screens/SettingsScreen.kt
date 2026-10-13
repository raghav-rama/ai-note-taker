package com.system.service.helper.ui.screens

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
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material.icons.filled.VisibilityOff
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuAnchorType
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import com.system.service.helper.native.CoreBridge
import org.json.JSONArray
import org.json.JSONObject

data class AudioDevice(
    val id: String,
    val name: String,
)

@Composable
fun SettingsScreen() {
    var whisperApiKey by remember { mutableStateOf("") }
    var claudeApiKey by remember { mutableStateOf("") }
    var useCloudTranscription by remember { mutableStateOf(false) }
    var selectedDevice by remember { mutableStateOf<AudioDevice?>(null) }
    var devices by remember { mutableStateOf(listOf<AudioDevice>()) }
    var selectedModel by remember { mutableStateOf("tiny") }

    // Load settings from core
    LaunchedEffect(Unit) {
        whisperApiKey = CoreBridge.getSetting("whisper_api_key") ?: ""
        claudeApiKey = CoreBridge.getSetting("claude_api_key") ?: ""
        useCloudTranscription = CoreBridge.getSetting("use_cloud") == "true"
        selectedModel = CoreBridge.getSetting("whisper_model") ?: "tiny"
        devices = loadAudioDevices()
        val savedDeviceId = CoreBridge.getSetting("audio_device")
        selectedDevice = devices.find { it.id == savedDeviceId } ?: devices.firstOrNull()
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 16.dp)
            .verticalScroll(rememberScrollState()),
    ) {
        Spacer(modifier = Modifier.height(16.dp))

        // Audio device section
        SectionHeader("Audio Input")
        AudioDevicePicker(
            devices = devices,
            selected = selectedDevice,
            onSelect = { device ->
                selectedDevice = device
                CoreBridge.setSetting("audio_device", device.id)
            },
        )

        Spacer(modifier = Modifier.height(24.dp))

        // Transcription section
        SectionHeader("Transcription")

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(vertical = 8.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = "Cloud Transcription",
                    style = MaterialTheme.typography.bodyLarge,
                )
                Text(
                    text = "Use OpenAI Whisper API instead of local model",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(
                checked = useCloudTranscription,
                onCheckedChange = {
                    useCloudTranscription = it
                    CoreBridge.setSetting("use_cloud", it.toString())
                },
            )
        }

        Spacer(modifier = Modifier.height(8.dp))

        // Model picker
        ModelPicker(
            selectedModel = selectedModel,
            onModelSelected = { model ->
                selectedModel = model
                CoreBridge.setSetting("whisper_model", model)
            },
        )

        Spacer(modifier = Modifier.height(24.dp))

        // API keys section
        SectionHeader("API Keys")
        Text(
            text = "Keys are stored locally on this device.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            // TODO: Use Android Keystore for secure storage
        )

        Spacer(modifier = Modifier.height(8.dp))

        ApiKeyField(
            label = "OpenAI API Key",
            value = whisperApiKey,
            onValueChange = {
                whisperApiKey = it
                CoreBridge.setSetting("whisper_api_key", it)
            },
            placeholder = "sk-...",
        )

        Spacer(modifier = Modifier.height(12.dp))

        ApiKeyField(
            label = "Claude API Key",
            value = claudeApiKey,
            onValueChange = {
                claudeApiKey = it
                CoreBridge.setSetting("claude_api_key", it)
            },
            placeholder = "sk-ant-...",
        )

        Spacer(modifier = Modifier.height(24.dp))

        // About section
        SectionHeader("About")
        Card(
            modifier = Modifier.fillMaxWidth(),
            colors = CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerLow,
            ),
        ) {
            Column(modifier = Modifier.padding(16.dp)) {
                Text(
                    text = "System Service",
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    text = "Version 1.0.0",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    text = "All data is stored locally on your device. No telemetry or analytics.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        Spacer(modifier = Modifier.height(24.dp))
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        fontWeight = FontWeight.Bold,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(bottom = 8.dp),
    )
}

@Composable
private fun ApiKeyField(
    label: String,
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
) {
    var visible by remember { mutableStateOf(false) }

    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(label) },
        placeholder = { Text(placeholder) },
        modifier = Modifier.fillMaxWidth(),
        singleLine = true,
        visualTransformation = if (visible) {
            VisualTransformation.None
        } else {
            PasswordVisualTransformation()
        },
        trailingIcon = {
            IconButton(onClick = { visible = !visible }) {
                Icon(
                    imageVector = if (visible) Icons.Default.VisibilityOff else Icons.Default.Visibility,
                    contentDescription = if (visible) "Hide" else "Show",
                )
            }
        },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AudioDevicePicker(
    devices: List<AudioDevice>,
    selected: AudioDevice?,
    onSelect: (AudioDevice) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }

    ExposedDropdownMenuBox(
        expanded = expanded,
        onExpandedChange = { expanded = it },
    ) {
        OutlinedTextField(
            value = selected?.name ?: "Default",
            onValueChange = {},
            readOnly = true,
            label = { Text("Audio Device") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
            modifier = Modifier
                .fillMaxWidth()
                .menuAnchor(MenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
        ) {
            devices.forEach { device ->
                DropdownMenuItem(
                    text = { Text(device.name) },
                    onClick = {
                        onSelect(device)
                        expanded = false
                    },
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ModelPicker(
    selectedModel: String,
    onModelSelected: (String) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }

    val models = listOf(
        "tiny" to "Tiny (~75 MB)",
        "base" to "Base (~142 MB)",
        "small" to "Small (~466 MB)",
        "medium" to "Medium (~1.5 GB)",
    )

    val selectedLabel = models.find { it.first == selectedModel }?.second ?: selectedModel

    ExposedDropdownMenuBox(
        expanded = expanded,
        onExpandedChange = { expanded = it },
    ) {
        OutlinedTextField(
            value = selectedLabel,
            onValueChange = {},
            readOnly = true,
            label = { Text("Whisper Model") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = expanded) },
            modifier = Modifier
                .fillMaxWidth()
                .menuAnchor(MenuAnchorType.PrimaryNotEditable),
        )
        ExposedDropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
        ) {
            models.forEach { (id, label) ->
                DropdownMenuItem(
                    text = { Text(label) },
                    onClick = {
                        onModelSelected(id)
                        expanded = false
                    },
                )
            }
        }
    }
}

private fun loadAudioDevices(): List<AudioDevice> {
    val json = CoreBridge.listAudioDevices() ?: return emptyList()
    return try {
        val array = JSONArray(json)
        (0 until array.length()).map { i ->
            val obj: JSONObject = array.getJSONObject(i)
            AudioDevice(
                id = obj.getString("id"),
                name = obj.getString("name"),
            )
        }
    } catch (_: Exception) {
        emptyList()
    }
}
