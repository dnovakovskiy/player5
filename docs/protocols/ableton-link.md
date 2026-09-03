# Ableton Link

Status: sources collected; to be digested in session 3 alongside the
`AbletonLink` clock source and the PLL layer.

Sources to digest:

- Ableton Link SDK and its `Link.hpp` documentation (timeline, session
  state, `beatAtTime` / `timeAtBeat`, quantum, start/stop sync):
  https://github.com/Ableton/link
- `rusty_link` crate (Rust bindings we plan to use):
  https://github.com/anzbert/rusty_link
- Link's clock is the host's monotonic clock, not the audio clock; the
  mapping to the audio sample clock is ours to build (see
  `lookahead-scheduling.md`). Confirm the recommended approach from the
  SDK's audio-thread example.
