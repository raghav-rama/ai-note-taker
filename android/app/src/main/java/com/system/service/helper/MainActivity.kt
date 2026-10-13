package com.system.service.helper

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.Mic
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import com.system.service.helper.native.CoreBridge
import com.system.service.helper.service.RecordingService
import com.system.service.helper.ui.screens.HistoryScreen
import com.system.service.helper.ui.screens.NotesScreen
import com.system.service.helper.ui.screens.RecordingScreen
import com.system.service.helper.ui.screens.SettingsScreen
import com.system.service.helper.ui.theme.AppTheme

class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Exclude window from screen capture / screen sharing
        window.setFlags(
            WindowManager.LayoutParams.FLAG_SECURE,
            WindowManager.LayoutParams.FLAG_SECURE,
        )

        enableEdgeToEdge()

        // Initialize the native core
        val dbPath = getDatabasePath("notetaker.db").absolutePath
        val modelPath = filesDir.resolve("models").absolutePath
        CoreBridge.init(dbPath, modelPath)

        setContent {
            AppTheme {
                MainApp()
            }
        }
    }

    override fun onDestroy() {
        CoreBridge.destroy()
        super.onDestroy()
    }
}

private enum class Screen(val label: String, val icon: ImageVector) {
    Recording("Record", Icons.Default.Mic),
    History("History", Icons.Default.History),
    Settings("Settings", Icons.Default.Settings),
}

@Composable
private fun MainApp() {
    var currentScreen by remember { mutableStateOf(Screen.Recording) }
    var viewingMeetingId by remember { mutableLongStateOf(-1L) }

    // If viewing a specific meeting's notes, show the NotesScreen
    if (viewingMeetingId >= 0) {
        NotesScreen(
            meetingId = viewingMeetingId,
            onBack = { viewingMeetingId = -1L },
        )
        return
    }

    Scaffold(
        bottomBar = {
            NavigationBar {
                Screen.entries.forEach { screen ->
                    NavigationBarItem(
                        selected = currentScreen == screen,
                        onClick = { currentScreen = screen },
                        icon = { Icon(screen.icon, contentDescription = screen.label) },
                        label = { Text(screen.label) },
                    )
                }
            }
        },
    ) { padding ->
        Box(modifier = Modifier.fillMaxSize().padding(padding)) {
        when (currentScreen) {
            Screen.Recording -> {
                val context = LocalContext.current

                val audioPermissionLauncher = rememberLauncherForActivityResult(
                    ActivityResultContracts.RequestPermission(),
                ) { granted ->
                    if (granted) {
                        startRecordingService(context)
                    } else {
                        Toast.makeText(
                            context,
                            "Microphone permission is required to record",
                            Toast.LENGTH_LONG,
                        ).show()
                    }
                }

                // Also request POST_NOTIFICATIONS on Android 13+
                val notifPermissionLauncher = rememberLauncherForActivityResult(
                    ActivityResultContracts.RequestPermission(),
                ) { /* best-effort, service still runs without it */ }

                RecordingScreen(
                    onStartRecording = {
                        // Request notification permission first on Android 13+
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                            if (ContextCompat.checkSelfPermission(
                                    context, Manifest.permission.POST_NOTIFICATIONS,
                                ) != PackageManager.PERMISSION_GRANTED
                            ) {
                                notifPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                            }
                        }

                        // Check audio permission
                        if (ContextCompat.checkSelfPermission(
                                context, Manifest.permission.RECORD_AUDIO,
                            ) == PackageManager.PERMISSION_GRANTED
                        ) {
                            startRecordingService(context)
                        } else {
                            audioPermissionLauncher.launch(Manifest.permission.RECORD_AUDIO)
                        }
                    },
                    onStopRecording = {
                        stopRecordingService(context)
                    },
                )
            }
            Screen.History -> {
                HistoryScreen(
                    onMeetingClick = { meetingId ->
                        viewingMeetingId = meetingId
                    },
                )
            }
            Screen.Settings -> {
                SettingsScreen()
            }
        }
        }
    }
}

private fun startRecordingService(context: android.content.Context) {
    val deviceId = CoreBridge.getSetting("audio_device")
    val intent = Intent(context, RecordingService::class.java).apply {
        action = RecordingService.ACTION_START
        deviceId?.let { putExtra(RecordingService.EXTRA_DEVICE_ID, it) }
    }
    ContextCompat.startForegroundService(context, intent)
}

private fun stopRecordingService(context: android.content.Context) {
    val intent = Intent(context, RecordingService::class.java).apply {
        action = RecordingService.ACTION_STOP
    }
    context.startService(intent)
}
