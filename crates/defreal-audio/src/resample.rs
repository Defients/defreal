//! Rubato wrappers + clock-drift controller.
//!
//! Capture and playback run on independent clocks. We always resample
//! (even 48k -> 48k) so the output-side ratio can adapt to drift:
//! the controller watches the staging-buffer fill level and steers the
//! output resampler ratio by small ppm corrections - continuous, bounded,
//! no dropped blocks.

use rubato::{
    Resampler, SincFixedIn, SincFixedOut, SincInterpolationParameters,
    SincInterpolationType, WindowFunction,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResampleError {
    #[error("resampler build failed: {0}")]
    Build(String),
    #[error("resample failed: {0}")]
    Process(String),
}

fn params() -> SincInterpolationParameters {
    SincInterpolationParameters {
        // 96-tap windowed sinc: good quality for voice, ~1ms group delay
        sinc_len: 96,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 128,
        window: WindowFunction::BlackmanHarris2,
    }
}

/// device-rate mono -> internal-rate mono. Consumes a fixed chunk of input
/// per call, emits a variable number of internal-rate frames.
pub struct InputResampler {
    res: SincFixedIn<f32>,
    chunk: usize,
    out_buf: Vec<Vec<f32>>,
}

impl InputResampler {
    /// `chunk_dev`: how many device-rate frames are consumed per call.
    pub fn new(dev_rate: u32, internal_rate: u32, chunk_dev: usize) -> Result<Self, ResampleError> {
        let res = SincFixedIn::new(
            internal_rate as f64 / dev_rate as f64,
            1.05,
            params(),
            chunk_dev,
            1,
        )
        .map_err(|e| ResampleError::Build(e.to_string()))?;
        let out_buf = res.output_buffer_allocate(true);
        Ok(Self { res, chunk: chunk_dev, out_buf })
    }

    pub fn chunk(&self) -> usize {
        self.chunk
    }

    /// Consume exactly `chunk` device-rate frames -> internal-rate frames.
    /// Returns how many frames were produced (writes into `out`).
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<usize, ResampleError> {
        debug_assert_eq!(input.len(), self.chunk);
        let slices = [input];
        let (consumed, produced) = self
            .res
            .process_into_buffer(&slices, &mut self.out_buf, None)
            .map_err(|e| ResampleError::Process(e.to_string()))?;
        let _ = consumed;
        out.clear();
        out.extend_from_slice(&self.out_buf[0][..produced]);
        Ok(produced)
    }

    /// Latency through this resampler in *output* samples.
    pub fn output_delay(&self) -> usize {
        self.res.output_delay()
    }
}

/// internal-rate stereo -> device-rate stereo, with adjustable ratio.
/// Consumes a variable number of internal frames (`input_frames_next()`),
/// emits exactly `chunk_dev` frames per channel per call.
pub struct OutputResampler {
    res: SincFixedOut<f32>,
    nominal_ratio: f64,
    drift_ppm: f64,
}

impl OutputResampler {
    /// `chunk_dev`: exactly how many device-rate frames each call emits.
    pub fn new(dev_rate: u32, internal_rate: u32, chunk_dev: usize) -> Result<Self, ResampleError> {
        let nominal = dev_rate as f64 / internal_rate as f64;
        let res = SincFixedOut::new(nominal, 1.1, params(), chunk_dev, 2)
            .map_err(|e| ResampleError::Build(e.to_string()))?;
        Ok(Self {
            res,
            nominal_ratio: nominal,
            drift_ppm: 0.0,
        })
    }

    /// How many internal-rate input frames the next call needs.
    pub fn input_needed(&self) -> usize {
        self.res.input_frames_next()
    }

    /// Consume `input_needed()` frames of `in_l`/`in_r` and emit device-rate
    /// frames into `out_l`/`out_r`.
    pub fn process(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) -> Result<(), ResampleError> {
        self.res
            .process_into_buffer(&[in_l, in_r], &mut [out_l, out_r], None)
            .map_err(|e| ResampleError::Process(e.to_string()))?;
        Ok(())
    }

    /// Steer the ratio: positive error (staging buffer draining) -> produce
    /// faster. Correction clamped to +-400 ppm - far below audibility.
    pub fn set_drift_correction(&mut self, err_frames: f64, scale: f64) {
        let ppm = (err_frames * scale).clamp(-400.0, 400.0);
        self.drift_ppm = ppm;
        let ratio = self.nominal_ratio * (1.0 + ppm * 1e-6);
        let _ = self.res.set_resample_ratio(ratio, true);
    }

    pub fn drift_ppm(&self) -> f64 {
        self.drift_ppm
    }

    pub fn output_delay(&self) -> usize {
        self.res.output_delay()
    }
}
