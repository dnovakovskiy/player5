# Architecture decision records

Numbered, append-only. To change a decision, write a new ADR that supersedes
the old one and link both ways; never edit history.

| # | Title | Status |
|---|-------|--------|
| [0001](0001-architecture.md) | One Rust core, lookahead scheduling, pluggable clocks | Accepted |
| [0002](0002-deterministic-dsp-math.md) | Deterministic math on the render path | Accepted |
| [0003](0003-parameters-as-events.md) | Parameter changes travel through the event queue | Accepted |
| [0004](0004-web-shell-first.md) | Web shell first; the browser consumes the C ABI as WASM | Accepted (amends 0001) |

Template: Context · Decision · Consequences · Sources (if any).
