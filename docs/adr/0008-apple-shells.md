# ADR-0008: Apple shells: one XCFramework, one Swift package, XcodeGen apps

Status: Accepted (Apple shells session). Implements ADR-0001 §2 for macOS
and iOS.

## Context

ADR-0001 fixed the shape: a static library with a C ABI consumed by Swift,
macOS and iOS sharing one SwiftUI package, audio through AVAudioEngine.
The core now offers the split API (`core/ffi/src/split.rs`, ABI 3):
`P5Renderer` for the audio callback, `P5Control` for a control thread,
clock sources started inside the core.

Constraints:

- The core must be built for five Apple targets and reach Swift with a
  header and a module map, for SwiftPM (`swift build`, `swift test`) and for
  Xcode app bundles (Info.plist, entitlements, signing).
- CI runs on GitHub's macOS runners; the development containers have no
  Swift toolchain. Nothing generated (libraries, project files) should be
  committed.
- Real-time rules (CLAUDE.md) apply to the render callback; `P5Control` is
  not thread-safe.

## Decision

1. **Packaging.** `scripts/build-xcframework.sh` builds `core/ffi` with
   `cargo rustc --crate-type staticlib` for `aarch64-apple-darwin`,
   `x86_64-apple-darwin`, `aarch64-apple-ios`, `aarch64-apple-ios-sim` and
   `x86_64-apple-ios` (with `MACOSX_DEPLOYMENT_TARGET=13.0`,
   `IPHONEOS_DEPLOYMENT_TARGET=16.0`), lipos the macOS and simulator pairs,
   generates `player5.h` with cbindgen plus a `module Player5Core` module
   map, and runs `xcodebuild -create-xcframework` into
   `apps/mac/Frameworks/Player5Core.xcframework` (gitignored).

2. **One Swift package** at `apps/mac`: binary target `Player5Core`,
   library `Player5Kit` (engine wrapper, pattern model, clock, audio I/O,
   SwiftUI views), executable `Player5Mac`, tests `Player5KitTests`.
   Platforms macOS 13, iOS 16; Swift tools 5.9; no third-party packages.

3. **App bundles via XcodeGen**, generated in CI and locally, never
   committed.
   - `apps/ios/project.yml`: the iOS app consumes the package (`../mac`,
     product `Player5Kit`) and adds only the `@main` entry, Info.plist
     (background audio, local-network usage string) and signing settings.
   - `apps/mac/project.yml`: one sandboxed app target that compiles the
     package's `Sources/Player5Kit` and `Sources/Player5Mac` into a single
     module and links the XCFramework directly. A project sitting next to
     `Package.swift` would have to reference its own directory as a local
     package, and a separate framework module would make the app target
     resolve `Player5Core`'s module map transitively; one module avoids both.
     The entry file imports `Player5Kit` only `#if canImport(Player5Kit)`.
   - The iOS multicast entitlement file exists with the key commented out
     and is not referenced until Apple grants it
     (`docs/ios-multicast-entitlement.md`).

4. **Threads.**
   - *Audio:* an `AVAudioSourceNode` render block calls
     `p5_renderer_render(renderer, buffer, frames, mHostTime)` and copies the
     mono buffer into the other channel(s) with `memcpy`. It captures only
     the renderer's raw pointer: no allocation, locks, Objective-C messaging,
     logging or class references. The node feeds the main mixer with a
     stereo (or mono) format at the hardware rate, so the mixer passes it
     through at unity.
   - *Control:* one serial `DispatchQueue` (user-interactive QoS) owns
     `P5Control`, from creation to `p5_control_free`. A strict
     `DispatchSourceTimer`, created with the first control handle (nothing
     to tick before audio starts), fires every 5 ms on it and calls
     `p5_control_tick`. Every other `p5_control_*` call is posted to the
     same queue. Every sixth tick (~30 ms) it reads tempo, beat, lock,
     playhead, status and devices, and posts a snapshot to the main thread
     only when it changed.
   - *Main:* the `AVAudioEngine` graph, renderer lifetime and UI state.
     The pair is rebuilt when the hardware sample rate changes; a record of
     the UI's requested state (pattern, transport, clock, nudge, latency) is
     replayed onto the new `P5Control`. An old renderer is freed only after
     the engine stopped and its node was detached (plus a one-second grace).
   - While audio runs, a `ProcessInfo` activity (`.userInitiated`,
     `.latencyCritical`) keeps App Nap and timer coalescing off the control
     timer.

5. **Host-time mapping.** The renderer publishes each block's position and
   `AudioTimeStamp.mHostTime` (only when `hostTimeValid`). The core converts
   ticks to nanoseconds with `mach_timebase_info` (`sync::host_time`) and
   maps network observations onto samples. CoreMIDI timestamps are host
   ticks too; Swift converts them with the same timebase (`HostClock`), and
   a zero timestamp means "now". Taps are stamped with `p5_host_time_ns()` in
   the button action on the main thread, before any queue hop.
   `mHostTime` is when the buffer starts at the device's I/O; the sound
   leaves the converter `AVAudioIONode.presentationLatency` later. The
   latency offset sent to the core is the user's offset plus, by default,
   that reported latency, re-read whenever audio (re)starts and, on iOS,
   after every route change: a new route (Bluetooth, AirPlay, USB) can keep
   the hardware format, so no configuration change arrives, yet change the
   latency by over 100 ms.

6. **Clock sources.** Internal and Tap run the internal clock (a tap's tempo
   becomes the pattern BPM). Pro DJ Link, Opus Quad, Ableton Link and
   Simulated go through `p5_control_start_source` (kinds 1–4); a return of
   2 ("not in this build") or 3 (failed) falls back to the internal clock
   and the UI says why. MIDI clock (macOS) listens to every CoreMIDI source
   through the Universal MIDI Packet input API, forwards Timing Clock,
   Start, Continue and Stop to `p5_control_midi_host`, and puts the core in
   jittery-follow mode. Devices come from `p5_control_devices_json`; the
   follow picker calls `p5_control_follow`.

7. **macOS output device.** Core Audio enumerates devices with output
   channels; the choice is stored by UID and applied with
   `kAudioOutputUnitProperty_CurrentDevice` on `outputNode.audioUnit` before
   the engine starts. That pins the output unit to the device it started
   on, so with "System default" selected the app also listens for
   `kAudioHardwarePropertyDefaultOutputDevice` and restarts on the new
   default. An unplugged choice falls back to the system default and
   returns when the device does. `AVAudioEngineConfigurationChange` (and,
   on iOS, interruptions and media-services resets) rebuild the graph.

8. **Patterns.** A Codable model mirrors `core/engine/src/spec.rs`. The
   encoder writes only keys the core accepts (it denies unknown fields) and
   omits silent default voices; the decoder is lenient and clamps. The
   current pattern, a named library and the clock settings persist in
   UserDefaults. Links use the web app's `#p=<base64url(JSON)>` format, so a
   pattern moves between browser and native apps unchanged.

## Consequences

- `.github/workflows/apple.yml` builds the XCFramework, runs `swift build`
  and `swift test`, and builds both XcodeGen apps unsigned on
  `macos-latest`. Swift is only compiled there; the tests cover the pattern
  model against the spec, the link codec against a web-app payload, the C
  ABI end to end (split pair, every voice and flam, presets and
  `patterns/` files load, kick renders, simulated source locks), the
  MIDI packet walker, the clock-kind codes, bar/beat display and output
  device resolution, plus a live Core Audio device enumeration.
- No new FFI exports were needed; `core/ffi/src/split.rs` is unchanged.
- `core/ffi/cbindgen.toml` sets `[export] prefix = "P5"` (types become
  `P5P5Control`) and `[fn] prefix = "p5_"` (a bare `p5_` token before every
  declaration, which is not valid C). The build script generates the
  header from that file minus its `prefix` keys; `scripts/gen-header.sh`
  still produces the broken header until those two keys are removed.
- Shells must never free a renderer while a render callback can still run;
  the wrapper enforces this by ordering (stop, detach, defer free).

## To verify on hardware

- Output timing: `mHostTime` plus `presentationLatency` against a measured
  loopback or scope (built-in output, class-compliant USB interfaces, a DJM
  over USB), and the effect of the latency toggle when following a deck.
- Control-timer jitter under CPU load, with the window hidden (App Nap),
  and on iOS with the screen locked in background audio.
- Configuration changes mid-play: unplugging an interface, changing its
  sample rate in Audio MIDI Setup, iOS route changes and interruptions.
- Interfaces with more than two outputs: the mix should land on 1–2.
- CoreMIDI clock from a DJM and from software: timestamps (some drivers
  send zero), Start/Continue/Stop handling, jitter.
- Pro DJ Link inside the App Sandbox (binding its UDP ports with
  `network.server`) and the local-network privacy prompt on macOS 15+ and
  iOS; iOS network clocks once the multicast entitlement is granted.

## Sources

- AVAudioSourceNode and AVAudioEngine: Apple sample "Building a Signal
  Generator",
  https://developer.apple.com/documentation/avfaudio/audio_engine/building_a_signal_generator
- `AVAudioIONode.presentationLatency`:
  https://developer.apple.com/documentation/avfaudio/avaudioionode/presentationlatency
- Host time and `mach_timebase_info`: Apple TN2169,
  https://developer.apple.com/library/archive/technotes/tn2169/_index.html
- XCFrameworks: "Creating a multiplatform binary framework bundle",
  https://developer.apple.com/documentation/xcode/creating-a-multi-platform-binary-framework-bundle
- SwiftPM binary targets: SE-0272,
  https://github.com/apple/swift-evolution/blob/main/proposals/0272-swiftpm-binary-dependencies.md
- XcodeGen project spec: https://github.com/yonaskolb/XcodeGen/blob/master/Docs/ProjectSpec.md
- Universal MIDI Packet message sizes: MIDI Association, "Universal MIDI
  Packet (UMP) Format and MIDI 2.0 Protocol" (M2-104-UM),
  https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification
- iOS multicast entitlement:
  https://developer.apple.com/documentation/bundleresources/entitlements/com_apple_developer_networking_multicast

## Clarification (appended 2026-10-09, final review)

The M2-104-UM link under Sources was not reachable from the build
container (midi.org answers 403 there) and was not read. The UMP facts
`MIDIClockInput` relies on (type nibble, one-word type-`0x1` system
messages with the status byte in bits 16–23, sizes per type, walking a
`MIDIEventList` by each packet's full `wordCount`) are digested in
`docs/protocols/midi-clock.md` from Apple's CoreMIDI reference and headers
and an independent UMP library, and are marked there for re-checking
against M2-104-UM.
