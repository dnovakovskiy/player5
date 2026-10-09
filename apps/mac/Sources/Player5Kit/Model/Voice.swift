import Foundation

/// The ten voices, in the core's order (`sequencer::VoiceId::ALL`). Raw
/// values are the JSON keys of `core/engine/src/spec.rs`.
///
/// Conforming to `CodingKey` lets the pattern coder use the voices directly
/// as the keys of the `"voices"` object.
public enum Voice: String, CaseIterable, Identifiable, CodingKey {
    case kick
    case snare
    case lowTom = "low_tom"
    case midTom = "mid_tom"
    case highTom = "high_tom"
    case rim
    case clap
    case closedHat = "closed_hat"
    case openHat = "open_hat"
    case cowbell

    public var id: String { rawValue }

    /// Position in the core's voice order (0..<10).
    public var index: Int {
        switch self {
        case .kick: return 0
        case .snare: return 1
        case .lowTom: return 2
        case .midTom: return 3
        case .highTom: return 4
        case .rim: return 5
        case .clap: return 6
        case .closedHat: return 7
        case .openHat: return 8
        case .cowbell: return 9
        }
    }

    /// Two-letter panel label, as in `sequencer::VoiceId::label`.
    public var label: String {
        switch self {
        case .kick: return "BD"
        case .snare: return "SD"
        case .lowTom: return "LT"
        case .midTom: return "MT"
        case .highTom: return "HT"
        case .rim: return "RS"
        case .clap: return "CP"
        case .closedHat: return "CH"
        case .openHat: return "OH"
        case .cowbell: return "CB"
        }
    }

    /// Human-readable name.
    public var title: String {
        switch self {
        case .kick: return "Kick"
        case .snare: return "Snare"
        case .lowTom: return "Low tom"
        case .midTom: return "Mid tom"
        case .highTom: return "High tom"
        case .rim: return "Rimshot"
        case .clap: return "Clap"
        case .closedHat: return "Closed hat"
        case .openHat: return "Open hat"
        case .cowbell: return "Cowbell"
        }
    }

    /// The controls this voice responds to (from each voice's module docs in
    /// `core/dsp`). Every control is stored and sent for every voice; the
    /// UI only shows the ones that do something.
    public var controls: [VoiceControl] {
        switch self {
        case .kick: return [.tune, .decay, .level]
        case .snare: return [.tune, .tone, .snappy, .decay, .level]
        case .lowTom, .midTom, .highTom: return [.tune, .decay, .level]
        case .rim: return [.tune, .decay, .level]
        case .clap: return [.tone, .decay, .level]
        case .closedHat, .openHat: return [.tone, .decay, .level]
        case .cowbell: return [.tune, .tone, .decay, .level]
        }
    }

    /// The voice at a core index.
    public static func at(index: Int) -> Voice? {
        allCases.first { $0.index == index }
    }
}

/// One of the per-voice `0...1` controls of the pattern format.
public enum VoiceControl: String, CaseIterable, Identifiable {
    case tune
    case decay
    case tone
    case snappy
    case level

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .tune: return "Tune"
        case .decay: return "Decay"
        case .tone: return "Tone"
        case .snappy: return "Snappy"
        case .level: return "Level"
        }
    }

    /// Key path into a track.
    public var keyPath: WritableKeyPath<VoiceTrack, Double> {
        switch self {
        case .tune: return \.tune
        case .decay: return \.decay
        case .tone: return \.tone
        case .snappy: return \.snappy
        case .level: return \.level
        }
    }
}
