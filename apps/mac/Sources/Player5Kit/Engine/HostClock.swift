import Darwin
import Player5Core

/// Host time on Apple platforms: `mach_absolute_time` ticks, the time base
/// of `AudioTimeStamp.mHostTime` and of CoreMIDI's `MIDITimeStamp`. The core
/// timestamps observations in nanoseconds on the same clock
/// (`sync::host_time`), so shells convert ticks to nanoseconds here.
enum HostClock {
    /// `mach_timebase_info` as (numer, denom): ns = ticks × numer / denom.
    private static let timebase: (numer: UInt64, denom: UInt64) = {
        var info = mach_timebase_info_data_t()
        guard mach_timebase_info(&info) == KERN_SUCCESS, info.numer > 0, info.denom > 0 else {
            return (1, 1)
        }
        return (UInt64(info.numer), UInt64(info.denom))
    }()

    /// Converts host ticks to nanoseconds without overflow or rounding
    /// drift (128-bit intermediate).
    static func nanoseconds(fromTicks ticks: UInt64) -> UInt64 {
        let tb = timebase
        if tb.numer == tb.denom {
            return ticks
        }
        let product = ticks.multipliedFullWidth(by: tb.numer)
        if product.high >= tb.denom {
            return UInt64.max
        }
        return tb.denom.dividingFullWidth(product).quotient
    }

    /// Now, in nanoseconds on the core's host clock.
    static func nowNanoseconds() -> UInt64 {
        p5_host_time_ns()
    }
}
