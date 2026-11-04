//! Flushing denormal numbers to zero while sound renders, so decaying feedback never slows the
//! processor down.

/// Turns on flush-to-zero and denormals-are-zero for this thread while it lives, and puts the
/// old setting back when dropped. Does nothing on processors without them.
pub struct FlushGuard {
    #[cfg_attr(not(any(target_arch = "x86", target_arch = "x86_64")), allow(dead_code))]
    old: u32,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod csr {
    /// Flush to zero (bit 15) and denormals are zero (bit 6).
    pub const FTZ_DAZ: u32 = 0x8040;

    pub fn get() -> u32 {
        let mut v: u32 = 0;
        // SAFETY: stmxcsr only writes the four bytes it is pointed at.
        unsafe { std::arch::asm!("stmxcsr [{}]", in(reg) &mut v, options(nostack, preserves_flags)) };
        v
    }

    pub fn set(v: u32) {
        // SAFETY: ldmxcsr only changes this thread's float control flags; the bits written are
        // ones `get` read back or the two flush flags, which are valid.
        unsafe { std::arch::asm!("ldmxcsr [{}]", in(reg) &v, options(nostack, preserves_flags)) };
    }
}

impl FlushGuard {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let old = csr::get();
            csr::set(old | csr::FTZ_DAZ);
            Self { old }
        }
        #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
        Self { old: 0 }
    }
}

impl Drop for FlushGuard {
    fn drop(&mut self) {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        csr::set(self.old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    fn denormals_flush_inside_the_guard_and_come_back_after() {
        let tiny = std::hint::black_box(f32::MIN_POSITIVE);
        let half = std::hint::black_box(0.5f32);
        assert!(tiny * half > 0.0, "denormals exist outside");
        {
            let _g = FlushGuard::new();
            assert_eq!(std::hint::black_box(tiny) * std::hint::black_box(half), 0.0);
        }
        assert!(std::hint::black_box(tiny) * std::hint::black_box(half) > 0.0);
    }
}
