# ADR-0002: Deterministic math on the render path

Status: Accepted (session 1)

## Context

The golden-master tests hash rendered audio bit-for-bit so refactors cannot
silently change the sound. The platform math libraries (`libm` on Linux,
macOS's `libsystem_m`, the WASM shim) return results for `exp`, `sin`,
`tanh` that differ in their last bits. A hash computed on one platform would
fail on another, and the CI matrix (Linux + macOS) would be meaningless.

## Decision

Every transcendental function used on the render path is implemented in
`dsp::math` from IEEE 754 basic operations only (add, mul, div, floor, bit
casts), which are correctly rounded on every target. Rust does not contract
`a * b + c` into a fused multiply-add, so results are identical everywhere.
`std`'s `f32::exp`, `sin`, `powf`, `tanh`, `ln` and friends are not called
from `dsp`, `sequencer`, `engine` render code, or any `Voice`.

Envelope, filter and tuning coefficients that need a logarithm are expressed
with precomputed `ln` constants (e.g. `exp_range(control, low, ln_ratio)`).

## Consequences

- Golden hashes are portable; CI runs the same tests on Linux and macOS and
  both must agree.
- Accuracy is documented per function (relative error ~1e-7 near zero for
  `exp`, a few ULPs for `sin_turns`), and unit tests compare against `std`
  with those tolerances.
- Analysis code (`render::analysis`) and tests may use `std` math freely;
  their results are compared with tolerances, not hashed.
- A compiler change that altered rounding of basic operations would break
  the goldens, which is the right outcome: it would also alter the sound.
