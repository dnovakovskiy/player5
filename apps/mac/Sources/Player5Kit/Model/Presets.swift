import Foundation

/// A named starting point.
public struct Preset: Identifiable, Equatable {
    public let name: String
    public let pattern: Pattern
    public var id: String { name }
}

/// Built-in presets. Genre names only; every voice is our own synthesis.
public enum Presets {
    /// The pattern a fresh install starts with (the web app's default).
    public static var initial: Pattern { fourOnTheFloor.pattern }

    public static let all: [Preset] = [
        fourOnTheFloor, house, techno, electro, breaks, flams, empty,
    ]

    public static let fourOnTheFloor = Preset(
        name: "Four on the floor",
        pattern: Pattern(
            bpm: 124, accent: 1,
            tracks: [.kick: VoiceTrack("X--- x--- X--- x---")]))

    public static let house = Preset(
        name: "House",
        pattern: Pattern(
            bpm: 124, shuffle: 0.1, accent: 0.8,
            tracks: [
                .kick: VoiceTrack("X--- x--- X--- x---"),
                .clap: VoiceTrack("---- X--- ---- X---"),
                .closedHat: VoiceTrack("x--x x--x x--x x--x", level: 0.7),
                .openHat: VoiceTrack("--x- --x- --x- --x-", level: 0.8),
            ]))

    public static let techno = Preset(
        name: "Techno",
        pattern: Pattern(
            bpm: 132, accent: 0.9,
            tracks: [
                .kick: VoiceTrack("X--- X--- X--- X---", tune: 0.4, decay: 0.6),
                .rim: VoiceTrack("---x --x- ---x -x--", level: 0.7),
                .clap: VoiceTrack("---- x--- ---- X---", level: 0.8),
                .closedHat: VoiceTrack("xX-x xX-x xX-x xX-x", decay: 0.3, level: 0.6),
                .openHat: VoiceTrack("--x- --x- --x- --x-", level: 0.7),
            ]))

    public static let electro = Preset(
        name: "Electro",
        pattern: Pattern(
            bpm: 118, accent: 0.8,
            tracks: [
                .kick: VoiceTrack("X--- --X- --X- ----", decay: 0.7),
                .snare: VoiceTrack("---- X--- ---- X---", snappy: 0.7),
                .closedHat: VoiceTrack("x-x- x-x- x-x- x-xX", level: 0.7),
                .cowbell: VoiceTrack("---- ---- --x- ---x", level: 0.6),
                .lowTom: VoiceTrack("---- ---- ---- -x--", level: 0.8),
            ]))

    public static let breaks = Preset(
        name: "Breaks",
        pattern: Pattern(
            bpm: 134, shuffle: 0.25, accent: 0.7,
            tracks: [
                .kick: VoiceTrack("X-x- ---- --X- ----"),
                .snare: VoiceTrack("---- X--x -x-- X---", snappy: 0.6),
                .closedHat: VoiceTrack("x-x- x-xx x-x- x---", level: 0.7),
                .openHat: VoiceTrack("---- ---- ---- --x-", level: 0.7),
            ]))

    public static let flams = Preset(
        name: "Flam fills",
        pattern: Pattern(
            bpm: 120, accent: 0.8, flam: 0.4,
            tracks: [
                .kick: VoiceTrack("X--- x--- X--- x---"),
                .snare: VoiceTrack("---- F--- ---- f-fF"),
                .highTom: VoiceTrack("---- ---- --f- ----"),
                .midTom: VoiceTrack("---- ---- ---f ----"),
                .closedHat: VoiceTrack("x-x- x-x- x-x- x---", level: 0.7),
            ]))

    public static let empty = Preset(
        name: "Empty",
        pattern: Pattern(bpm: 124, accent: 0.8))
}
