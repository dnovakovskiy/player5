import Foundation

/// Where the tempo and phase come from (ADR-0001 §4).
public enum ClockKind: String, Codable, CaseIterable, Identifiable {
    /// The pattern's BPM.
    case internalClock = "internal"
    /// The internal clock, set by tapping.
    case tap
    /// CDJ/XDJ players and DJM mixers on the booth LAN.
    case proDJLink = "prodjlink"
    /// An all-in-one unit (rekordbox-lighting style source; coarse phase).
    case opusQuad = "opus"
    /// Ableton Link peers.
    case link
    /// MIDI clock in (24 ppqn) from any CoreMIDI source; macOS only.
    case midi
    /// A perfect built-in clock, for demos and tests without hardware.
    case simulated

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .internalClock: return "Internal"
        case .tap: return "Tap"
        case .proDJLink: return "Pro DJ Link"
        case .opusQuad: return "Opus Quad"
        case .link: return "Ableton Link"
        case .midi: return "MIDI clock"
        case .simulated: return "Simulated"
        }
    }

    /// The kinds this platform offers, in menu order.
    public static var available: [ClockKind] {
        #if os(macOS)
            return [.internalClock, .tap, .proDJLink, .opusQuad, .link, .midi, .simulated]
        #else
            return [.internalClock, .tap, .proDJLink, .opusQuad, .link, .simulated]
        #endif
    }

    /// `kind` argument of `p5_control_start_source`, for the sources that
    /// run inside the core.
    var sourceCode: Int32? {
        switch self {
        case .proDJLink: return 1
        case .opusQuad: return 2
        case .link: return 3
        case .simulated: return 4
        case .internalClock, .tap, .midi: return nil
        }
    }

    /// Network sources list devices and accept a follow target.
    public var listsDevices: Bool {
        self == .proDJLink || self == .opusQuad
    }

    /// Whether the engine follows an external timeline (vs. its own clock).
    public var isExternal: Bool {
        switch self {
        case .internalClock, .tap: return false
        case .proDJLink, .opusQuad, .link, .midi, .simulated: return true
        }
    }
}

/// Outcome of selecting a clock.
public enum ClockStartResult: Equatable {
    case ok
    /// The core was built without this source (`p5_control_start_source`
    /// returned 2).
    case notInThisBuild
    /// The source failed to start (returned 3); the core's status message.
    case failed(String)
}

/// A device a network source reports (`p5_control_devices_json`, same
/// fields as the bridge protocol's `devices` message).
public struct NetworkDevice: Decodable, Identifiable, Equatable {
    public let number: Int
    public let name: String
    public let address: String
    public let kind: String
    public let bpm: Double?
    public let playing: Bool?
    public let master: Bool?
    public let onAir: Bool?

    public var id: Int { number }

    enum CodingKeys: String, CodingKey {
        case number
        case name
        case address
        case kind
        case bpm
        case playing
        case master
        case onAir = "on_air"
    }

    /// Parses the core's device list; `[]` for anything unreadable.
    public static func list(fromJSON json: String) -> [NetworkDevice] {
        guard let data = json.data(using: .utf8) else { return [] }
        return (try? JSONDecoder().decode([NetworkDevice].self, from: data)) ?? []
    }
}
