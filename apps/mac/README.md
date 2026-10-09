# apps/mac

The macOS app and the Swift package both Apple shells share (ADR-0008).
SwiftUI + AVAudioEngine over the Rust core's split C ABI.

## Build

Needs macOS 13+, Xcode 15+ (Swift 5.9+) and rustup.

```sh
scripts/build-xcframework.sh     # repo root: Rust core → Frameworks/Player5Core.xcframework
cd apps/mac
swift build                      # library + macOS executable
swift test                       # pattern model, #p= links, C ABI smoke tests
swift run Player5Mac             # run it (no bundle, no sandbox)
```

For a real, sandboxed `.app` (signing, entitlements, Info.plist):

```sh
brew install xcodegen
cd apps/mac && xcodegen generate # Player5Mac.xcodeproj (generated, gitignored)
open Player5Mac.xcodeproj        # or:
xcodebuild -project Player5Mac.xcodeproj -scheme Player5Mac build
```

Re-run `scripts/build-xcframework.sh` whenever `core/` changes. CI does all
of the above on every push (`.github/workflows/apple.yml`), unsigned.

## Layout

```
Package.swift                 binaryTarget Player5Core, library Player5Kit,
                              executable Player5Mac, tests Player5KitTests
Frameworks/                   Player5Core.xcframework (generated)
Sources/Player5Kit/
  Engine/EngineHost.swift     split pair, AVAudioEngine, control queue + 5 ms timer
  Engine/HostClock.swift      mach host ticks → ns (CoreMIDI timestamps)
  Model/                      Pattern/VoiceTrack/Step (spec.rs), presets,
                              UserDefaults store, #p= share codec
  Clock/                      ClockKind, network devices, CoreMIDI clock in
  Audio/OutputDevices.swift   Core Audio output devices (macOS)
  App/AppModel.swift          ObservableObject: every UI action goes here
  Views/                      step grid, voice, transport, clock, master, patterns
Sources/Player5Mac/           @main for macOS
Tests/Player5KitTests/
App/                          Info.plist + sandbox entitlements for project.yml
project.yml                   XcodeGen spec for the .app
```

## How it runs

- **Audio thread:** the `AVAudioSourceNode` render block calls
  `p5_renderer_render` with the buffer's host time and copies the mono mix
  to the other channel. Nothing else: no allocation, locks, logging or
  Swift class references.
- **Control queue:** a serial queue owns `P5Control`. A 5 ms
  `DispatchSourceTimer` calls `p5_control_tick`; every other `p5_control_*`
  call is posted to the same queue. Every ~30 ms a snapshot (playhead,
  tempo, lock, bar/beat, status, devices) goes to the main thread if it
  changed.
- **Main thread:** SwiftUI, the pattern model, the audio graph. Every edit
  sends the whole pattern as JSON (`p5_control_load_pattern_json`), like
  the web app.

Clock panel: Internal, Tap, Pro DJ Link, Opus Quad, Ableton Link, MIDI
clock (any CoreMIDI source) and Simulated. Sources the core was built
without report "not in this build yet" and the app stays on the internal
clock. Nudge (ms), latency offset (ms, plus the device's reported output
latency), quantized re-sync and tap work with every source.

Master: one output gain, the soft safety limiter, and the output device
(Core Audio; the mono mix plays on outputs 1–2). Patterns: presets, a named
library, `#p=` links compatible with the web app, pattern JSON copy/import.

## Not yet

- Verification on real hardware: see "To verify on hardware" in ADR-0008.
- Measured (loopback) latency calibration; the audio-input entitlement is
  reserved for it.
- Choosing a single MIDI source (the app listens to all of them).
