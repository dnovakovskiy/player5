# MIDI clock

Status: sources collected; to be digested in session 6 alongside
`MidiClockIn` and `WebMidiClock`.

Sources to digest:

- MIDI 1.0 Detailed Specification, System Real-Time messages (Timing Clock
  0xF8 at 24 ppqn, Start 0xFA, Continue 0xFB, Stop 0xFC):
  https://midi.org/midi-1-0-detailed-specification
- Web MIDI API: https://www.w3.org/TR/webmidi/ — and browser support
  (Safari has none): https://caniuse.com/midi
- DJM-series MIDI clock output behaviour: the relevant mixer's MIDI
  implementation chart (AlphaTheta support site), to be linked per model.

Design note to confirm: 24 ppqn gives tempo only; phase must be inferred
from Start/Continue and the tick count, and jitter smoothed by the PLL.
