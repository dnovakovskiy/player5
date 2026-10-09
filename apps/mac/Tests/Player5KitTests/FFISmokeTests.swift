import Darwin
import Foundation
import Player5Core
import XCTest

@testable import Player5Kit

#if os(macOS)
    import CoreMIDI
#endif

/// The Rust core through the C ABI exactly as the app uses it: a split
/// control/render pair, pattern JSON from the Swift model, rendering in
/// blocks. No audio hardware involved.
final class FFISmokeTests: XCTestCase {
    private var control: OpaquePointer!
    private var renderer: OpaquePointer!

    override func setUpWithError() throws {
        var c: OpaquePointer?
        var r: OpaquePointer?
        XCTAssertEqual(p5_split_new(48_000, &c, &r), 0)
        control = try XCTUnwrap(c)
        renderer = try XCTUnwrap(r)
    }

    override func tearDown() {
        if control != nil {
            p5_control_free(control)
        }
        if renderer != nil {
            p5_renderer_free(renderer)
        }
        control = nil
        renderer = nil
    }

    private func load(_ json: String) -> Int32 {
        json.withCString { p5_control_load_pattern_json(control, $0) }
    }

    /// Renders `blocks` × 512 frames, ticking the control half between
    /// blocks like the 5 ms timer does. Returns the peak magnitude.
    private func renderPeak(blocks: Int, hostTicks: UInt64 = 0) -> Float {
        var block = [Float](repeating: 0, count: 512)
        var peak: Float = 0
        for _ in 0..<blocks {
            block.withUnsafeMutableBufferPointer { buffer in
                p5_renderer_render(renderer, buffer.baseAddress, buffer.count, hostTicks)
            }
            XCTAssertTrue(block.allSatisfy { $0.isFinite })
            for sample in block {
                peak = max(peak, abs(sample))
            }
            p5_control_tick(control)
        }
        return peak
    }

    func testABIVersion() {
        XCTAssertGreaterThanOrEqual(p5_abi_version(), 3)
    }

    func testKickPatternRendersAudio() {
        let pattern = Pattern(bpm: 120, tracks: [.kick: VoiceTrack("x--- x--- x--- x---")])
        XCTAssertEqual(load(pattern.jsonString()), 0)
        p5_control_start(control)
        p5_control_tick(control)
        XCTAssertEqual(p5_control_is_playing(control), 1)
        let peak = renderPeak(blocks: 16)
        XCTAssertGreaterThan(peak, 0.01, "the kick on step 1 should sound")
        XCTAssertLessThan(peak, 2, "default output peaks near -6 dBFS")
        XCTAssertGreaterThanOrEqual(p5_control_playing_step(control), 0)
    }

    func testStoppedEngineIsSilent() {
        XCTAssertEqual(load(Presets.techno.pattern.jsonString()), 0)
        p5_control_tick(control)
        XCTAssertEqual(p5_control_is_playing(control), 0)
        XCTAssertLessThan(renderPeak(blocks: 8), 1e-6)
        XCTAssertEqual(p5_control_playing_step(control), -1)
    }

    /// Everything the Swift model can write, the core accepts.
    func testCoreAcceptsEverythingTheModelWrites() throws {
        var full = Pattern(bpm: 133.25, shuffle: 0.5, accent: 0.75, flam: 0.25, outputGain: 2, limiter: true)
        for voice in Voice.allCases {
            full[voice] = VoiceTrack("f--- F--- x--- X---", tune: 0.1, mute: voice == .clap)
        }
        XCTAssertEqual(load(full.jsonString()), 0)
        XCTAssertEqual(load(full.jsonString(pretty: true)), 0)
        XCTAssertEqual(load(Pattern().jsonString()), 0)
        for preset in Presets.all {
            XCTAssertEqual(load(preset.pattern.jsonString()), 0, preset.name)
        }
        // Repository pattern files, re-encoded by the model.
        for url in try PatternFiles.all() {
            let pattern = try Pattern.decode(jsonData: try Data(contentsOf: url))
            XCTAssertEqual(load(pattern.jsonString()), 0, url.lastPathComponent)
        }
        // And the core still rejects what it should.
        XCTAssertEqual(load(#"{ "voices": { "cymbal": { "steps": "----------------" } } }"#), 1)
        XCTAssertEqual(load(#"{ "voices": { "kick": { "steps": "x---" } } }"#), 1)
        XCTAssertEqual(p5_control_load_pattern_json(control, nil), 2)
    }

    func testClockSourcesAndCodes() throws {
        // The simulator is always built in.
        XCTAssertEqual(p5_control_start_source(control, 4, 0, 128), 0)
        p5_control_stop_source(control)
        // Network sources: 0 once merged and started, 2 while not in this
        // build, 3 if the machine has no usable network.
        for kind in [Int32(1), 2, 3] {
            let rc = p5_control_start_source(control, kind, 0, 0)
            XCTAssertTrue([Int32(0), 2, 3].contains(rc), "kind \(kind) returned \(rc)")
            p5_control_stop_source(control)
        }
        XCTAssertEqual(p5_control_start_source(control, 9, 0, 0), 1)
        XCTAssertEqual(p5_control_set_clock_mode(control, 0), 0)
        XCTAssertEqual(p5_control_set_clock_mode(control, 4), 0)
        XCTAssertEqual(p5_control_set_clock_mode(control, 42), 1)

        // No tick has drained a source yet, so the list is still empty.
        let devices = try XCTUnwrap(p5_control_devices_json(control))
        XCTAssertEqual(NetworkDevice.list(fromJSON: String(cString: devices)), [])
        XCTAssertNotNil(p5_control_status(control))
    }

    /// Following the simulated source with real host times locks to it,
    /// as the app does when "Simulated" is selected.
    func testFollowsTheSimulatedSource() {
        XCTAssertEqual(p5_control_start_source(control, 4, 0, 133), 0)
        var block = [Float](repeating: 0, count: 480)
        for _ in 0..<60 {
            let now = mach_absolute_time()
            block.withUnsafeMutableBufferPointer { buffer in
                p5_renderer_render(renderer, buffer.baseAddress, buffer.count, now)
            }
            p5_control_tick(control)
            usleep(10_000)
        }
        XCTAssertEqual(p5_control_clock_locked(control), 1)
        XCTAssertEqual(p5_control_tempo(control), 133, accuracy: 0.5)
        p5_control_stop_source(control)
    }

    func testMIDIAndTapCalls() {
        XCTAssertEqual(p5_control_set_clock_mode(control, 4), 0)
        // No host time published yet: refused, not crashed.
        XCTAssertEqual(p5_control_midi_host(control, 0, p5_host_time_ns()), 1)
        var block = [Float](repeating: 0, count: 256)
        block.withUnsafeMutableBufferPointer { buffer in
            p5_renderer_render(renderer, buffer.baseAddress, buffer.count, mach_absolute_time())
        }
        XCTAssertEqual(p5_control_midi_host(control, 1, p5_host_time_ns()), 0)
        XCTAssertEqual(p5_control_midi_host(control, 0, p5_host_time_ns()), 0)
        XCTAssertEqual(p5_control_midi_host(control, 7, p5_host_time_ns()), 1)
        XCTAssertEqual(p5_control_set_clock_mode(control, 0), 0)
        p5_control_tap_host(control, 0)
        p5_control_tap_host(control, p5_host_time_ns())
        p5_control_set_nudge_ms(control, 2.5)
        p5_control_set_latency_ms(control, 10)
        p5_control_resync(control)
        p5_control_follow(control, 0)
        p5_control_tick(control)
    }

    /// Swift's tick conversion and the core's host clock agree (both are
    /// `mach_absolute_time` scaled by `mach_timebase_info`).
    func testHostClockMatchesTheCore() {
        let swiftNs = HostClock.nanoseconds(fromTicks: mach_absolute_time())
        let coreNs = p5_host_time_ns()
        let difference = Int64(bitPattern: coreNs &- swiftNs)
        XCTAssertLessThan(abs(difference), 5_000_000, "host clocks differ by \(difference) ns")
    }

    #if os(macOS)
        /// The UMP walker finds clock messages and skips multi-word messages
        /// whose payload happens to look like one.
        func testMIDIScanFindsClockMessages() {
            let size = 512
            let raw = UnsafeMutableRawPointer.allocate(byteCount: size, alignment: 8)
            defer { raw.deallocate() }
            raw.initializeMemory(as: UInt8.self, repeating: 0, count: size)
            // MIDIEventList: protocol, numPackets, then packed packets of
            // { timeStamp: UInt64, wordCount: UInt32, words: [UInt32] }.
            raw.storeBytes(of: Int32(1), toByteOffset: 0, as: Int32.self)
            raw.storeBytes(of: UInt32(2), toByteOffset: 4, as: UInt32.self)
            var offset = 8
            func packet(_ time: UInt64, _ words: [UInt32]) {
                raw.storeBytes(of: time, toByteOffset: offset, as: UInt64.self)
                raw.storeBytes(of: UInt32(words.count), toByteOffset: offset + 8, as: UInt32.self)
                for (i, word) in words.enumerated() {
                    raw.storeBytes(of: word, toByteOffset: offset + 12 + 4 * i, as: UInt32.self)
                }
                offset += 12 + 4 * words.count
            }
            packet(1_000, [0x10F8_0000, 0x4090_3C00, 0x10F8_0000, 0x2090_3C64, 0x10FC_0000])
            packet(2_000, [0x10FA_0000])

            var seen: [(Int32, UInt64)] = []
            let list = UnsafePointer(raw.assumingMemoryBound(to: MIDIEventList.self))
            MIDIClockInput.scan(list) { code, time in
                seen.append((code, time))
            }
            XCTAssertEqual(seen.map { $0.0 }, [0, 3, 1])
            XCTAssertEqual(seen.last?.1, HostClock.nanoseconds(fromTicks: 2_000))
            XCTAssertEqual(MIDIClockInput.code(forStatus: 0xF8), 0)
            XCTAssertEqual(MIDIClockInput.code(forStatus: 0xFB), 2)
            XCTAssertNil(MIDIClockInput.code(forStatus: 0x90))
        }

        /// A packet longer than the 64 words `MIDIEventPacket` declares is
        /// read whole, and the next packet is found after all its words
        /// (`MIDIEventPacketNext` uses the full `wordCount`).
        func testMIDIScanHandlesPacketsLongerThan64Words() {
            let size = 1_024
            let raw = UnsafeMutableRawPointer.allocate(byteCount: size, alignment: 8)
            defer { raw.deallocate() }
            raw.initializeMemory(as: UInt8.self, repeating: 0, count: size)
            raw.storeBytes(of: Int32(1), toByteOffset: 0, as: Int32.self)
            raw.storeBytes(of: UInt32(2), toByteOffset: 4, as: UInt32.self)
            var offset = 8
            func packet(_ time: UInt64, _ words: [UInt32]) {
                raw.storeBytes(of: time, toByteOffset: offset, as: UInt64.self)
                raw.storeBytes(of: UInt32(words.count), toByteOffset: offset + 8, as: UInt32.self)
                for (i, word) in words.enumerated() {
                    raw.storeBytes(of: word, toByteOffset: offset + 12 + 4 * i, as: UInt32.self)
                }
                offset += 12 + 4 * words.count
            }
            // 32 two-word SysEx messages (64 words), then Clock and Stop:
            // 66 words in one packet.
            var long: [UInt32] = []
            for _ in 0..<32 {
                long.append(contentsOf: [0x3016_0000, 0])
            }
            long.append(contentsOf: [0x10F8_0000, 0x10FC_0000])
            packet(1_000, long)
            packet(2_000, [0x10FA_0000])

            var seen: [(Int32, UInt64)] = []
            let list = UnsafePointer(raw.assumingMemoryBound(to: MIDIEventList.self))
            MIDIClockInput.scan(list) { code, time in
                seen.append((code, time))
            }
            XCTAssertEqual(seen.map { $0.0 }, [0, 3, 1])
            XCTAssertEqual(seen.first?.1, HostClock.nanoseconds(fromTicks: 1_000))
            XCTAssertEqual(seen.last?.1, HostClock.nanoseconds(fromTicks: 2_000))
        }
    #endif
}
