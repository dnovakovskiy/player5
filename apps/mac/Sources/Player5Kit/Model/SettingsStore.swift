import Foundation

/// A pattern the user saved under a name. Stored as pattern-file JSON so
/// saved data is always a valid `patterns/*.json` file.
public struct SavedPattern: Codable, Identifiable, Equatable {
    public var id: UUID
    public var name: String
    public var json: String

    public init(id: UUID = UUID(), name: String, pattern: Pattern) {
        self.id = id
        self.name = name
        self.json = pattern.jsonString()
    }

    /// The pattern, or `nil` if the stored JSON no longer parses.
    public var pattern: Pattern? { try? Pattern.decode(json: json) }
}

/// Clock settings that survive relaunches.
public struct ClockSettings: Codable, Equatable {
    public var kind: ClockKind = .internalClock
    /// 0 = tempo master, else a Pro DJ Link device number.
    public var followDevice: Int = 0
    /// Phase nudge, ms (positive = later).
    public var nudgeMs: Double = 0
    /// User latency offset, ms (positive = trigger earlier).
    public var latencyMs: Double = 0
    /// Add the output device's reported latency to the offset.
    public var compensateDeviceLatency: Bool = true

    public init() {}
}

/// UserDefaults persistence for the current pattern, the user's library,
/// clock settings and a few preferences.
public final class SettingsStore {
    private let defaults: UserDefaults

    private enum Key {
        static let pattern = "player5.pattern.v1"
        static let library = "player5.library.v1"
        static let clock = "player5.clock.v1"
        static let webAppURL = "player5.webAppURL"
        static let outputDeviceUID = "player5.outputDeviceUID"
    }

    public init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    public func loadPattern() -> Pattern? {
        guard let json = defaults.string(forKey: Key.pattern) else { return nil }
        return try? Pattern.decode(json: json)
    }

    public func savePattern(_ pattern: Pattern) {
        defaults.set(pattern.jsonString(), forKey: Key.pattern)
    }

    public func loadLibrary() -> [SavedPattern] {
        guard let data = defaults.data(forKey: Key.library) else { return [] }
        return (try? JSONDecoder().decode([SavedPattern].self, from: data)) ?? []
    }

    public func saveLibrary(_ library: [SavedPattern]) {
        if let data = try? JSONEncoder().encode(library) {
            defaults.set(data, forKey: Key.library)
        }
    }

    public func loadClock() -> ClockSettings {
        guard let data = defaults.data(forKey: Key.clock),
            let settings = try? JSONDecoder().decode(ClockSettings.self, from: data)
        else {
            return ClockSettings()
        }
        return settings
    }

    public func saveClock(_ settings: ClockSettings) {
        if let data = try? JSONEncoder().encode(settings) {
            defaults.set(data, forKey: Key.clock)
        }
    }

    public var webAppURL: String {
        get { defaults.string(forKey: Key.webAppURL) ?? "" }
        set { defaults.set(newValue, forKey: Key.webAppURL) }
    }

    /// Core Audio device UID of the chosen output (macOS); `nil` = system
    /// default.
    public var outputDeviceUID: String? {
        get { defaults.string(forKey: Key.outputDeviceUID) }
        set {
            if let uid = newValue {
                defaults.set(uid, forKey: Key.outputDeviceUID)
            } else {
                defaults.removeObject(forKey: Key.outputDeviceUID)
            }
        }
    }
}
