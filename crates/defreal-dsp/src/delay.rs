//! Circular delay lines with fractional (linear-interpolated) reads.

#[derive(Clone, Debug)]
pub struct DelayLine {
    buf: Vec<f32>,
    write: usize,
    len: usize,
}

impl DelayLine {
    /// Allocate a delay line with capacity for `max_samples`.
    pub fn new(max_samples: usize) -> Self {
        Self {
            buf: vec![0.0; max_samples.max(1) + 1],
            write: 0,
            len: max_samples.max(1) + 1,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, x: f32) {
        self.buf[self.write] = x;
        self.write += 1;
        if self.write >= self.len {
            self.write = 0;
        }
    }

    /// Integer-sample read, `d` samples in the past (d >= 1).
    #[inline(always)]
    pub fn read(&self, d: usize) -> f32 {
        let d = d.min(self.len - 1);
        let idx = (self.write + self.len - d) % self.len;
        self.buf[idx]
    }

    /// Fractional delay read with linear interpolation.
    #[inline(always)]
    pub fn read_frac(&self, d: f32) -> f32 {
        let d = d.clamp(0.0, (self.len - 2) as f32);
        let i = d as usize;
        let frac = d - i as f32;
        let a = self.read(i);
        let b = self.read(i + 1);
        a + (b - a) * frac
    }

    /// Write then read: convenience single-sample delay.
    #[inline(always)]
    pub fn tick(&mut self, x: f32, d: f32) -> f32 {
        let y = self.read_frac(d);
        self.push(x);
        y
    }

    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
    }

    pub fn capacity(&self) -> usize {
        self.len - 1
    }
}

/// Multi-tap reader over a shared delay line - used for early reflections.
#[derive(Clone, Debug)]
pub struct Tap {
    /// delay in samples (fractional ok)
    pub delay: f32,
    /// linear gain
    pub gain: f32,
    /// simple damping: one-pole LP coefficient per tap is handled by caller;
    /// kept here for clarity of the spec
    pub pan: f32, // -1..1 for stereo placement of this tap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_delay_exact() {
        let mut d = DelayLine::new(64);
        for i in 0..64 {
            d.push(i as f32);
        }
        // last written was 63 at write idx wrapped; read(1) => newest
        assert_eq!(d.read(1), 63.0);
        assert_eq!(d.read(10), 54.0);
    }

    #[test]
    fn fractional_delay_interpolates() {
        let mut d = DelayLine::new(64);
        for i in 0..64 {
            d.push(i as f32);
        }
        let v = d.read_frac(10.5);
        assert!((v - 53.5).abs() < 1e-4, "got {v}");
    }

    #[test]
    fn wraps_correctly() {
        let mut d = DelayLine::new(8);
        for i in 0..100 {
            d.push(i as f32);
        }
        assert_eq!(d.read(1), 99.0);
        assert_eq!(d.read(8), 92.0);
    }

    #[test]
    fn bounded_on_bad_delay() {
        let mut d = DelayLine::new(8);
        for i in 0..16 {
            d.push(i as f32);
        }
        let v = d.read_frac(9999.0);
        assert!(v.is_finite());
    }
}
