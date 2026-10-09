#if os(macOS)
    import CoreAudio
    import Foundation

    /// An audio device with output channels.
    public struct OutputDevice: Identifiable, Hashable {
        public let id: AudioDeviceID
        /// Stable across reboots and reconnects; what we persist.
        public let uid: String
        public let name: String
        public let outputChannels: Int
    }

    /// Core Audio output-device enumeration (macOS).
    public enum OutputDevices {
        /// Every device that has at least one output channel.
        public static func all() -> [OutputDevice] {
            let system = AudioObjectID(kAudioObjectSystemObject)
            var address = globalAddress(kAudioHardwarePropertyDevices)
            var size: UInt32 = 0
            guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr, size > 0
            else {
                return []
            }
            let count = Int(size) / MemoryLayout<AudioDeviceID>.size
            var ids = [AudioDeviceID](repeating: 0, count: count)
            let status = ids.withUnsafeMutableBytes { raw -> OSStatus in
                guard let base = raw.baseAddress else { return -1 }
                return AudioObjectGetPropertyData(system, &address, 0, nil, &size, base)
            }
            guard status == noErr else { return [] }
            let valid = Int(size) / MemoryLayout<AudioDeviceID>.size
            return ids.prefix(valid).compactMap { id -> OutputDevice? in
                let channels = outputChannelCount(id)
                guard channels > 0 else { return nil }
                return OutputDevice(
                    id: id,
                    uid: stringProperty(id, kAudioDevicePropertyDeviceUID) ?? "device-\(id)",
                    name: stringProperty(id, kAudioObjectPropertyName) ?? "Audio device \(id)",
                    outputChannels: channels)
            }
        }

        /// The system's default output device.
        public static func defaultOutputID() -> AudioDeviceID? {
            let system = AudioObjectID(kAudioObjectSystemObject)
            var address = globalAddress(kAudioHardwarePropertyDefaultOutputDevice)
            var id = AudioDeviceID(0)
            var size = UInt32(MemoryLayout<AudioDeviceID>.size)
            let status = AudioObjectGetPropertyData(system, &address, 0, nil, &size, &id)
            guard status == noErr, id != 0 else { return nil }
            return id
        }

        /// The device with this UID, if it is connected.
        public static func device(uid: String) -> OutputDevice? {
            all().first { $0.uid == uid }
        }

        /// Calls `handler` on the main queue whenever devices come or go.
        /// The listener lives as long as the process.
        public static func observeChanges(_ handler: @escaping () -> Void) {
            var address = globalAddress(kAudioHardwarePropertyDevices)
            _ = AudioObjectAddPropertyListenerBlock(
                AudioObjectID(kAudioObjectSystemObject), &address, DispatchQueue.main
            ) { _, _ in
                handler()
            }
        }

        private static func globalAddress(_ selector: AudioObjectPropertySelector)
            -> AudioObjectPropertyAddress
        {
            AudioObjectPropertyAddress(
                mSelector: selector,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain)
        }

        private static func outputChannelCount(_ id: AudioDeviceID) -> Int {
            var address = AudioObjectPropertyAddress(
                mSelector: kAudioDevicePropertyStreamConfiguration,
                mScope: kAudioDevicePropertyScopeOutput,
                mElement: kAudioObjectPropertyElementMain)
            var size: UInt32 = 0
            guard AudioObjectGetPropertyDataSize(id, &address, 0, nil, &size) == noErr, size > 0
            else {
                return 0
            }
            let raw = UnsafeMutableRawPointer.allocate(
                byteCount: Int(size), alignment: MemoryLayout<AudioBufferList>.alignment)
            defer { raw.deallocate() }
            guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, raw) == noErr else {
                return 0
            }
            let list = UnsafeMutableAudioBufferListPointer(
                raw.assumingMemoryBound(to: AudioBufferList.self))
            var channels = 0
            for buffer in list {
                channels += Int(buffer.mNumberChannels)
            }
            return channels
        }

        private static func stringProperty(
            _ id: AudioObjectID, _ selector: AudioObjectPropertySelector
        ) -> String? {
            var address = globalAddress(selector)
            var value: Unmanaged<CFString>?
            var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
            let status = withUnsafeMutablePointer(to: &value) { pointer -> OSStatus in
                AudioObjectGetPropertyData(id, &address, 0, nil, &size, pointer)
            }
            guard status == noErr, let string = value else { return nil }
            return string.takeRetainedValue() as String
        }
    }
#endif
