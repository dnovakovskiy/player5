import Foundation
import XCTest

@testable import Player5Kit

/// The Swift pattern model against the format of `core/engine/src/spec.rs`.
final class PatternModelTests: XCTestCase {
    /// The example from the module docs of `core/engine/src/spec.rs`.
    private let specExample = """
        {
          "bpm": 120,
          "shuffle": 0.0,
          "accent": 0.5,
          "flam": 0.5,
          "voices": {
            "kick":       { "steps": "X--- x--- X--- x---", "tune": 0.5, "decay": 0.5, "level": 1.0 },
            "snare":      { "steps": "---- X--- ---- X---", "snappy": 0.6 },
            "closed_hat": { "steps": "x-x- x-x- x-x- x-x-", "mute": false }
          },
          "render": { "bars": 2, "sample_rate": 48000, "tail_seconds": 0.5 }
        }
        """

    func testSpecExampleDecodes() throws {
        let p = try Pattern.decode(json: specExample)
        XCTAssertEqual(p.bpm, 120)
        XCTAssertEqual(p.shuffle, 0)
        XCTAssertEqual(p.accent, 0.5)
        XCTAssertEqual(p.flam, 0.5)
        XCTAssertEqual(p[.kick].notation, "X---x---X---x---")
        XCTAssertEqual(p[.kick].level, 1)
        XCTAssertEqual(p[.snare].notation, "----X-------X---")
        XCTAssertEqual(p[.snare].snappy, 0.6)
        XCTAssertEqual(p[.snare].tune, 0.5)
        XCTAssertEqual(p[.closedHat].mute, false)
        XCTAssertEqual(p[.cowbell], VoiceTrack())
        XCTAssertEqual(p.outputGain, 1)
        XCTAssertEqual(p.limiter, false)
    }

    func testDefaultsMatchTheCore() throws {
        let p = try Pattern.decode(json: #"{ "voices": { "kick": { "steps": "x---x---x---x---" } } }"#)
        XCTAssertEqual(p.bpm, 120)
        XCTAssertEqual(p.shuffle, 0)
        XCTAssertEqual(p.accent, 0.5)
        XCTAssertEqual(p.flam, 0.5)
        let kick = p[.kick]
        XCTAssertEqual(kick.tune, 0.5)
        XCTAssertEqual(kick.decay, 0.5)
        XCTAssertEqual(kick.tone, 0.5)
        XCTAssertEqual(kick.snappy, 0.5)
        XCTAssertEqual(kick.level, 1)
        XCTAssertFalse(kick.mute)
        XCTAssertEqual(try Pattern.decode(json: "{}"), Pattern())
    }

    func testRoundTripsEveryVoiceWithFlamsAndMute() throws {
        // Binary fractions, so equality does not depend on decimal printing.
        var p = Pattern(bpm: 127.5, shuffle: 0.25, accent: 0.75, flam: 0.375, outputGain: 1.5, limiter: true)
        for voice in Voice.allCases {
            p[voice] = VoiceTrack(
                "f--- F--- x--- X-.-",
                tune: Double(voice.index) / 16,
                decay: 0.25,
                tone: 0.75,
                snappy: 0.125,
                level: 0.875,
                mute: voice == .clap)
        }
        let json = p.jsonString()
        let again = try Pattern.decode(json: json)
        XCTAssertEqual(again, p)
        XCTAssertEqual(again[.clap].mute, true)
        XCTAssertEqual(again[.cowbell].steps[0], .flam)
        XCTAssertEqual(again[.cowbell].steps[4], .accentFlam)
        XCTAssertEqual(again[.cowbell].steps[14], .off)
        // Pretty output is the same pattern.
        XCTAssertEqual(try Pattern.decode(json: p.jsonString(pretty: true)), p)
    }

    /// The core rejects unknown fields, so everything we write must be a
    /// key it knows.
    func testEncodesOnlyKeysTheCoreAccepts() throws {
        var p = Presets.flams.pattern
        p[.clap] = VoiceTrack("x---", mute: true)
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: try p.jsonData()) as? [String: Any])
        let topKeys: Set<String> = ["bpm", "shuffle", "accent", "flam", "voices", "render"]
        XCTAssertTrue(Set(object.keys).isSubset(of: topKeys), "\(object.keys)")

        let voices = try XCTUnwrap(object["voices"] as? [String: Any])
        let voiceNames = Set(Voice.allCases.map { $0.rawValue })
        XCTAssertEqual(
            voiceNames,
            ["kick", "snare", "low_tom", "mid_tom", "high_tom", "rim", "clap", "closed_hat", "open_hat", "cowbell"])
        XCTAssertTrue(Set(voices.keys).isSubset(of: voiceNames), "\(voices.keys)")
        let voiceKeys: Set<String> = ["steps", "tune", "decay", "tone", "snappy", "level", "mute"]
        for (name, value) in voices {
            let fields = try XCTUnwrap(value as? [String: Any], name)
            XCTAssertTrue(Set(fields.keys).isSubset(of: voiceKeys), "\(name): \(fields.keys)")
            let steps = try XCTUnwrap(fields["steps"] as? String, name)
            XCTAssertEqual(steps.count, 16, name)
            XCTAssertTrue(steps.allSatisfy { "-xXfF".contains($0) }, steps)
        }

        let render = try XCTUnwrap(object["render"] as? [String: Any])
        let renderKeys: Set<String> = [
            "bars", "sample_rate", "tail_seconds", "output_gain", "limiter", "block_size",
        ]
        XCTAssertTrue(Set(render.keys).isSubset(of: renderKeys), "\(render.keys)")
    }

    func testSilentVoicesAndFalseMuteAreOmitted() throws {
        let object = try XCTUnwrap(
            JSONSerialization.jsonObject(with: try Presets.fourOnTheFloor.pattern.jsonData())
                as? [String: Any])
        let voices = try XCTUnwrap(object["voices"] as? [String: Any])
        XCTAssertEqual(Array(voices.keys), ["kick"])
        let kick = try XCTUnwrap(voices["kick"] as? [String: Any])
        XCTAssertNil(kick["mute"])
    }

    func testLenientDecodingNormalizes() throws {
        let json = """
            { "bpm": 1000, "shuffle": -1, "accent": 2,
              "voices": { "rim": { "steps": "x.X f q", "level": 7 }, "cymbal": { "steps": "x" } },
              "render": { "output_gain": 9, "limiter": true, "bars": 4 } }
            """
        let p = try Pattern.decode(json: json)
        XCTAssertEqual(p.bpm, 400)
        XCTAssertEqual(p.shuffle, 0)
        XCTAssertEqual(p.accent, 1)
        XCTAssertEqual(p[.rim].notation, "x-Xf------------")
        XCTAssertEqual(p[.rim].level, 1)
        XCTAssertEqual(p.outputGain, 4)
        XCTAssertTrue(p.limiter)
    }

    func testStepTaps() {
        XCTAssertEqual(Step.off.cycled, .hit)
        XCTAssertEqual(Step.hit.cycled, .accent)
        XCTAssertEqual(Step.accent.cycled, .off)
        XCTAssertEqual(Step.flam.cycled, .accentFlam)
        XCTAssertEqual(Step.accentFlam.cycled, .off)
        XCTAssertEqual(Step.off.flamToggled, .flam)
        XCTAssertEqual(Step.hit.flamToggled, .flam)
        XCTAssertEqual(Step.flam.flamToggled, .hit)
        XCTAssertEqual(Step.accent.flamToggled, .accentFlam)
        XCTAssertEqual(Step.accentFlam.flamToggled, .accent)
        for step in Step.allCases {
            XCTAssertEqual(Step(symbol: step.rawValue), step)
        }
        XCTAssertEqual(Step(symbol: "."), .off)
        XCTAssertNil(Step(symbol: "q"))
    }

    func testVoiceOrderMatchesTheCore() {
        XCTAssertEqual(Voice.allCases.map { $0.index }, Array(0..<10))
        XCTAssertEqual(
            Voice.allCases.map { $0.label }, ["BD", "SD", "LT", "MT", "HT", "RS", "CP", "CH", "OH", "CB"])
        for voice in Voice.allCases {
            XCTAssertEqual(Voice.at(index: voice.index), voice)
            XCTAssertTrue(voice.controls.contains(.level))
        }
    }

    /// Every example in `patterns/` (the golden-test fixtures) loads.
    func testRepositoryPatternFilesDecode() throws {
        let files = try PatternFiles.all()
        XCTAssertFalse(files.isEmpty)
        for url in files {
            let pattern = try Pattern.decode(jsonData: try Data(contentsOf: url))
            XCTAssertTrue(Pattern.bpmRange.contains(pattern.bpm), url.lastPathComponent)
        }
        let four = try Pattern.decode(
            jsonData: try Data(contentsOf: try PatternFiles.url(named: "four-on-the-floor.json")))
        XCTAssertEqual(four.bpm, 124)
        XCTAssertEqual(four[.kick].notation, "X---x---X---x---")
    }

    func testPresetsAreDistinctAndNamed() {
        XCTAssertEqual(Set(Presets.all.map { $0.name }).count, Presets.all.count)
        XCTAssertEqual(Presets.initial, Presets.fourOnTheFloor.pattern)
    }

    func testSettingsStoreRoundTrip() throws {
        let suite = "player5.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = SettingsStore(defaults: defaults)
        XCTAssertNil(store.loadPattern())
        store.savePattern(Presets.house.pattern)
        XCTAssertEqual(store.loadPattern(), Presets.house.pattern)

        var clock = ClockSettings()
        clock.kind = .simulated
        clock.nudgeMs = -3.5
        store.saveClock(clock)
        XCTAssertEqual(store.loadClock(), clock)

        let saved = SavedPattern(name: "mine", pattern: Presets.breaks.pattern)
        store.saveLibrary([saved])
        XCTAssertEqual(store.loadLibrary(), [saved])
        XCTAssertEqual(store.loadLibrary().first?.pattern, Presets.breaks.pattern)
    }

    func testDeviceListDecodes() {
        let json = """
            [{"number":2,"name":"CDJ-3000","address":"169.254.12.34","kind":"player",
              "bpm":124.0,"playing":true,"master":true,"on_air":null}]
            """
        let devices = NetworkDevice.list(fromJSON: json)
        XCTAssertEqual(devices.count, 1)
        XCTAssertEqual(devices.first?.number, 2)
        XCTAssertEqual(devices.first?.bpm, 124)
        XCTAssertEqual(devices.first?.master, true)
        XCTAssertNil(devices.first?.onAir)
        XCTAssertEqual(NetworkDevice.list(fromJSON: "[]"), [])
        XCTAssertEqual(NetworkDevice.list(fromJSON: "garbage"), [])
    }
}

/// The repository's `patterns/` directory, found from this source file.
enum PatternFiles {
    static func directory() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()  // Player5KitTests
            .deletingLastPathComponent()  // Tests
            .deletingLastPathComponent()  // apps/mac
            .deletingLastPathComponent()  // apps
            .deletingLastPathComponent()  // repository root
            .appendingPathComponent("patterns", isDirectory: true)
    }

    static func all() throws -> [URL] {
        try FileManager.default
            .contentsOfDirectory(at: directory(), includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "json" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
    }

    static func url(named name: String) throws -> URL {
        let url = directory().appendingPathComponent(name)
        guard FileManager.default.fileExists(atPath: url.path) else {
            throw CocoaError(.fileNoSuchFile)
        }
        return url
    }
}
