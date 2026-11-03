//! The DefReal processing chain.
//!
//! Signal order:
//!   input gain -> MICPRINT (mic character) -> HUMANIZE (chain dynamics)
//!   -> PROXIMITY (direct-path distance/tone)
//!   -> [+] ROOMSPACE return (send driven by proximity room coupling)
//!   -> [+] AMBIENCE texture
//!   -> output gain -> safety limiter -> wet/dry (realism x master amount)
//!
//! Everything is mono-internal; ROOMSPACE and AMBIENCE generate the stereo
//! field. The dry path is the pure input (post input-gain), so 0% realism
//! is genuinely transparent.

use crate::ambience::{Ambience, AmbienceParams};
use crate::envelope::{from_db, LevelMeter};
use crate::humanize::{Humanize, HumanizeParams};
use crate::limiter::SafetyLimiter;
use crate::meters::StereoMeter;
use crate::micprint::{Micprint, MicprintParams};
use crate::motion::{MotionEngine, MotionParams};
pub use crate::motion::MotionDebug;
use crate::proximity::{Proximity, ProximityParams};
use crate::realism;
use crate::roomspace::{Roomspace, RoomspaceParams};
use crate::smooth::SmoothedParam;
use crate::vad::SpeechGate;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ChainParams {
    /// master input trim, dB
    pub input_gain_db: f32,
    /// master output trim, dB
    pub output_gain_db: f32,
    /// user-facing overall effect mix 0..1
    pub master_amount: f32,
    /// REALISM macro 0..1
    pub realism: f32,
    /// hard bypass: input -> output untouched
    pub bypass: bool,
    pub micprint: MicprintParams,
    pub humanize: HumanizeParams,
    pub proximity: ProximityParams,
    pub roomspace: RoomspaceParams,
    pub ambience: AmbienceParams,
    pub motion: MotionParams,
}

impl Default for ChainParams {
    fn default() -> Self {
        Self {
            input_gain_db: 0.0,
            output_gain_db: 0.0,
            master_amount: 1.0,
            realism: 0.5,
            bypass: false,
            micprint: MicprintParams::default(),
            humanize: HumanizeParams::default(),
            proximity: ProximityParams::default(),
            roomspace: RoomspaceParams::default(),
            ambience: AmbienceParams::default(),
            motion: MotionParams::default(),
        }
    }
}

/// Telemetry produced per processed block; the audio layer publishes it.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct EngineMetrics {
    pub in_rms_db: f32,
    pub in_peak_db: f32,
    pub out_rms_db: f32,
    pub out_peak_db: f32,
    pub limiter_gain_db: f32,
    pub speech_level: f32,
    pub speech_open: bool,
    pub mic_comp_db: f32,
    pub hum_comp_db: f32,
}

const MAX_BLOCK: usize = 8192;

pub struct DspChain {
    sr: f32,
    micprint: Micprint,
    humanize: Humanize,
    proximity: Proximity,
    roomspace: Roomspace,
    ambience: Ambience,
    limiter_l: SafetyLimiter,
    limiter_r: SafetyLimiter,
    motion: MotionEngine,
    gate: SpeechGate,

    in_gain: SmoothedParam,
    out_gain: SmoothedParam,
    wet: SmoothedParam,
    room_send: SmoothedParam,

    // scratch buffers (mono)
    dry_buf: Vec<f32>,
    wet_buf: Vec<f32>,
    direct_buf: Vec<f32>,
    interleave_buf: Vec<f32>,

    eff: ChainParams,
    in_meter: LevelMeter,
    out_meter: StereoMeter,
    metrics: EngineMetrics,
    motion_debug: MotionDebug,
    processed: u64,
    seed: u64,
}

impl DspChain {
    pub fn new(sample_rate: f32, seed: u64) -> Self {
        Self {
            sr: sample_rate,
            micprint: Micprint::new(sample_rate),
            humanize: Humanize::new(sample_rate, seed),
            proximity: Proximity::new(sample_rate),
            roomspace: Roomspace::new(sample_rate),
            ambience: Ambience::new(sample_rate, seed),
            limiter_l: SafetyLimiter::new(-1.5, sample_rate),
            limiter_r: SafetyLimiter::new(-1.5, sample_rate),
            motion: MotionEngine::new(seed),
            gate: SpeechGate::new(sample_rate, -45.0, -55.0, 250.0),
            in_gain: SmoothedParam::new(1.0, 20.0, sample_rate),
            out_gain: SmoothedParam::new(1.0, 20.0, sample_rate),
            wet: SmoothedParam::new(0.5, 25.0, sample_rate),
            room_send: SmoothedParam::new(1.0, 40.0, sample_rate),
            dry_buf: vec![0.0; MAX_BLOCK],
            wet_buf: vec![0.0; MAX_BLOCK],
            direct_buf: vec![0.0; MAX_BLOCK],
            interleave_buf: vec![0.0; MAX_BLOCK * 2],
            eff: ChainParams::default(),
            in_meter: LevelMeter::default(),
            out_meter: StereoMeter::default(),
            metrics: EngineMetrics::default(),
            motion_debug: MotionDebug {
                state: crate::motion::MotionState::Neutral,
                distance_m: 0.32,
                target_distance_m: 0.32,
                transition_progress: 1.0,
                seconds_since_transition: 0.0,
                manual: false,
                pending: None,
            },
            processed: 0,
            seed,
        }
    }

    /// Apply parameter update once per block (control rate).
    /// Stores the macro-adjusted *effective* params for the processing loop.
    fn update_params(&mut self, p: &ChainParams) {
        let e = realism::apply(p);
        self.in_gain.set(from_db(e.input_gain_db));
        self.out_gain.set(from_db(e.output_gain_db));
        self.wet.set(realism::wet_dry(p));
        self.micprint.update(&e.micprint, self.proximity.derived.proximity_bass_db);
        self.humanize.update(&e.humanize);
        self.roomspace.update(&e.roomspace);
        self.ambience.update(&e.ambience);
        self.eff = e;
    }

    /// Process `input` mono frames into interleaved stereo `out`.
    /// `out.len()` must be `input.len() * 2`.
    /// For mono output devices use `process_to_mono`.
    pub fn process(&mut self, input: &[f32], out: &mut [f32], p: &ChainParams) {
        let n = input.len();
        debug_assert!(n <= MAX_BLOCK);
        debug_assert!(out.len() >= n * 2);

        self.update_params(p);

        // metering on input
        self.in_meter.push_block(input);

        if p.bypass {
            self.route_bypass(input, out);
            self.motion_debug = self.motion.debug();
            return;
        }

        let dt = n as f32 / self.sr;
        // speech estimation on raw (post-in-gain) signal, decimated by 4
        let g_in = self.in_gain.get();
        for &s in input.iter().step_by(4) {
            self.gate.process(s * g_in);
        }
        let speech_open = self.gate.is_open();
        let speech_level = self.gate.level();
        let pos = self.motion.update(dt, speech_open, speech_level, &self.eff.motion);
        self.proximity.update(&pos, &self.eff.proximity);
        self.room_send.set(self.proximity.derived.room_send);

        // --- per-sample ---
        let wet_on = self.wet.get() > 0.0005 || !self.wet.is_settled();
        for i in 0..n {
            let x = input[i] * self.in_gain.next();
            self.dry_buf[i] = x;
            if !wet_on {
                self.direct_buf[i] = 0.0;
                continue;
            }
            let mut y = x;
            if self.eff.micprint.enabled {
                y = self.micprint.process(y);
            }
            if self.eff.humanize.enabled {
                y = self.humanize.process(y);
            }
            self.wet_buf[i] = y; // "voice in room" source
            self.direct_buf[i] = if self.eff.proximity.enabled {
                self.proximity.process(y)
            } else {
                y
            };
        }

        // room + ambience + mix
        for i in 0..n {
            let (mut l, mut r) = if wet_on && self.eff.roomspace.enabled {
                let send = self.wet_buf[i] * self.room_send.next();
                let (rl, rr) = self.roomspace.process(send);
                (self.direct_buf[i] + rl, self.direct_buf[i] + rr)
            } else if wet_on {
                (self.direct_buf[i], self.direct_buf[i])
            } else {
                (0.0, 0.0)
            };

            if wet_on && self.eff.ambience.enabled {
                let (al, ar) = self.ambience.process();
                l += al;
                r += ar;
            }

            let g = self.out_gain.next();
            l = self.limiter_l.process(l * g);
            r = self.limiter_r.process(r * g);

            let w = self.wet.next();
            let dry = self.dry_buf[i];
            out[i * 2] = dry * (1.0 - w) + l * w;
            out[i * 2 + 1] = dry * (1.0 - w) + r * w;
            self.out_meter.push_frame(out[i * 2], out[i * 2 + 1]);
        }

        self.processed += n as u64;
        self.motion_debug = self.motion.debug();
        self.update_metrics();
    }

    /// Mono output variant (for mono devices / offline single-channel export).
    pub fn process_to_mono(&mut self, input: &[f32], out: &mut [f32], p: &ChainParams) {
        let n = input.len();
        let mut tmp = std::mem::take(&mut self.interleave_buf);
        self.process(input, &mut tmp[..n * 2], p);
        for i in 0..n {
            out[i] = tmp[i * 2];
        }
        self.interleave_buf = tmp;
    }

    fn route_bypass(&mut self, input: &[f32], out: &mut [f32]) {
        for i in 0..input.len() {
            out[i * 2] = input[i];
            out[i * 2 + 1] = input[i];
            self.out_meter.push_frame(input[i], input[i]);
        }
        self.processed += input.len() as u64;
        self.update_metrics();
    }

    fn update_metrics(&mut self) {
        let (ir, ip) = self.in_meter.snapshot(0.9);
        let (or, op) = self.out_meter.snapshot(0.9);
        self.metrics = EngineMetrics {
            in_rms_db: crate::envelope::to_db(ir),
            in_peak_db: crate::envelope::to_db(ip),
            out_rms_db: crate::envelope::to_db(or),
            out_peak_db: crate::envelope::to_db(op),
            limiter_gain_db: crate::envelope::to_db(
                self.limiter_l.current_gain().min(self.limiter_r.current_gain()),
            ),
            speech_level: self.gate.level(),
            speech_open: self.gate.is_open(),
            mic_comp_db: self.micprint.current_comp_gain_db(),
            hum_comp_db: self.humanize.current_gain_db(),
        };
    }

    pub fn metrics(&self) -> EngineMetrics {
        self.metrics
    }

    pub fn motion_debug(&self) -> &MotionDebug {
        &self.motion_debug
    }

    pub fn motion_mut(&mut self) -> &mut MotionEngine {
        &mut self.motion
    }

    pub fn processed_samples(&self) -> u64 {
        self.processed
    }

    /// Full state reset (keeps seed).
    pub fn reset(&mut self) {
        self.micprint.reset();
        self.humanize.reset();
        self.roomspace.reset();
        self.limiter_l.reset();
        self.limiter_r.reset();
        self.motion = MotionEngine::new(self.seed);
        self.gate = SpeechGate::new(self.sr, -45.0, -55.0, 250.0);
        self.in_meter.reset();
        self.out_meter.reset();
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::PositionState;
    use std::f32::consts::PI;

    fn sine(n: usize, amp: f32, sr: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * PI * 220.0 * i as f32 / sr).sin())
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn bypass_is_bit_transparent() {
        let mut c = DspChain::new(48000.0, 1);
        let p = ChainParams { bypass: true, ..Default::default() };
        let input = sine(512, 0.3, 48000.0);
        let mut out = vec![0.0; 1024];
        c.process(&input, &mut out, &p);
        for i in 0..512 {
            assert_eq!(out[i * 2], input[i]);
            assert_eq!(out[i * 2 + 1], input[i]);
        }
    }

    #[test]
    fn zero_realism_is_dry() {
        let mut c = DspChain::new(48000.0, 1);
        let p = ChainParams { realism: 0.0, ..Default::default() };
        let input = sine(512, 0.3, 48000.0);
        let mut out = vec![0.0; 1024];
        for _ in 0..40 {
            c.process(&input, &mut out, &p);
        }
        let mut diff = 0.0f32;
        for i in 0..512 {
            diff = diff.max((out[i * 2] - input[i]).abs());
        }
        assert!(diff < 1e-4, "diff {diff}");
    }

    #[test]
    fn processing_adds_room_character() {
        let mut c = DspChain::new(48000.0, 1);
        let p = ChainParams {
            realism: 1.0,
            roomspace: crate::roomspace::RoomspaceParams {
                enabled: true,
                level: 0.9,
                ..Default::default()
            },
            ..Default::default()
        };
        // impulse-ish burst then silence; room should ring briefly
        let mut input = vec![0.0f32; 2048];
        input[0] = 0.8;
        input[100] = 0.6;
        let mut out = vec![0.0; 4096];
        c.process(&input, &mut out, &p);
        let tail: f32 = out[400..].iter().map(|s| s.abs()).sum();
        assert!(tail > 0.001, "no room tail {tail}");
    }

    #[test]
    fn silence_does_not_explode() {
        let mut c = DspChain::new(48000.0, 1);
        let p = ChainParams::default();
        let input = vec![0.0f32; 512];
        let mut out = vec![0.0; 1024];
        for _ in 0..200 {
            c.process(&input, &mut out, &p);
        }
        assert!(out.iter().all(|s| s.is_finite()));
        let r = rms(&out);
        assert!(r < 0.02, "silence rms {r}"); // ambience texture may be audible
    }

    #[test]
    fn distance_change_alters_level() {
        // forcing lean-back should reduce direct level
        let mut c = DspChain::new(48000.0, 1);
        let p = ChainParams {
            realism: 1.0,
            motion: crate::motion::MotionParams { enabled: false, ..Default::default() },
            ..Default::default()
        };
        c.motion_mut().set_manual(PositionState {
            distance_m: 0.15,
            azimuth_deg: 0.0,
            facing_deg: 0.0,
        });
        let input = sine(480, 0.3, 48000.0);
        let mut out = vec![0.0; 960];
        for _ in 0..30 {
            c.process(&input, &mut out, &p);
        }
        let near = rms(&out[..]);
        c.motion_mut().set_manual(PositionState {
            distance_m: 1.4,
            azimuth_deg: 0.0,
            facing_deg: 0.0,
        });
        for _ in 0..60 {
            c.process(&input, &mut out, &p);
        }
        let far = rms(&out[..]);
        assert!(far < near * 0.75, "near {near} far {far}");
    }
}
