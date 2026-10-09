# apps/ios

The iOS app. All of its code is the shared Swift package in `apps/mac`
(product `Player5Kit`); this folder holds only the entry point, Info.plist,
entitlements and the XcodeGen spec (ADR-0008).

## Build

Needs macOS with Xcode 15+, rustup and XcodeGen (`brew install xcodegen`).

```sh
scripts/build-xcframework.sh     # repo root: builds the Rust core for iOS + simulator
cd apps/ios
xcodegen generate                # Player5.xcodeproj (generated, gitignored)
open Player5.xcodeproj
```

Simulator build without signing (what CI runs):

```sh
xcodebuild build -project Player5.xcodeproj -scheme Player5 \
  -destination 'generic/platform=iOS Simulator' CODE_SIGNING_ALLOWED=NO
```

To run on a device, set your team (`DEVELOPMENT_TEAM` in `project.yml` or
in Xcode) and change the bundle id from `com.example.player5`.

## Platform notes

- Audio: `AVAudioSession` category `.playback`, 5 ms preferred buffer,
  `UIBackgroundModes: audio` so playback continues with the screen locked.
  Interruptions and media-server resets restart the engine. The screen
  stays awake while the app is open.
- Audio starts on the first Play (or when an external clock is chosen), so
  opening the app does not stop other audio.
- Network clocks (Pro DJ Link, Ableton Link) need Apple's multicast
  entitlement. `Player5.entitlements` carries the key commented out and
  `project.yml` does not reference the file yet; see
  `docs/ios-multicast-entitlement.md`. Until it is granted those sources
  fail to start and the app says so; Internal, Tap and Simulated work.
- `NSLocalNetworkUsageDescription` is set for the local-network prompt.
- MIDI clock in is macOS-only for now.
