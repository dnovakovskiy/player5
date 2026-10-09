import Foundation

/// One step of one track, in the notation of `core/engine/src/spec.rs`:
/// `-` off, `x` hit, `X` accented hit, `f` flammed hit, `F` accented flam.
public enum Step: Character, CaseIterable {
    case off = "-"
    case hit = "x"
    case accent = "X"
    case flam = "f"
    case accentFlam = "F"

    /// Steps per pattern (`sequencer::STEP_COUNT`).
    public static let count = 16

    /// Parses one notation character. `.` is an alternative spelling of off.
    public init?(symbol: Character) {
        switch symbol {
        case "-", ".": self = .off
        case "x": self = .hit
        case "X": self = .accent
        case "f": self = .flam
        case "F": self = .accentFlam
        default: return nil
        }
    }

    public var isOn: Bool { self != .off }
    public var isAccent: Bool { self == .accent || self == .accentFlam }
    public var isFlam: Bool { self == .flam || self == .accentFlam }

    /// A normal tap: off → hit → accent → off. A flammed step keeps its flam
    /// while it moves to accent.
    public var cycled: Step {
        switch self {
        case .off: return .hit
        case .hit: return .accent
        case .accent: return .off
        case .flam: return .accentFlam
        case .accentFlam: return .off
        }
    }

    /// A tap in flam mode: toggles the flam, turning an empty step into a
    /// flammed hit.
    public var flamToggled: Step {
        switch self {
        case .off: return .flam
        case .hit: return .flam
        case .accent: return .accentFlam
        case .flam: return .hit
        case .accentFlam: return .accent
        }
    }

    /// Parses a notation string leniently: whitespace is ignored, unknown
    /// characters count as off, and the result is padded or cut to 16 steps.
    /// (The core is strict; this is for imported links and old data.)
    public static func parse(_ notation: String) -> [Step] {
        var steps: [Step] = []
        steps.reserveCapacity(count)
        for ch in notation where !ch.isWhitespace {
            if steps.count == count { break }
            steps.append(Step(symbol: ch) ?? .off)
        }
        while steps.count < count {
            steps.append(.off)
        }
        return steps
    }

    /// The notation for a step list, without grouping spaces (the form the
    /// web app writes into its links).
    public static func notation(_ steps: [Step]) -> String {
        String(steps.map { $0.rawValue })
    }
}
