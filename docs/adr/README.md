# Architecture decision records

Numbered, append-only. To change a decision, write a new ADR that supersedes
the old one and link both ways; never edit history.

| # | Title | Status |
|---|-------|--------|
| [0001](0001-architecture.md) | One Rust core, lookahead scheduling, pluggable clocks | Accepted |
| [0002](0002-deterministic-dsp-math.md) | Deterministic math on the render path | Accepted |
| [0003](0003-parameters-as-events.md) | Parameter changes travel through the event queue | Accepted |
| [0004](0004-web-shell-first.md) | Web shell first; the browser consumes the C ABI as WASM | Accepted (amends 0001) |
| [0005](0005-ableton-link-licensing.md) | Ableton Link stays behind an off-by-default feature (GPL) | Accepted; distribution decision open |
| [0006](0006-clock-following.md) | Following an external clock: the phase-locked follower | Accepted |
| [0007](0007-bridge.md) | A headless bridge serves the network clock (and the app) to browsers | Accepted |
| [0008](0008-apple-shells.md) | Apple shells: one XCFramework, one Swift package, XcodeGen apps | Accepted |
| [0009](0009-kit-headroom.md) | One fixed headroom trim on the kit mix | Accepted |
| [0010](0010-web-delivery.md) | Web delivery: runtime fallbacks, single-file artifact, offline PWA, time mapping | Accepted (amends 0004) |
| [0011](0011-realign-semantics.md) | Two kinds of realign, whole flams, flushes that always land | Accepted (amends 0006) |
| [0012](0012-bridge-origin-policy.md) | Which pages may use the bridge (Origin check, framing, per-peer cap) | Accepted (amends 0007) |

Template: Context · Decision · Consequences · Sources (if any).
