//! Subtle saturation. Rational-function soft clipper:
//! cheap, monotonic, no discontinuities, bounded to +-1.

/// Soft clip; `drive` in 1.0..~8.0. Output stays within [-1, 1).
/// Input is bounded to +-3 before the rational curve (the curve is only
/// monotonic/bounded on that interval), then level-compensated so more
/// drive reads as more density rather than more volume.
#[inline(always)]
pub fn soft_clip(x: f32, drive: f32) -> f32 {
    let d = (x * drive).clamp(-3.0, 3.0);
    let y = d * (27.0 + d * d) / (27.0 + 9.0 * d * d);
    y / (0.7 + 0.3 * drive.min(4.0))
}

/// Hard safety clip at +-limit.
#[inline(always)]
pub fn hard_clip(x: f32, limit: f32) -> f32 {
    x.clamp(-limit, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded() {
        for i in -100..=100 {
            let x = i as f32 * 0.1;
            let y = soft_clip(x, 4.0);
            assert!(y.abs() <= 1.0, "x={x} y={y}");
            assert!(y.is_finite());
        }
    }

    #[test]
    fn near_linear_at_low_levels() {
        let x = 0.05;
        let y = soft_clip(x, 1.0);
        assert!((y - x).abs() < 0.01);
    }

    #[test]
    fn odd_symmetry() {
        let a = soft_clip(0.6, 2.0);
        let b = soft_clip(-0.6, 2.0);
        assert!((a + b).abs() < 1e-6);
    }
}
