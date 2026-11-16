//! Denormal numbers flushed to zero while audio is rendered: reverb, delay
//! and filter tails decaying towards zero otherwise reach the denormal
//! range, where x86 processors take many times longer per operation.
//! [`FlushDenormals`] sets the thread's floating-point control for its
//! lifetime and restores the previous setting when dropped.

/// Flush-to-zero (and, on x86, denormals-are-zero) for the current thread
/// while alive.
pub struct FlushDenormals {
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    previous: u64,
}

impl FlushDenormals {
    #[allow(clippy::new_without_default)]
    pub fn new() -> FlushDenormals {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        {
            let previous = control::get();
            control::set(previous | control::FLUSH);
            FlushDenormals { previous }
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        FlushDenormals {}
    }
}

impl Drop for FlushDenormals {
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
        control::set(self.previous);
    }
}

/// MXCSR: flush to zero (bit 15) and denormals are zero (bit 6).
#[cfg(target_arch = "x86_64")]
mod control {
    pub const FLUSH: u64 = (1 << 15) | (1 << 6);

    pub fn get() -> u64 {
        let mut v: u32 = 0;
        // SAFETY: stores MXCSR into a local.
        unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut v, options(nostack, preserves_flags)) };
        v as u64
    }

    pub fn set(v: u64) {
        let v = v as u32;
        // SAFETY: loads a value read from MXCSR with only the flush bits
        // changed; the bits only affect results of denormal operands.
        unsafe { std::arch::asm!("ldmxcsr [{}]", in(reg) &v, options(nostack, preserves_flags, readonly)) };
    }
}

/// FPCR: flush to zero (bit 24).
#[cfg(target_arch = "aarch64")]
mod control {
    pub const FLUSH: u64 = 1 << 24;

    pub fn get() -> u64 {
        let v: u64;
        // SAFETY: reads FPCR.
        unsafe { std::arch::asm!("mrs {}, fpcr", out(reg) v, options(nomem, nostack, preserves_flags)) };
        v
    }

    pub fn set(v: u64) {
        // SAFETY: writes a value read from FPCR with only the FZ bit changed.
        unsafe { std::arch::asm!("msr fpcr, {}", in(reg) v, options(nomem, nostack, preserves_flags)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denormals_flush_while_the_guard_lives() {
        let tiny = std::hint::black_box(f32::MIN_POSITIVE);
        let half = |x: f32| std::hint::black_box(x) * std::hint::black_box(0.5);
        assert!(half(tiny) > 0.0, "a denormal without the guard");
        {
            let _g = FlushDenormals::new();
            if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
                assert_eq!(half(tiny), 0.0);
            }
        }
        assert!(half(tiny) > 0.0, "restored");
    }
}
