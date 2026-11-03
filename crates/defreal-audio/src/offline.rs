//! Offline rendering & A/B comparison - same DspChain as the live engine.
//! Deterministic: seed + params => identical output every run.

use crate::resample::InputResampler;
use defreal_dsp::{DspChain, Preset};
use serde::Serialize;
use std::path::Path;
use thiserror::Error;

use crate::engine::INTERNAL_RATE;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("wav read: {0}")]
    Read(#[from] hound::Error),
    #[error("unsupported wav format (need pcm integer or float): {0}")]
    Format(String),
    #[error("empty input file")]
    Empty,
    #[error("resample: {0}")]
    Resample(#[from] crate::resample::ResampleError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Serialize)]
pub struct RenderReport {
    pub input_path: String,
    pub output_path: String,
    pub in_samples: usize,
    pub out_frames: usize,
    pub in_rms_db: f32,
    pub out_rms_db: f32,
    pub out_peak_db: f32,
    pub in_rate: u32,
    pub level_match_db: f32,
}

/// Read a WAV file -> mono f32. Supports 16/24/32-bit int and float.
pub fn read_wav_mono(path: &Path) -> Result<(Vec<f32>, u32), RenderError> {
    let reader = hound::WavReader::open(path).map_err(RenderError::Read)?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let mut r = reader;
    let inter: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => r
            .samples::<f32>()
            .collect::<Result<_, _>>()
            .map_err(RenderError::Read)?,
        (hound::SampleFormat::Int, 16) => r
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.0))
            .collect::<Result<_, _>>()
            .map_err(RenderError::Read)?,
        (hound::SampleFormat::Int, 24) => r
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 8_388_608.0))
            .collect::<Result<_, _>>()
            .map_err(RenderError::Read)?,
        (hound::SampleFormat::Int, 32) => r
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 2_147_483_648.0))
            .collect::<Result<_, _>>()
            .map_err(RenderError::Read)?,
        (fmt, bits) => {
            return Err(RenderError::Format(format!("{fmt:?}/{bits}bit")));
        }
    };
    if inter.is_empty() {
        return Err(RenderError::Empty);
    }
    let frames = inter.len() / ch;
    let mut mono = vec![0.0f32; frames];
    for f in 0..frames {
        let mut a = 0.0f32;
        for c in 0..ch {
            a += inter[f * ch + c];
        }
        mono[f] = a / ch as f32;
    }
    Ok((mono, spec.sample_rate))
}

/// Resample mono f32 to INTERNAL_RATE using the same quality path as live.
fn to_internal(input: &[f32], in_rate: u32) -> Result<Vec<f32>, RenderError> {
    if in_rate == INTERNAL_RATE {
        return Ok(input.to_vec());
    }
    // chunked: feed in ~5ms input chunks
    let chunk = (in_rate as usize / 200).max(64);
    let mut res = InputResampler::new(in_rate, INTERNAL_RATE, chunk)?;
    let mut out = Vec::with_capacity(input.len() * INTERNAL_RATE as usize / in_rate as usize + 1024);
    let mut tmp = Vec::new();
    for block in input.chunks(chunk) {
        if block.len() == chunk {
            res.process(block, &mut tmp)?;
            out.extend_from_slice(&tmp);
        }
    }
    // flush tail: pad last partial chunk with zeros
    let rem = input.len() % chunk;
    if rem > 0 {
        let mut tail = vec![0.0f32; chunk];
        tail[..rem].copy_from_slice(&input[input.len() - rem..]);
        res.process(&tail, &mut tmp)?;
        let keep = tmp.len() * rem / chunk;
        out.extend_from_slice(&tmp[..keep.min(tmp.len())]);
    }
    Ok(out)
}

/// Write stereo f32 -> 16-bit WAV.
pub fn write_wav_stereo(path: &Path, interleaved: &[f32], rate: u32) -> Result<(), RenderError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(RenderError::Read)?;
    for &s in interleaved {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .map_err(RenderError::Read)?;
    }
    w.finalize().map_err(RenderError::Read)
}

fn rms_db(x: &[f32]) -> f32 {
    if x.is_empty() {
        return -100.0;
    }
    let r = (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
    20.0 * r.max(1e-8).log10()
}

/// Render `input_path` through `preset` -> stereo 48k WAV at `output_path`.
/// Deterministic given `seed`.
pub fn render_wav(
    input_path: &Path,
    output_path: &Path,
    preset: &Preset,
    seed: u64,
) -> Result<RenderReport, RenderError> {
    let (mono, in_rate) = read_wav_mono(input_path)?;
    let sig48 = to_internal(&mono, in_rate)?;

    let mut chain = DspChain::new(INTERNAL_RATE as f32, seed);
    chain.motion_mut().reseed(preset.params.motion.seed ^ seed);

    let block = 480usize; // 10ms blocks
    let mut out = Vec::with_capacity(sig48.len() * 2 + block * 4);
    let params = preset.params.clone();
    let mut in_meter = Vec::new();
    for chunk in sig48.chunks(block) {
        let mut ob = vec![0.0f32; chunk.len() * 2];
        chain.process(chunk, &mut ob, &params);
        in_meter.extend_from_slice(chunk);
        out.extend_from_slice(&ob);
    }

    write_wav_stereo(output_path, &out, INTERNAL_RATE)?;

    Ok(RenderReport {
        input_path: input_path.display().to_string(),
        output_path: output_path.display().to_string(),
        in_samples: mono.len(),
        out_frames: out.len() / 2,
        in_rms_db: rms_db(&in_meter),
        out_rms_db: rms_db(&out),
        out_peak_db: 20.0
            * out.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-8).log10(),
        in_rate,
        level_match_db: 0.0,
    })
}

/// Render A/B: original (resampled mono->stereo, level-matched reference) and
/// processed output; returns paths + the gain applied to match loudness.
pub fn render_ab(
    input_path: &Path,
    out_dir: &Path,
    preset: &Preset,
    seed: u64,
) -> Result<AbResult, RenderError> {
    let (mono, in_rate) = read_wav_mono(input_path)?;
    let sig48 = to_internal(&mono, in_rate)?;

    // reference: same signal, duplicated stereo, chain bypassed at 0% realism
    // so SRC/filter path matches
    let mut dry_params = preset.params.clone();
    dry_params.realism = 0.0;
    dry_params.bypass = false;
    let mut dry_chain = DspChain::new(INTERNAL_RATE as f32, seed);
    let mut wet_chain = DspChain::new(INTERNAL_RATE as f32, seed);
    wet_chain.motion_mut().reseed(preset.params.motion.seed ^ seed);

    let block = 480usize;
    let mut a = Vec::with_capacity(sig48.len() * 2);
    let mut b = Vec::with_capacity(sig48.len() * 2);
    for chunk in sig48.chunks(block) {
        let mut ao = vec![0.0f32; chunk.len() * 2];
        let mut bo = vec![0.0f32; chunk.len() * 2];
        dry_chain.process(chunk, &mut ao, &dry_params);
        wet_chain.process(chunk, &mut bo, &preset.params);
        a.extend_from_slice(&ao);
        b.extend_from_slice(&bo);
    }

    // level-match B to A (RMS)
    let ra = rms_db(&a);
    let rb = rms_db(&b);
    let match_db = (ra - rb).clamp(-12.0, 12.0);
    let g = 10f32.powf(match_db / 20.0);
    let b_matched: Vec<f32> = b.iter().map(|s| s * g).collect();

    let a_path = out_dir.join("ab_a_original.wav");
    let b_path = out_dir.join("ab_b_processed.wav");
    let bm_path = out_dir.join("ab_b_matched.wav");
    write_wav_stereo(&a_path, &a, INTERNAL_RATE)?;
    write_wav_stereo(&b_path, &b, INTERNAL_RATE)?;
    write_wav_stereo(&bm_path, &b_matched, INTERNAL_RATE)?;

    Ok(AbResult {
        a_path: a_path.display().to_string(),
        b_path: b_path.display().to_string(),
        b_matched_path: bm_path.display().to_string(),
        a_rms_db: ra,
        b_rms_db: rb,
        level_match_db: match_db,
        frames: a.len() / 2,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct AbResult {
    pub a_path: String,
    pub b_path: String,
    pub b_matched_path: String,
    pub a_rms_db: f32,
    pub b_rms_db: f32,
    pub level_match_db: f32,
    pub frames: usize,
}
