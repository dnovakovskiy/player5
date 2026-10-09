import Foundation

// JSON coding for the pattern format of `core/engine/src/spec.rs`.
//
// Decoding is lenient (absent fields take the core's defaults, unknown keys
// such as the offline `render.bars` are ignored, values are clamped) so
// pattern files, web-app links and older saved data all load. Encoding
// writes only keys the core accepts — the core rejects unknown fields
// (`deny_unknown_fields`) — so the output always loads in
// `p5_control_load_pattern_json`.

extension VoiceTrack: Codable {
    enum CodingKeys: String, CodingKey {
        case steps
        case tune
        case decay
        case tone
        case snappy
        case level
        case mute
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let notation = try c.decodeIfPresent(String.self, forKey: .steps) ?? ""
        let tune = try c.decodeIfPresent(Double.self, forKey: .tune) ?? 0.5
        let decay = try c.decodeIfPresent(Double.self, forKey: .decay) ?? 0.5
        let tone = try c.decodeIfPresent(Double.self, forKey: .tone) ?? 0.5
        let snappy = try c.decodeIfPresent(Double.self, forKey: .snappy) ?? 0.5
        let level = try c.decodeIfPresent(Double.self, forKey: .level) ?? 1.0
        let mute = try c.decodeIfPresent(Bool.self, forKey: .mute) ?? false
        self.init(
            notation, tune: tune, decay: decay, tone: tone, snappy: snappy,
            level: level, mute: mute)
        normalize()
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(notation, forKey: .steps)
        try c.encode(tune, forKey: .tune)
        try c.encode(decay, forKey: .decay)
        try c.encode(tone, forKey: .tone)
        try c.encode(snappy, forKey: .snappy)
        try c.encode(level, forKey: .level)
        // The core omits `mute` when false; so do we.
        if mute {
            try c.encode(true, forKey: .mute)
        }
    }
}

extension Pattern: Codable {
    enum CodingKeys: String, CodingKey {
        case bpm
        case shuffle
        case accent
        case flam
        case voices
        case render
    }

    enum RenderKeys: String, CodingKey {
        case outputGain = "output_gain"
        case limiter
    }

    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        let bpm = try c.decodeIfPresent(Double.self, forKey: .bpm) ?? 120
        let shuffle = try c.decodeIfPresent(Double.self, forKey: .shuffle) ?? 0
        let accent = try c.decodeIfPresent(Double.self, forKey: .accent) ?? 0.5
        let flam = try c.decodeIfPresent(Double.self, forKey: .flam) ?? 0.5

        var tracks: [Voice: VoiceTrack] = [:]
        if c.contains(.voices), try !c.decodeNil(forKey: .voices) {
            let voices = try c.nestedContainer(keyedBy: Voice.self, forKey: .voices)
            for voice in Voice.allCases {
                if let track = try voices.decodeIfPresent(VoiceTrack.self, forKey: voice) {
                    tracks[voice] = track
                }
            }
        }

        var outputGain = 1.0
        var limiter = false
        if c.contains(.render), try !c.decodeNil(forKey: .render) {
            let render = try c.nestedContainer(keyedBy: RenderKeys.self, forKey: .render)
            outputGain = try render.decodeIfPresent(Double.self, forKey: .outputGain) ?? 1.0
            limiter = try render.decodeIfPresent(Bool.self, forKey: .limiter) ?? false
        }

        self.init(
            bpm: bpm, shuffle: shuffle, accent: accent, flam: flam, tracks: tracks,
            outputGain: outputGain, limiter: limiter)
        normalize()
    }

    public func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        try c.encode(bpm, forKey: .bpm)
        try c.encode(shuffle, forKey: .shuffle)
        try c.encode(accent, forKey: .accent)
        try c.encode(flam, forKey: .flam)
        // Absent voices are silent with default controls, so a voice equal
        // to that is left out: shorter links, same pattern.
        var voices = c.nestedContainer(keyedBy: Voice.self, forKey: .voices)
        let silent = VoiceTrack()
        for voice in Voice.allCases where self[voice] != silent {
            try voices.encode(self[voice], forKey: voice)
        }
        var render = c.nestedContainer(keyedBy: RenderKeys.self, forKey: .render)
        try render.encode(outputGain, forKey: .outputGain)
        try render.encode(limiter, forKey: .limiter)
    }
}

extension Pattern {
    /// Compact (or pretty) JSON with sorted keys: the pattern file format.
    public func jsonData(pretty: Bool = false) throws -> Data {
        let encoder = JSONEncoder()
        encoder.outputFormatting = pretty ? [.sortedKeys, .prettyPrinted] : [.sortedKeys]
        return try encoder.encode(normalized())
    }

    /// JSON text for the core (`p5_control_load_pattern_json`), for files
    /// and for links. Never fails: a pattern that cannot be encoded (which
    /// normalization rules out) becomes `{}`, the core's silent default.
    public func jsonString(pretty: Bool = false) -> String {
        guard let data = try? jsonData(pretty: pretty),
            let text = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }
        return text
    }

    /// Parses pattern JSON (a pattern file, a web-app link payload, saved
    /// data).
    public static func decode(json: String) throws -> Pattern {
        try decode(jsonData: Data(json.utf8))
    }

    /// Parses pattern JSON bytes.
    public static func decode(jsonData: Data) throws -> Pattern {
        try JSONDecoder().decode(Pattern.self, from: jsonData)
    }
}
