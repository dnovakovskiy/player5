import Foundation

/// One voice's steps and TR-style controls (`VoiceSpec` in
/// `core/engine/src/spec.rs`). Defaults match the core's.
public struct VoiceTrack: Equatable {
    /// Exactly 16 steps.
    public var steps: [Step]
    public var tune: Double
    public var decay: Double
    public var tone: Double
    public var snappy: Double
    public var level: Double
    /// Muted tracks keep their steps but play nothing.
    public var mute: Bool

    public init(
        steps: [Step] = Array(repeating: .off, count: Step.count),
        tune: Double = 0.5,
        decay: Double = 0.5,
        tone: Double = 0.5,
        snappy: Double = 0.5,
        level: Double = 1.0,
        mute: Bool = false
    ) {
        self.steps = Step.parse(Step.notation(steps))
        self.tune = tune
        self.decay = decay
        self.tone = tone
        self.snappy = snappy
        self.level = level
        self.mute = mute
    }

    /// A track from step notation (`"X--- x--- X--- x---"`).
    public init(
        _ notation: String,
        tune: Double = 0.5,
        decay: Double = 0.5,
        tone: Double = 0.5,
        snappy: Double = 0.5,
        level: Double = 1.0,
        mute: Bool = false
    ) {
        self.init(
            steps: Step.parse(notation), tune: tune, decay: decay, tone: tone,
            snappy: snappy, level: level, mute: mute)
    }

    /// Step notation without grouping spaces.
    public var notation: String { Step.notation(steps) }

    /// Whether any step is on.
    public var hasHits: Bool { steps.contains { $0.isOn } }

    /// Brings every value into the range the core expects.
    public mutating func normalize() {
        steps = Step.parse(Step.notation(steps))
        tune = Pattern.clamp(tune, 0...1, fallback: 0.5)
        decay = Pattern.clamp(decay, 0...1, fallback: 0.5)
        tone = Pattern.clamp(tone, 0...1, fallback: 0.5)
        snappy = Pattern.clamp(snappy, 0...1, fallback: 0.5)
        level = Pattern.clamp(level, 0...1, fallback: 1.0)
    }
}

/// A complete pattern: the live subset of `PatternSpec` in
/// `core/engine/src/spec.rs` (tempo, feel, ten voices, master). The offline
/// render settings (`bars`, `sample_rate`, …) are not part of a live pattern;
/// the core applies its defaults.
public struct Pattern: Equatable {
    /// Tempo for the internal clock.
    public var bpm: Double
    /// Shuffle `0...1`.
    public var shuffle: Double
    /// Accent amount `0...1`.
    public var accent: Double
    /// Flam spacing `0...1`.
    public var flam: Double
    /// One track per voice, in `Voice.allCases` order.
    public internal(set) var tracks: [VoiceTrack]
    /// Master output gain (linear; 1 = unity, the ≈ −6 dBFS default peaks).
    public var outputGain: Double
    /// Soft safety limiter.
    public var limiter: Bool

    /// Tempo range the UI and decoder accept (the web app uses the same).
    public static let bpmRange: ClosedRange<Double> = 20...400
    /// Output gain range (0 to +12 dB).
    public static let outputGainRange: ClosedRange<Double> = 0...4

    public init(
        bpm: Double = 120,
        shuffle: Double = 0,
        accent: Double = 0.5,
        flam: Double = 0.5,
        tracks: [Voice: VoiceTrack] = [:],
        outputGain: Double = 1,
        limiter: Bool = false
    ) {
        self.bpm = bpm
        self.shuffle = shuffle
        self.accent = accent
        self.flam = flam
        self.tracks = Voice.allCases.map { tracks[$0] ?? VoiceTrack() }
        self.outputGain = outputGain
        self.limiter = limiter
    }

    /// The track of one voice.
    public subscript(voice: Voice) -> VoiceTrack {
        get { tracks[voice.index] }
        set { tracks[voice.index] = newValue }
    }

    /// Brings every value into range (non-finite values fall back to the
    /// defaults).
    public mutating func normalize() {
        bpm = Pattern.clamp(bpm, Pattern.bpmRange, fallback: 120)
        shuffle = Pattern.clamp(shuffle, 0...1, fallback: 0)
        accent = Pattern.clamp(accent, 0...1, fallback: 0.5)
        flam = Pattern.clamp(flam, 0...1, fallback: 0.5)
        outputGain = Pattern.clamp(outputGain, Pattern.outputGainRange, fallback: 1)
        if tracks.count != Voice.allCases.count {
            let old = tracks
            tracks = Voice.allCases.map { $0.index < old.count ? old[$0.index] : VoiceTrack() }
        }
        for i in tracks.indices {
            tracks[i].normalize()
        }
    }

    /// Normalized copy.
    public func normalized() -> Pattern {
        var p = self
        p.normalize()
        return p
    }

    /// Clamps `value` into `range`; NaN and infinities become `fallback`.
    public static func clamp(_ value: Double, _ range: ClosedRange<Double>, fallback: Double) -> Double {
        guard value.isFinite else { return fallback }
        return Swift.min(Swift.max(value, range.lowerBound), range.upperBound)
    }

    /// Every step of every voice off; controls untouched.
    public mutating func clearSteps() {
        for i in tracks.indices {
            tracks[i].steps = Array(repeating: .off, count: Step.count)
        }
    }
}
