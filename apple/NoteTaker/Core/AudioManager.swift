import AVFoundation
import CoreAudio
import Foundation

/// Manages macOS audio permissions and device monitoring.
///
/// Wraps the Rust core's audio functions and handles macOS-specific concerns:
/// microphone permission requests, audio device enumeration via CoreAudio,
/// and live device-change notifications. When a CoreBridge is provided, it
/// tries the core's `listAudioDevices()` first and falls back to CoreAudio enumeration.
@MainActor @Observable
final class AudioManager {

    // MARK: - Published state

    var permissionGranted: Bool = false
    var permissionDetermined: Bool = false
    var availableDevices: [AudioDevice] = []
    var selectedDeviceId: String?

    /// Current audio input level (0.0 ... 1.0) for UI metering.
    var audioLevel: Float = 0.0

    // MARK: - Types

    struct AudioDevice: Identifiable, Hashable {
        let id: String
        let name: String
    }

    // MARK: - Private

    private let core: CoreBridge?
    private var deviceChangeListenerInstalled = false
    private var deviceChangePropertyAddress = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )

    // MARK: - Lifecycle

    init(core: CoreBridge? = nil) {
        self.core = core
        checkPermission()
        refreshDevices()
        installDeviceChangeListener()
    }

    // MARK: - Permission

    func checkPermission() {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            permissionGranted = true
            permissionDetermined = true
        case .notDetermined:
            permissionGranted = false
            permissionDetermined = false
        case .denied, .restricted:
            permissionGranted = false
            permissionDetermined = true
        @unknown default:
            permissionGranted = false
            permissionDetermined = true
        }
    }

    func requestPermission() async {
        let granted = await AVCaptureDevice.requestAccess(for: .audio)
        permissionGranted = granted
        permissionDetermined = true
    }

    // MARK: - Device enumeration

    func refreshDevices() {
        // Try the Rust core first; it may return a device list in its own ID format.
        if let core, let json = core.listAudioDevices() {
            let parsed = parseCoreDevices(json: json)
            if !parsed.isEmpty {
                availableDevices = parsed
                if selectedDeviceId == nil, let first = parsed.first {
                    selectedDeviceId = first.id
                }
                return
            }
        }
        // Fallback: enumerate via CoreAudio directly.
        refreshDevicesViaCoreAudio()
    }

    /// Parse the JSON returned by `note_list_audio_devices` as `[{"id": "...", "name": "..."}]`.
    private func parseCoreDevices(json: String) -> [AudioDevice] {
        guard let data = json.data(using: .utf8),
              let array = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return [] }
        return array.compactMap { dict in
            guard let id = dict["id"] as? String,
                  let name = dict["name"] as? String
            else { return nil }
            return AudioDevice(id: id, name: name)
        }
    }

    private func refreshDevicesViaCoreAudio() {
        var propertyAddress = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var dataSize: UInt32 = 0
        var status = AudioObjectGetPropertyDataSize(
            AudioObjectID(kAudioObjectSystemObject),
            &propertyAddress,
            0,
            nil,
            &dataSize
        )
        guard status == noErr else { return }

        let deviceCount = Int(dataSize) / MemoryLayout<AudioDeviceID>.size
        var deviceIDs = [AudioDeviceID](repeating: 0, count: deviceCount)

        status = AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &propertyAddress,
            0,
            nil,
            &dataSize,
            &deviceIDs
        )
        guard status == noErr else { return }

        var devices: [AudioDevice] = []
        for deviceID in deviceIDs {
            // Check if this device has input channels.
            var inputAddress = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyStreamConfiguration,
                mScope: kAudioDevicePropertyScopeInput,
                mElement: kAudioObjectPropertyElementMain
            )
            var bufferListSize: UInt32 = 0
            let sizeStatus = AudioObjectGetPropertyDataSize(
                deviceID, &inputAddress, 0, nil, &bufferListSize
            )
            guard sizeStatus == noErr, bufferListSize > 0 else { continue }

            let bufferListPointer = UnsafeMutablePointer<AudioBufferList>.allocate(
                capacity: Int(bufferListSize) / MemoryLayout<AudioBufferList>.stride + 1
            )
            defer { bufferListPointer.deallocate() }

            let dataStatus = AudioObjectGetPropertyData(
                deviceID, &inputAddress, 0, nil, &bufferListSize, bufferListPointer
            )
            guard dataStatus == noErr else { continue }

            let bufferList = UnsafeMutableAudioBufferListPointer(bufferListPointer)
            let inputChannels = bufferList.reduce(0) { $0 + Int($1.mNumberChannels) }
            guard inputChannels > 0 else { continue }

            // Get device name.
            var nameAddress = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyDeviceNameCFString,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            var nameRef: Unmanaged<CFString>?
            var nameRefSize = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            let nameStatus = AudioObjectGetPropertyData(
                deviceID, &nameAddress, 0, nil, &nameRefSize, &nameRef
            )
            let deviceName: String
            if nameStatus == noErr, let cfStr = nameRef?.takeRetainedValue() {
                deviceName = cfStr as String
            } else {
                deviceName = "Unknown Device"
            }

            devices.append(AudioDevice(id: String(deviceID), name: deviceName))
        }

        availableDevices = devices
        if selectedDeviceId == nil, let first = devices.first {
            selectedDeviceId = first.id
        }
    }

    // MARK: - Device change monitoring

    private func installDeviceChangeListener() {
        guard !deviceChangeListenerInstalled else { return }
        let status = AudioObjectAddPropertyListenerBlock(
            AudioObjectID(kAudioObjectSystemObject),
            &deviceChangePropertyAddress,
            DispatchQueue.main
        ) { [weak self] _, _ in
            Task { @MainActor in
                self?.refreshDevices()
            }
        }
        if status == noErr {
            deviceChangeListenerInstalled = true
        }
    }
}
