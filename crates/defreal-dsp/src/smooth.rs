//! Parameter smoothing to prevent zipper noise.
//! Linear per-sample ramp over a configurable time; settles exactly on target.

#[derive(Clone, Debug)]
pub struct SmoothedParam {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp_samples: u32,
}

impl SmoothedParam {
    /// `ramp_ms`: time taken to move to a newly set target.
    pub fn new(value: f32, ramp_ms: f32, sample_rate: f32) -> Self {
        let ramp_samples = ((ramp_ms * 0.001 * sample_rate).max(1.0)) as u32;
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            ramp_samples,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32, ramp_ms: f32) {
        self.ramp_samples = ((ramp_ms * 0.001 * sample_rate).max(1.0)) as u32;
    }

    /// Queue a move to `v` over the configured ramp.
    pub fn set(&mut self, v: f32) {
        if (v - self.target).abs() < 1e-9 && self.remaining == 0 {
            return;
        }
        self.target = v;
        self.step = (self.target - self.current) / self.ramp_samples as f32;
        self.remaining = self.ramp_samples;
    }

    /// Jump instantly (initialization, preset load).
    pub fn snap(&mut self, v: f32) {
        self.current = v;
        self.target = v;
        self.remaining = 0;
        self.step = 0.0;
    }

    /// Advance one sample.
    #[inline(always)]
    pub fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.current += self.step;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            }
        }
        self.current
    }

    /// Current value without advancing (control-rate use).
    #[inline(always)]
    pub fn get(&self) -> f32 {
        self.current
    }

    /// Advance a whole block when only block-rate resolution is needed.
    /// Returns the end value. Uses the ramp for correctness.
    pub fn next_block(&mut self, n: usize) -> f32 {
        for _ in 0..n {
            self.next();
        }
        self.current
    }

    #[inline]
    pub fn is_settled(&self) -> bool {
        self.remaining == 0
    }
}

/// Apply a smoothed gain to a buffer in-place.
pub fn apply_gain_smooth(p: &mut SmoothedParam, buf: &mut [f32]) {
    if p.is_settled() {
        let g = p.get();
        if (g - 1.0).abs() < 1e-6 {
            return;
        }
        for s in buf.iter_mut() {
            *s *= g;
        }
    } else {
        for s in buf.iter_mut() {
            *s *= p.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_to_target() {
        let mut p = SmoothedParam::new(0.0, 10.0, 48000.0);
        p.set(1.0);
        let v = p.next_block(480);
        assert!((v - 1.0).abs() < 1e-4);
        assert!(p.is_settled());
    }

    #[test]
    fn no_overshoot() {
        let mut p = SmoothedParam::new(0.0, 5.0, 48000.0);
        p.set(0.5);
        let mut max_v = 0.0f32;
        for _ in 0..480 {
            max_v = max_v.max(p.next());
        }
        assert!(max_v <= 0.5 + 1e-6);
    }

    #[test]
    fn snap_is_immediate() {
        let mut p = SmoothedParam::new(0.0, 100.0, 48000.0);
        p.snap(0.7);
        assert_eq!(p.next(), 0.7);
    }
}
