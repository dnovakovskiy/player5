import Foundation
import XCTest

@testable import Player5Kit

#if os(macOS)
    import CoreAudio
#endif

/// The pure parts of the engine host, clock selection, settings and output
/// routing. (The audio graph itself needs hardware; see ADR-0008.)
final class EngineHostTests: XCTestCase {
    func testBarAndBeatFromTheCoreBeat() {
        func check(_ beat: Double, _ bar: Int, _ beatInBar: Int, line: UInt = #line) {
            let position = EngineSnapshot.position(atBeat: beat)
            XCTAssertEqual(position?.bar, bar, "bar at beat \(beat)", line: line)
            XCTAssertEqual(position?.beatInBar, beatInBar, "beat in bar at beat \(beat)", line: line)
        }
        check(0, 1, 1)
        check(0.99, 1, 1)
        check(1, 1, 2)
        check(3.999, 1, 4)
        check(4, 2, 1)
        check(13.5, 4, 2)
        // Before the timeline's zero: the bar before bar 1.
        check(-0.5, 0, 4)
        check(-4, 0, 1)
        XCTAssertNil(EngineSnapshot.position(atBeat: .nan))
        XCTAssertNil(EngineSnapshot.position(atBeat: .infinity))
        XCTAssertNil(EngineSnapshot.position(atBeat: 1e15))
    }

    /// `p5_control_start_source` kinds (core/ffi/src/split.rs).
    func testClockKindsMapOntoTheCoreSourceCodes() {
        XCTAssertEqual(ClockKind.proDJLink.sourceCode, 1)
        XCTAssertEqual(ClockKind.opusQuad.sourceCode, 2)
        XCTAssertEqual(ClockKind.link.sourceCode, 3)
        XCTAssertEqual(ClockKind.simulated.sourceCode, 4)
        for kind in [ClockKind.internalClock, .tap, .midi] {
            XCTAssertNil(kind.sourceCode, kind.title)
        }
        XCTAssertFalse(ClockKind.internalClock.isExternal)
        XCTAssertFalse(ClockKind.tap.isExternal)
        XCTAssertTrue(ClockKind.midi.isExternal)
        XCTAssertTrue(ClockKind.simulated.isExternal)
        XCTAssertEqual(Set(ClockKind.available).count, ClockKind.available.count)
        XCTAssertEqual(ClockKind.available.first, ClockKind.internalClock)
        #if os(macOS)
            XCTAssertTrue(ClockKind.available.contains(.midi))
        #else
            XCTAssertFalse(ClockKind.available.contains(.midi))
        #endif
    }

    /// Saved clock settings from another build (unknown source) fall back to
    /// the defaults instead of failing the launch.
    func testUnreadableClockSettingsFallBackToDefaults() throws {
        let suite = "player5.tests.\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set(Data(#"{"kind":"turntable","followDevice":3}"#.utf8), forKey: "player5.clock.v1")
        XCTAssertEqual(SettingsStore(defaults: defaults).loadClock(), ClockSettings())
        defaults.set("not data", forKey: "player5.clock.v1")
        XCTAssertEqual(SettingsStore(defaults: defaults).loadClock(), ClockSettings())
    }

    #if os(macOS)
        func testOutputDeviceResolution() {
            let a = OutputDevice(id: 10, uid: "uid-a", name: "A", outputChannels: 2)
            let b = OutputDevice(id: 20, uid: "uid-b", name: "B", outputChannels: 4)

            // "System default": nothing is pinned, the default plays.
            var target = OutputDevices.resolve(uid: nil, among: [a, b], defaultID: 10)
            XCTAssertNil(target.chosen)
            XCTAssertEqual(target.effective, 10)
            // A new system default changes the effective device, which is
            // what makes the app move the engine onto it.
            target = OutputDevices.resolve(uid: nil, among: [a, b], defaultID: 20)
            XCTAssertNil(target.chosen)
            XCTAssertEqual(target.effective, 20)

            // A chosen device wins while it is connected, whatever the default.
            target = OutputDevices.resolve(uid: "uid-b", among: [a, b], defaultID: 10)
            XCTAssertEqual(target.chosen, 20)
            XCTAssertEqual(target.effective, 20)
            // Unplugged: the default plays until it returns.
            target = OutputDevices.resolve(uid: "uid-b", among: [a], defaultID: 10)
            XCTAssertNil(target.chosen)
            XCTAssertEqual(target.effective, 10)

            // No output at all.
            target = OutputDevices.resolve(uid: nil, among: [], defaultID: nil)
            XCTAssertNil(target.chosen)
            XCTAssertNil(target.effective)
        }

        /// Runs the real Core Audio queries (pointer handling, property
        /// sizes) on whatever the machine has; CI runners may have no
        /// output devices at all.
        func testOutputDeviceEnumerationRuns() {
            let devices = OutputDevices.all()
            XCTAssertEqual(Set(devices.map { $0.id }).count, devices.count)
            for device in devices {
                XCTAssertGreaterThan(device.outputChannels, 0, device.name)
                XCTAssertFalse(device.uid.isEmpty, device.name)
                XCTAssertEqual(OutputDevices.device(uid: device.uid)?.id, device.id, device.name)
            }
            if let id = OutputDevices.defaultOutputID() {
                XCTAssertNotEqual(id, AudioDeviceID(0))
            }
        }
    #endif
}
