//! One-pole smoothing for knob values, so turning a knob never clicks.

/// Glides towards its target, getting 99 % of the way there in its time.
#[derive(Clone, Debug)]
pub struct Smoother {
    k: f32,
    value: f32,
    target: f32,
}

impl Smoother {
    /// The usual time for a knob, in milliseconds.
    pub const KNOB_MS: f32 = 20.0;

    /// A smoother that takes `time_ms` to get 99 % of the way, starting at 0.
    pub fn new(time_ms: f32) -> Self {
        let samples = (time_ms.max(0.0) / 1_000.0 * crate::SAMPLE_RATE).max(1.0);
        Self { k: 1.0 - 0.01f32.powf(1.0 / samples), value: 0.0, target: 0.0 }
    }

    /// Where it glides to.
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Where it is gliding to.
    pub fn target(&self) -> f32 {
        self.target
    }

    /// Jumps straight to `value`, with nothing to glide.
    pub fn reset(&mut self, value: f32) {
        self.value = value;
        self.target = value;
    }

    /// The next sample's value.
    #[inline]
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f32 {
        self.value += (self.target - self.value) * self.k;
        if (self.target - self.value).abs() <= 2e-5 * self.target.abs().max(1.0) {
            self.value = self.target;
        }
        self.value
    }

    /// Fills `out` with the next values.
    pub fn fill(&mut self, out: &mut [f32]) {
        for v in out {
            *v = self.next();
        }
    }

    /// The value now.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Whether it has arrived.
    pub fn settled(&self) -> bool {
        self.value == self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoother_reaches_99_percent_in_its_time() {
        let mut s = Smoother::new(20.0);
        s.set_target(1.0);
        let n = (0.02 * crate::SAMPLE_RATE) as usize;
        let mut last = 0.0;
        for _ in 0..n - 1 {
            last = s.next();
        }
        assert!(last < 0.99, "not there early: {last}");
        let at = s.next();
        assert!((0.9895..0.9905).contains(&at), "at its time: {at}");
        for _ in 0..48_000 {
            s.next();
        }
        assert!(s.settled());
        s.reset(0.25);
        assert_eq!(s.next(), 0.25);
    }
}
