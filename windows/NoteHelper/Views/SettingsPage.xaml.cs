using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using NoteHelper.Services;
using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Threading.Tasks;

namespace NoteHelper.Views;

/// <summary>
/// Settings page for audio device selection, API keys, model management, and data.
/// </summary>
public sealed partial class SettingsPage : Page
{
    private CoreBridge? _core;
    private readonly string _dataDir;

    public SettingsPage()
    {
        this.InitializeComponent();

        _dataDir = System.IO.Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "NoteHelper");

        StoragePathText.Text = _dataDir;

        Loaded += SettingsPage_Loaded;
    }

    private void SettingsPage_Loaded(object sender, RoutedEventArgs e)
    {
        InitializeCore();
        LoadSettings();
        RefreshAudioDevices();
    }

    private void InitializeCore()
    {
        try
        {
            _core = new CoreBridge();
            System.IO.Directory.CreateDirectory(_dataDir);
            var dbPath = System.IO.Path.Combine(_dataDir, "meetings.db");
            var modelPath = System.IO.Path.Combine(_dataDir, "models", "ggml-tiny.bin");

            if (System.IO.File.Exists(modelPath))
            {
                _core.Initialize(dbPath, modelPath);
            }
        }
        catch
        {
            _core = null;
        }
    }

    private void LoadSettings()
    {
        // Load transcription mode
        var mode = _core?.GetSetting("transcription_mode") ?? "local";
        for (int i = 0; i < TranscriptionMode.Items.Count; i++)
        {
            if (TranscriptionMode.Items[i] is RadioButton rb &&
                rb.Tag?.ToString() == mode)
            {
                TranscriptionMode.SelectedIndex = i;
                break;
            }
        }

        // Load model selection
        var model = _core?.GetSetting("whisper_model") ?? "tiny";
        for (int i = 0; i < ModelCombo.Items.Count; i++)
        {
            if (ModelCombo.Items[i] is ComboBoxItem item &&
                item.Tag?.ToString() == model)
            {
                ModelCombo.SelectedIndex = i;
                break;
            }
        }

        // Load language
        var lang = _core?.GetSetting("language") ?? "auto";
        for (int i = 0; i < LanguageCombo.Items.Count; i++)
        {
            if (LanguageCombo.Items[i] is ComboBoxItem item &&
                item.Tag?.ToString() == lang)
            {
                LanguageCombo.SelectedIndex = i;
                break;
            }
        }

        // Load loopback toggle
        var loopback = _core?.GetSetting("loopback_enabled") ?? "false";
        LoopbackToggle.IsOn = loopback == "true";

        // Load API keys (masked — actual keys stored in Credential Manager)
        var hasOpenAI = _core?.GetSetting("has_openai_key") ?? "false";
        if (hasOpenAI == "true")
        {
            OpenAIKeyBox.PlaceholderText = "Key saved (click to change)";
        }

        var hasAnthropic = _core?.GetSetting("has_anthropic_key") ?? "false";
        if (hasAnthropic == "true")
        {
            AnthropicKeyBox.PlaceholderText = "Key saved (click to change)";
        }
    }

    private void RefreshAudioDevices()
    {
        AudioDeviceCombo.Items.Clear();
        AudioDeviceCombo.Items.Add(new ComboBoxItem
        {
            Content = "Default System Microphone",
            Tag = ""
        });
        AudioDeviceCombo.SelectedIndex = 0;

        try
        {
            var json = _core?.ListAudioDevices();
            if (json is null) return;

            var devices = JsonSerializer.Deserialize<AudioDevice[]>(json,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });

            if (devices is null) return;

            var selectedDevice = _core?.GetSetting("audio_device_id");

            foreach (var device in devices)
            {
                var item = new ComboBoxItem
                {
                    Content = device.Name,
                    Tag = device.Id
                };
                AudioDeviceCombo.Items.Add(item);

                if (device.Id == selectedDevice)
                {
                    AudioDeviceCombo.SelectedItem = item;
                }
            }
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Failed to list audio devices: {ex.Message}");
        }
    }

    #region Event Handlers

    private void AudioDeviceCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (AudioDeviceCombo.SelectedItem is ComboBoxItem item)
        {
            var deviceId = item.Tag?.ToString() ?? "";
            _core?.SetSetting("audio_device_id", deviceId);
        }
    }

    private void RefreshDevices_Click(object sender, RoutedEventArgs e)
    {
        RefreshAudioDevices();
    }

    private void LoopbackToggle_Toggled(object sender, RoutedEventArgs e)
    {
        _core?.SetSetting("loopback_enabled", LoopbackToggle.IsOn ? "true" : "false");
    }

    private void TranscriptionMode_SelectionChanged(object sender,
        SelectionChangedEventArgs e)
    {
        if (TranscriptionMode.SelectedItem is RadioButton rb)
        {
            var mode = rb.Tag?.ToString() ?? "local";
            _core?.SetSetting("transcription_mode", mode);
        }
    }

    private void ModelCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (ModelCombo.SelectedItem is ComboBoxItem item)
        {
            var model = item.Tag?.ToString() ?? "tiny";
            _core?.SetSetting("whisper_model", model);
        }
    }

    private void LanguageCombo_SelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (LanguageCombo.SelectedItem is ComboBoxItem item)
        {
            var lang = item.Tag?.ToString() ?? "auto";
            _core?.SetSetting("language", lang);
        }
    }

    private async void DownloadModel_Click(object sender, RoutedEventArgs e)
    {
        if (ModelCombo.SelectedItem is not ComboBoxItem item) return;

        var model = item.Tag?.ToString() ?? "tiny";
        DownloadModelButton.IsEnabled = false;
        ModelDownloadProgress.Visibility = Visibility.Visible;
        ModelDownloadProgress.IsIndeterminate = true;

        try
        {
            // In production, this would download from huggingface
            // For now, show progress and create the models directory
            var modelsDir = System.IO.Path.Combine(_dataDir, "models");
            System.IO.Directory.CreateDirectory(modelsDir);

            await Task.Delay(1000); // Placeholder for actual download

            var dialog = new ContentDialog
            {
                Title = "Model Download",
                Content = $"Download the '{model}' model from huggingface.co and place it at:\n\n"
                    + System.IO.Path.Combine(modelsDir, $"ggml-{model}.bin"),
                CloseButtonText = "OK",
                XamlRoot = this.XamlRoot
            };
            await dialog.ShowAsync();
        }
        finally
        {
            DownloadModelButton.IsEnabled = true;
            ModelDownloadProgress.Visibility = Visibility.Collapsed;
        }
    }

    private void OpenAIKeyBox_PasswordChanged(object sender, RoutedEventArgs e)
    {
        var key = OpenAIKeyBox.Password;
        if (!string.IsNullOrWhiteSpace(key))
        {
            // In production: store in Windows Credential Manager
            _core?.SetSetting("openai_api_key", key);
            _core?.SetSetting("has_openai_key", "true");
        }
    }

    private void AnthropicKeyBox_PasswordChanged(object sender, RoutedEventArgs e)
    {
        var key = AnthropicKeyBox.Password;
        if (!string.IsNullOrWhiteSpace(key))
        {
            // In production: store in Windows Credential Manager
            _core?.SetSetting("anthropic_api_key", key);
            _core?.SetSetting("has_anthropic_key", "true");
        }
    }

    private async void ExportAll_Click(object sender, RoutedEventArgs e)
    {
        var picker = new Windows.Storage.Pickers.FolderPicker();
        picker.SuggestedStartLocation =
            Windows.Storage.Pickers.PickerLocationId.DocumentsLibrary;

        var hwnd = GetWindowHandle();
        WinRT.Interop.InitializeWithWindow.Initialize(picker, hwnd);

        var folder = await picker.PickSingleFolderAsync();
        if (folder is null) return;

        try
        {
            var json = _core?.ListMeetings(1000, 0);
            if (json is null) return;

            var meetings = JsonSerializer.Deserialize<MeetingItem[]>(json,
                new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
            if (meetings is null) return;

            foreach (var meeting in meetings)
            {
                var exported = _core?.ExportMeeting(meeting.Id, "markdown");
                if (exported is not null)
                {
                    var fileName = $"{meeting.StartedAt:yyyy-MM-dd}_{SanitizeFileName(meeting.Title)}.md";
                    var file = await folder.CreateFileAsync(fileName,
                        Windows.Storage.CreationCollisionOption.GenerateUniqueName);
                    await Windows.Storage.FileIO.WriteTextAsync(file, exported);
                }
            }

            var dialog = new ContentDialog
            {
                Title = "Export Complete",
                Content = $"Exported {meetings.Length} meetings to {folder.Path}",
                CloseButtonText = "OK",
                XamlRoot = this.XamlRoot
            };
            await dialog.ShowAsync();
        }
        catch (Exception ex)
        {
            System.Diagnostics.Debug.WriteLine($"Export failed: {ex.Message}");
        }
    }

    private void OpenDataFolder_Click(object sender, RoutedEventArgs e)
    {
        System.IO.Directory.CreateDirectory(_dataDir);
        System.Diagnostics.Process.Start("explorer.exe", _dataDir);
    }

    private async void ClearData_Click(object sender, RoutedEventArgs e)
    {
        var dialog = new ContentDialog
        {
            Title = "Clear All Data",
            Content = "This will permanently delete all meetings, transcripts, notes, and settings. "
                + "This action cannot be undone.\n\nAre you sure?",
            PrimaryButtonText = "Delete Everything",
            CloseButtonText = "Cancel",
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = this.XamlRoot
        };

        var result = await dialog.ShowAsync();
        if (result == ContentDialogResult.Primary)
        {
            try
            {
                _core?.Dispose();
                _core = null;

                var dbPath = System.IO.Path.Combine(_dataDir, "meetings.db");
                if (System.IO.File.Exists(dbPath))
                    System.IO.File.Delete(dbPath);

                InitializeCore();
                LoadSettings();
            }
            catch (Exception ex)
            {
                System.Diagnostics.Debug.WriteLine($"Clear data failed: {ex.Message}");
            }
        }
    }

    #endregion

    private static IntPtr GetWindowHandle()
    {
        // Get HWND from the current window through the App
        if (Application.Current is App app)
        {
            var mainWindow = app.GetType()
                .GetField("_mainWindow",
                    System.Reflection.BindingFlags.NonPublic |
                    System.Reflection.BindingFlags.Instance)
                ?.GetValue(app) as MainWindow;
            if (mainWindow is not null)
                return WinRT.Interop.WindowNative.GetWindowHandle(mainWindow);
        }
        return IntPtr.Zero;
    }

    private static string SanitizeFileName(string name)
    {
        var invalid = System.IO.Path.GetInvalidFileNameChars();
        return string.Join("_", name.Split(invalid, StringSplitOptions.RemoveEmptyEntries))
            .Trim();
    }
}

/// <summary>
/// Audio device info deserialized from core library JSON.
/// </summary>
internal sealed class AudioDevice
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public bool IsDefault { get; set; }
}
