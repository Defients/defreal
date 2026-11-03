//! Lightweight acoustic space synthesis.
//!
//! Two cooperating structures:
//!  - `EarlyReflections`: tapped delay line, discrete taps with per-tap gain
//!  - `LateReverb`: 4-line feedback delay network (FDN) with Householder
//!    feedback mixing and one-pole damping in the loop.
//!
//! Mono in -> mono out per instance. Stereo width comes from running two
//! instances with slightly detuned delay lengths (see roomspace.rs).

use crate::biquad::OnePole;
use crate::delay::DelayLine;

/// A single early-reflection tap spec.
#[derive(Clone, Copy, Debug)]
pub struct ReflectionTap {
    pub delay_ms: f32,
    pub gain: f32,
}

pub struct EarlyReflections {
    line: DelayLine,
    taps: Vec<(f32, f32)>, // (delay_samples, gain)
    sr: f32,
    max_ms: f32,
}

impl EarlyReflections {
    pub fn new(sr: f32, max_ms: f32) -> Self {
        let max_samples = (max_ms * 0.001 * sr) as usize + 4;
        Self {
            line: DelayLine::new(max_samples),
            taps: Vec::with_capacity(8),
            sr,
            max_ms,
        }
    }

    /// Re-declare the tap pattern. Clamped into capacity.
    pub fn set_taps(&mut self, taps: &[ReflectionTap], level: f32) {
        self.taps.clear();
        for t in taps.iter().take(8) {
            let d = (t.delay_ms.clamp(1.0, self.max_ms - 1.0) * 0.001 * self.sr).max(1.0);
            self.taps.push((d, (t.gain * level).clamp(0.0, 0.9)));
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // read BEFORE push so a tap of `d` samples is exactly d samples late
        let mut y = 0.0;
        for &(d, g) in &self.taps {
            y += self.line.read_frac(d) * g;
        }
        self.line.push(x);
        y
    }

    pub fn reset(&mut self) {
        self.line.reset();
    }
}

/// 4-line FDN. Householder mixing matrix: F = I - (2/N) * 1*1^T,
/// orthogonal => energy-preserving; per-line feedback gain sets decay.
pub struct LateReverb {
    lines: [DelayLine; 4],
    damping: [OnePole; 4],
    delays: [f32; 4], // samples, fractional
    feedback: [f32; 4],
    out_gains: [f32; 4],
    state: [f32; 4],
    sr: f32,
}

impl LateReverb {
    /// `base_delays_ms`: 4 mutually-incommensurate delay lengths.
    /// `decay_ms`: target RT60-ish decay time; feedback gains derived from it.
    pub fn new(sr: f32, base_delays_ms: [f32; 4], decay_ms: f32) -> Self {
        let max_d = base_delays_ms
            .iter()
            .cloned()
            .fold(0.0f32, f32::max)
            .max(10.0);
        let cap = (max_d * 1.3 * 0.001 * sr) as usize + 8;
        let mut r = Self {
            lines: [
                DelayLine::new(cap),
                DelayLine::new(cap),
                DelayLine::new(cap),
                DelayLine::new(cap),
            ],
            damping: [
                OnePole::lowpass(6000.0, sr),
                OnePole::lowpass(6000.0, sr),
                OnePole::lowpass(6000.0, sr),
                OnePole::lowpass(6000.0, sr),
            ],
            delays: [0.0; 4],
            feedback: [0.0; 4],
            out_gains: [1.0, 1.0, -1.0, -1.0],
            state: [0.0; 4],
            sr,
        };
        r.set_delays(&base_delays_ms);
        r.set_decay(decay_ms);
        r
    }

    /// Maximum delay capacity in ms; keep requested delays below this.
    pub fn max_delay_ms(&self) -> f32 {
        (self.lines[0].capacity() as f32 / self.sr) * 1000.0 / 1.3
    }

    pub fn set_delays(&mut self, delays_ms: &[f32; 4]) {
        for i in 0..4 {
            self.delays[i] =
                (delays_ms[i].clamp(5.0, self.max_delay_ms()) * 0.001 * self.sr).max(2.0);
        }
    }

    /// feedback gain g = 10^(-3 * delay / rt60) per loop traversal.
    pub fn set_decay(&mut self, decay_ms: f32) {
        let rt = decay_ms.max(30.0);
        for i in 0..4 {
            let loop_ms = self.delays[i] / self.sr * 1000.0;
            self.feedback[i] = 10f32.powf(-3.0 * loop_ms / rt).clamp(0.0, 0.97);
        }
    }

    pub fn set_damping_hz(&mut self, hz: f32) {
        for d in &mut self.damping {
            d.set_lowpass(hz.clamp(500.0, self.sr * 0.45), self.sr);
        }
    }

    /// Wet output only.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // Householder feedback: y_i = s_i - (2/N) * sum(s)
        let sum = self.state[0] + self.state[1] + self.state[2] + self.state[3];
        let mut out = 0.0f32;
        for i in 0..4 {
            let fb = (self.state[i] - 0.5 * sum) * self.feedback[i];
            let damped = self.damping[i].process(fb);
            self.lines[i].push(x + damped);
            let v = self.lines[i].read_frac(self.delays[i]);
            self.state[i] = v;
            out += v * self.out_gains[i] * 0.5;
        }
        out
    }

    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.reset();
        }
        self.state = [0.0; 4];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_reflections_produce_delayed_echoes() {
        let sr = 48000.0;
        let mut er = EarlyReflections::new(sr, 100.0);
        er.set_taps(
            &[
                ReflectionTap { delay_ms: 10.0, gain: 0.5 },
                ReflectionTap { delay_ms: 25.0, gain: 0.25 },
            ],
            1.0,
        );
        let mut out = vec![0.0; 4800];
        for i in 0..4800 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            out[i] = er.process(x);
        }
        let t10 = (0.010 * sr) as usize;
        let t25 = (0.025 * sr) as usize;
        assert!((out[t10] - 0.5).abs() < 0.05, "tap1 {}", out[t10]);
        assert!((out[t25] - 0.25).abs() < 0.05, "tap2 {}", out[t25]);
    }

    #[test]
    fn late_reverb_decays_and_is_stable() {
        let sr = 48000.0;
        let mut r = LateReverb::new(sr, [19.7, 24.3, 29.9, 35.1], 350.0);
        // impulse
        let mut peak_after_tail = 0.0f32;
        for i in 0..48000 {
            let x = if i == 0 { 0.8 } else { 0.0 };
            let y = r.process(x);
            assert!(y.is_finite());
            if i > 40000 {
                peak_after_tail = peak_after_tail.max(y.abs());
            }
        }
        // after ~830ms at rt60=350ms, should be well decayed
        assert!(peak_after_tail < 0.05, "tail {peak_after_tail}");
    }

    #[test]
    fn reverb_tail_nonzero_early() {
        let sr = 48000.0;
        let mut r = LateReverb::new(sr, [20.0, 25.0, 30.0, 36.0], 400.0);
        let mut energy = 0.0f32;
        for i in 0..4800 {
            let x = if i == 0 { 1.0 } else { 0.0 };
            let y = r.process(x);
            energy += y * y;
        }
        assert!(energy > 1e-4, "reverb produced no tail");
    }
}
