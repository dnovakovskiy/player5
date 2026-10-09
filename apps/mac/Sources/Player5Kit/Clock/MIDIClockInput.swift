#if os(macOS)
    import CoreMIDI
    import Foundation

    /// MIDI clock in from every CoreMIDI source (macOS). Forwards Timing
    /// Clock, Start, Continue and Stop with their packet timestamps; the core
    /// does the tempo/phase work (`sync::midi`, `docs/protocols/midi-clock.md`).
    ///
    /// Uses the Universal MIDI Packet input API (macOS 11+). System
    /// Real-Time messages arrive as one-word UMP messages of type 0x1 with
    /// the MIDI 1.0 status byte in bits 16–23.
    final class MIDIClockInput {
        /// `(code, hostNanoseconds)` with the codes of `p5_control_midi_host`.
        /// Called on CoreMIDI's receive thread.
        typealias Handler = (Int32, UInt64) -> Void

        private var client = MIDIClientRef()
        private var port = MIDIPortRef()
        private var connected: [MIDIEndpointRef] = []
        /// Display names of the connected sources. Main thread.
        private(set) var sourceNames: [String] = []
        /// Called on the main thread when the source list changes.
        var onSourcesChanged: (([String]) -> Void)?

        /// `nil` if CoreMIDI refused the client or port.
        init?(handler: @escaping Handler) {
            var newClient = MIDIClientRef()
            let clientStatus = MIDIClientCreateWithBlock("player5" as CFString, &newClient) {
                [weak self] notification in
                if notification.pointee.messageID == .msgSetupChanged {
                    DispatchQueue.main.async {
                        self?.connectAll()
                    }
                }
            }
            guard clientStatus == noErr else { return nil }
            client = newClient

            var newPort = MIDIPortRef()
            let portStatus = MIDIInputPortCreateWithProtocol(
                newClient, "player5 clock in" as CFString, ._1_0, &newPort
            ) { eventList, _ in
                MIDIClockInput.scan(eventList, handler)
            }
            guard portStatus == noErr else {
                MIDIClientDispose(newClient)
                return nil
            }
            port = newPort
            connectAll()
        }

        deinit {
            MIDIPortDispose(port)
            MIDIClientDispose(client)
        }

        /// (Re)connects the port to every source. Main thread.
        func connectAll() {
            for source in connected {
                MIDIPortDisconnectSource(port, source)
            }
            connected.removeAll()
            var names: [String] = []
            for index in 0..<MIDIGetNumberOfSources() {
                let source = MIDIGetSource(index)
                guard source != 0, MIDIPortConnectSource(port, source, nil) == noErr else {
                    continue
                }
                connected.append(source)
                names.append(MIDIClockInput.displayName(of: source))
            }
            sourceNames = names
            onSourcesChanged?(names)
        }

        private static func displayName(of endpoint: MIDIEndpointRef) -> String {
            var name: Unmanaged<CFString>?
            guard MIDIObjectGetStringProperty(endpoint, kMIDIPropertyDisplayName, &name) == noErr,
                let value = name
            else {
                return "MIDI source \(endpoint)"
            }
            return value.takeRetainedValue() as String
        }

        /// `p5_control_midi_host` code for a MIDI 1.0 status byte.
        static func code(forStatus status: UInt8) -> Int32? {
            switch status {
            case 0xF8: return 0  // Timing Clock
            case 0xFA: return 1  // Start
            case 0xFB: return 2  // Continue
            case 0xFC: return 3  // Stop
            default: return nil
            }
        }

        /// Size in 32-bit words of a UMP message, by message type (the top
        /// four bits of its first word), per the MIDI 2.0 UMP specification.
        static func wordCount(ofMessageType type: UInt32) -> Int {
            switch type {
            case 0x0, 0x1, 0x2, 0x6, 0x7: return 1
            case 0x3, 0x4, 0x8, 0x9, 0xA: return 2
            case 0xB, 0xC: return 3
            default: return 4
            }
        }

        /// Walks an event list and reports the clock messages. Reads the
        /// packed structs through raw pointers (packets are variable-length,
        /// 4-byte aligned) instead of copying them.
        static func scan(_ list: UnsafePointer<MIDIEventList>, _ handler: Handler) {
            let packetCount = Int(list.pointee.numPackets)
            let firstPacketOffset = MemoryLayout<MIDIEventList>.offset(of: \MIDIEventList.packet) ?? 8
            let wordsOffset = MemoryLayout<MIDIEventPacket>.offset(of: \MIDIEventPacket.words) ?? 12
            let countOffset = MemoryLayout<MIDIEventPacket>.offset(of: \MIDIEventPacket.wordCount) ?? 8
            var packet = UnsafeRawPointer(list) + firstPacketOffset
            for _ in 0..<packetCount {
                let timeStamp = packet.loadUnaligned(as: UInt64.self)
                let words = min(Int(packet.loadUnaligned(fromByteOffset: countOffset, as: UInt32.self)), 64)
                let wordBase = packet + wordsOffset
                let hostNanoseconds =
                    timeStamp == 0
                    ? HostClock.nowNanoseconds() : HostClock.nanoseconds(fromTicks: timeStamp)
                var i = 0
                while i < words {
                    let word = wordBase.loadUnaligned(fromByteOffset: i * 4, as: UInt32.self)
                    let messageType = word >> 28
                    if messageType == 0x1,
                        let message = MIDIClockInput.code(forStatus: UInt8((word >> 16) & 0xFF))
                    {
                        handler(message, hostNanoseconds)
                    }
                    i += MIDIClockInput.wordCount(ofMessageType: messageType)
                }
                packet = wordBase + words * 4
            }
        }
    }
#endif
