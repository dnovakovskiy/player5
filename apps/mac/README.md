# apps/mac

SwiftUI + AVAudioEngine shell for macOS. Arrives in session 2.

Plan: a SwiftPM multiplatform package (shared with `apps/ios`), linking the
static library built from `core/ffi` (`libplayer5.a` + the header from
`scripts/gen-header.sh`). The audio callback owns the engine's render half;
the UI owns the control half. Minimal step grid, transport, tempo.
