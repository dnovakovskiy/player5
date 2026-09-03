# Protocol notes

Digested, not copied: each file states what we rely on, and every fact
carries a link to the source it came from. Packet captures used as test
fixtures live next to the notes. Nothing here is folklore; if a fact has no
source, it does not go in.

| Note | Covers | Status |
|------|--------|--------|
| [lookahead-scheduling.md](lookahead-scheduling.md) | The two-clock scheduling pattern the sequencer is built on | Digested (session 1) |
| [pro-dj-link.md](pro-dj-link.md) | CDJ-3000 / XDJ beat packets, high-precision position, device numbering | Sources listed; digest in session 4 |
| [opus-quad.md](opus-quad.md) | Opus Quad rekordbox-lighting impersonation, BPM + beat count | Sources listed; digest in session 5 |
| [ableton-link.md](ableton-link.md) | Link timeline, session tempo, phase alignment | Sources listed; digest in session 3 |
| [midi-clock.md](midi-clock.md) | MIDI clock in (DJM USB) and Web MIDI | Sources listed; digest in session 6 |

## Reference material

- Deep Symmetry `beat-link` (Java reference implementation):
  https://github.com/Deep-Symmetry/beat-link
- Deep Symmetry `dysentery` (protocol analysis, the "Packet Analysis" PDF):
  https://github.com/Deep-Symmetry/dysentery —
  https://djl-analysis.deepsymmetry.org/
- `prolink-connect` (TypeScript implementation):
  https://github.com/EvanPurkhiser/prolink-connect
- `kyleawayan/opus-quad-pro-dj-link-analysis` (Opus Quad packet captures):
  https://github.com/kyleawayan/opus-quad-pro-dj-link-analysis
- Ableton Link C++ SDK: https://github.com/Ableton/link
- `rusty_link` crate: https://github.com/anzbert/rusty_link
- Chris Wilson, "A Tale of Two Clocks – Scheduling Web Audio with Precision":
  https://web.dev/articles/audio-scheduling
