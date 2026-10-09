# ADR-0009: One fixed headroom trim on the kit mix

Status: Accepted

## Context

The brief asks for a dumb master output whose default peaks sit near
−6 dBFS, with one output-gain control and no master compression. Each voice
was calibrated relative to the kick, which peaks near −6 dBFS on its own
(snare about −9, toms −8, clap and rim −10, cowbell −12, hats −14). In a
busy groove, hits coincide (kick with clap or toms on the same step) and add
up: the full-kit golden patterns peaked between −2.3 and −4.5 dBFS.

Options considered:

1. Recalibrate every voice lower: ten calibration constants to retune and
   re-verify, and the per-voice tests would no longer describe what a voice
   sounds like on its own.
2. A limiter or compressor on the master: ruled out by the brief.
3. One fixed trim on the summed kit.

## Decision

`dsp::Kit::process` multiplies the summed voices by `KIT_HEADROOM = 0.75`
(about −2.5 dB). Per-voice calibration is unchanged and still describes each
voice in isolation; the trim is the only place the mix level is set.

## Consequences

- Typical full-kit grooves peak between about −5 and −7 dBFS
  (`patterns/kit-*.json`), a lone kick near −8.6 dBFS. The output-gain
  control adds level when a sparse pattern needs it.
- The golden test's default-headroom check allows nothing above −4 dBFS at
  unity output gain.
- Every golden master was regenerated in the same change; the relative
  balance of the voices did not change.
- The trim is a multiplication by an exactly representable constant, so
  renders stay bit-identical across platforms (ADR-0002).
