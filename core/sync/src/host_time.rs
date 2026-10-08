//! A monotonic host clock in nanoseconds, shared by network sources and
//! the audio side.
//!
//! On Apple platforms this is `mach_absolute_time` scaled to nanoseconds,
//! the same time base as `AVAudioTime.hostTime`, so packet timestamps and
//! render-callback timestamps can be compared directly. Elsewhere it is a
//! process-local monotonic clock (`Instant` since first use), which is all
//! the bridge needs.

#[cfg(target_vendor = "apple")]
mod imp {
    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }

    extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    }

    fn timebase() -> (u64, u64) {
        use std::sync::OnceLock;
        static TB: OnceLock<(u64, u64)> = OnceLock::new();
        *TB.get_or_init(|| {
            let mut info = MachTimebaseInfo { numer: 1, denom: 1 };
            // SAFETY: mach_timebase_info writes into the struct we own.
            #[allow(unsafe_code)]
            unsafe {
                mach_timebase_info(&mut info);
            }
            (u64::from(info.numer.max(1)), u64::from(info.denom.max(1)))
        })
    }

    pub fn now_ticks() -> u64 {
        // SAFETY: mach_absolute_time has no preconditions.
        #[allow(unsafe_code)]
        unsafe {
            mach_absolute_time()
        }
    }

    pub fn now_ns() -> u64 {
        ticks_to_ns(now_ticks())
    }

    pub fn ticks_to_ns(ticks: u64) -> u64 {
        let (n, d) = timebase();
        ((u128::from(ticks) * u128::from(n)) / u128::from(d)) as u64
    }
}

#[cfg(not(target_vendor = "apple"))]
mod imp {
    use std::sync::OnceLock;
    use std::time::Instant;

    fn origin() -> Instant {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        *ORIGIN.get_or_init(Instant::now)
    }

    pub fn now_ns() -> u64 {
        origin().elapsed().as_nanos() as u64
    }

    pub fn now_ticks() -> u64 {
        now_ns()
    }

    pub fn ticks_to_ns(ticks: u64) -> u64 {
        ticks
    }
}

/// Current host time in nanoseconds. Monotonic; arbitrary epoch.
#[must_use]
pub fn now_ns() -> u64 {
    imp::now_ns()
}

/// Current platform host time in native ticks (what `AVAudioTime.hostTime`
/// carries on Apple platforms; nanoseconds elsewhere).
#[must_use]
pub fn now_ticks() -> u64 {
    imp::now_ticks()
}

/// Converts a platform host-time tick value (e.g. `AVAudioTime.hostTime`)
/// to nanoseconds on the same scale as [`now_ns`]. Identity off Apple
/// platforms.
#[must_use]
pub fn ticks_to_ns(ticks: u64) -> u64 {
    imp::ticks_to_ns(ticks)
}

#[cfg(test)]
mod tests {
    #[test]
    fn monotonic() {
        let a = super::now_ns();
        let b = super::now_ns();
        assert!(b >= a);
    }
}
