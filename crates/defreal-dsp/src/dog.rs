//! DefDog — ambient spatial canine companion.
//!
//! An independent simulated sound source sharing DefReal's acoustic room.
//! Not a pet game: a deterministic behavior state machine schedules sparse,
//! physically-coherent sound events (breath, steps, sniffs, sighs, collar
//! jingles, occasional permitted barks) which are rendered through the same
//! proximity/pickup model and the same room as the voice — at its own
//! position, independent of the speaker's position.
//!
//! Sound provenance (v0.2.0): all events are procedurally synthesized in
//! `synth` — deterministic given the seed, no filesystem access, no real-time
//! decoding. They are honest syntheses (filtered noise breaths, resonant
//! collar pings, formant-burst barks), NOT recordings. `MANIFEST` documents
//! each category. A future `assets/dog/*.wav` overlay can replace any event
//! category with licensed recordings without touching the engine.

use crate::biquad::{Biquad, BiquadKind};
use crate::motion::PositionState;
use crate::proximity::{derive_from_position, ProximityDerived};
use crate::rng::Rng;
use crate::roomspace::{Roomspace, RoomspaceParams};
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// === configuration ===========================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DogSize {
    Small,
    Medium,
    Large,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DogTemperament {
    Calm,
    Curious,
    Playful,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DogActivity {
    Rare,
    Occasional,
    Frequent,
}

/// Which output branch(es) receive the dog bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DogRoute {
    Both,
    Main,
    Monitor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DogState {
    Resting,
    Wandering,
    Investigating,
    Attentive,
    Vocalizing,
    Settling,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DogConfig {
    pub enabled: bool,
    pub size: DogSize,
    pub temperament: DogTemperament,
    pub activity: DogActivity,
    /// dog bus gain before shared output protection, dB
    pub volume_db: f32,
    pub allow_bark: bool,
    pub route: DogRoute,
    /// dog position override (from stage drag); None = autonomous
    pub manual_pos: Option<(f32, f32)>, // (distance_m, azimuth_deg)
}

impl Default for DogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            size: DogSize::Medium,
            temperament: DogTemperament::Calm,
            activity: DogActivity::Rare,
            volume_db: -24.0,
            allow_bark: false,
            route: DogRoute::Both,
            manual_pos: None,
        }
    }
}

/// Manual commands from the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DogCommand {
    Come,
    Settle,
    Stay,
}

// === events ==================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DogEvent {
    Breath,
    Pant,
    Sniff,
    Whine,
    Sigh,
    Step,
    Collar,
    SettleDown,
    Move,
    Bark,
    BarkCluster,
}
pub const N_EVENTS: usize = 11;
const EV: [DogEvent; N_EVENTS] = [
    DogEvent::Breath,
    DogEvent::Pant,
    DogEvent::Sniff,
    DogEvent::Whine,
    DogEvent::Sigh,
    DogEvent::Step,
    DogEvent::Collar,
    DogEvent::SettleDown,
    DogEvent::Move,
    DogEvent::Bark,
    DogEvent::BarkCluster,
];

fn ev_idx(e: DogEvent) -> usize {
    EV.iter().position(|&x| x == e).unwrap_or(0)
}

/// Sound-library manifest entry (one per event category).
pub struct ManifestEntry {
    pub event: DogEvent,
    /// synthesized buffers per size (some events are size-shared)
    pub per_size: bool,
    pub provenance: &'static str,
}

pub const MANIFEST: &[ManifestEntry] = &[
    ManifestEntry { event: DogEvent::Breath, per_size: true, provenance: "synthesized: enveloped filtered noise" },
    ManifestEntry { event: DogEvent::Pant, per_size: true, provenance: "synthesized: periodic noise bursts" },
    ManifestEntry { event: DogEvent::Sniff, per_size: false, provenance: "synthesized: short noise packet pairs" },
    ManifestEntry { event: DogEvent::Whine, per_size: true, provenance: "synthesized: gliding tone + breath" },
    ManifestEntry { event: DogEvent::Sigh, per_size: true, provenance: "synthesized: slow filtered-noise swell" },
    ManifestEntry { event: DogEvent::Step, per_size: true, provenance: "synthesized: resonant thump + transient" },
    ManifestEntry { event: DogEvent::Collar, per_size: false, provenance: "synthesized: decaying metallic ping cluster" },
    ManifestEntry { event: DogEvent::SettleDown, per_size: true, provenance: "synthesized: body flump + exhale" },
    ManifestEntry { event: DogEvent::Move, per_size: true, provenance: "synthesized: soft shuffle noise" },
    ManifestEntry { event: DogEvent::Bark, per_size: true, provenance: "synthesized: impulse train + formant sweep" },
    ManifestEntry { event: DogEvent::BarkCluster, per_size: true, provenance: "synthesized: timed bark sequence" },
];

// === procedural synthesis ====================================================

mod synth {
    use super::*;
    use std::f32::consts::PI;

    fn env(t: f32, a: f32, d: f32, total: f32) -> f32 {
        // attack-decay envelope, no sustain
        if t < a {
            t / a.max(1e-4)
        } else {
            (1.0 - (t - a) / (total - a).max(1e-4)).max(0.0).powf(d)
        }
    }

    /// one-pole LP noise helper state
    struct NoiseLp { y: f32, k: f32 }
    impl NoiseLp {
        fn new(hz: f32, sr: f32) -> Self {
            Self { y: 0.0, k: 1.0 - (-2.0 * PI * hz / sr).exp() }
        }
        fn next(&mut self, w: f32) -> f32 {
            self.y += self.k * (w - self.y);
            self.y
        }
    }

    /// Render one variation of `ev` for `size` into a new buffer (48 kHz mono).
    /// Deterministic for a given rng stream.
    pub fn render(ev: DogEvent, size: DogSize, sr: f32, rng: &mut Rng) -> Vec<f32> {
        let (base_f, body, step_thump) = match size {
            DogSize::Small => (420.0f32, 0.55f32, 240.0f32),
            DogSize::Medium => (300.0, 0.8, 160.0),
            DogSize::Large => (170.0, 1.25, 95.0),
        };
        match ev {
            DogEvent::Breath => {
                // slow inhale/exhale noise, size-scaled bandwidth & period
                let dur = rng.range(1.6, 2.4) / body.sqrt();
                let n = (dur * sr) as usize;
                let mut lp = NoiseLp::new(600.0 * body, sr);
                let mut out = Vec::with_capacity(n);
                let cycles = rng.range(1.0, 2.0);
                for i in 0..n {
                    let t = i as f32 / n as f32;
                    let swell = (t * PI * cycles).sin().max(0.0);
                    let w = rng.gauss() * 0.5;
                    out.push(lp.next(w) * swell * 0.5);
                }
                out
            }
            DogEvent::Pant => {
                let rate = rng.range(3.0, 4.5) / body.cbrt();
                let dur = rng.range(1.5, 3.0);
                let n = (dur * sr) as usize;
                let mut lp = NoiseLp::new(900.0 * body, sr);
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    let ph = (t * rate).fract();
                    let e = env(ph, 0.06, 1.8, 1.0 / rate);
                    out.push(lp.next(rng.gauss() * 0.6) * e * 0.55);
                }
                out
            }
            DogEvent::Sniff => {
                // 2-3 sharp sniffs
                let sniffs = 2 + (rng.below(2) as usize);
                let dur = 0.22 * sniffs as f32 + 0.3;
                let n = (dur * sr) as usize;
                let mut lp = NoiseLp::new(2400.0, sr);
                let mut out = vec![0.0f32; n];
                for s in 0..sniffs {
                    let start = (s as f32 * 0.22 * sr) as usize;
                    let len = (0.13 * sr) as usize;
                    for i in 0..len.min(n - start) {
                        let t = i as f32 / len as f32;
                        out[start + i] += lp.next(rng.gauss()) * env(t, 0.02, 2.5, 1.0) * 0.7;
                    }
                }
                out
            }
            DogEvent::Whine => {
                let dur = rng.range(0.6, 1.2);
                let n = (dur * sr) as usize;
                let f0 = base_f * rng.range(1.8, 2.6);
                let mut lp = NoiseLp::new(1200.0, sr);
                let mut ph = 0.0f32;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    let e = env(t, 0.08, 1.4, dur);
                    let glide = f0 * (1.0 + 0.25 * (t / dur) + 0.05 * (t * 11.0).sin());
                    ph += 2.0 * PI * glide / sr;
                    let tone = ph.sin() * 0.5 + (2.0 * ph).sin() * 0.12;
                    let breath = lp.next(rng.gauss()) * 0.15;
                    out.push((tone + breath) * e * 0.4);
                }
                out
            }
            DogEvent::Sigh => {
                let dur = rng.range(0.9, 1.5) * body.cbrt();
                let n = (dur * sr) as usize;
                let mut lp = NoiseLp::new(450.0 * body, sr);
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    let e = env(t, 0.25, 1.9, dur);
                    out.push(lp.next(rng.gauss() * 0.7) * e * 0.6);
                }
                out
            }
            DogEvent::Step => {
                // resonant floor thump + click transient
                let dur = 0.14;
                let n = (dur * sr) as usize;
                let f = step_thump * rng.range(0.9, 1.15);
                let mut lp = NoiseLp::new(3000.0, sr);
                let mut ph = 0.0f32;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    ph += 2.0 * PI * f / sr;
                    let thump = ph.sin() * (-(t * 28.0).exp());
                    let click = lp.next(rng.gauss()) * (-(t * 90.0).exp()) * 0.35;
                    out.push((thump * 0.9 + click) * env(t, 0.002, 2.0, dur));
                }
                out
            }
            DogEvent::Collar => {
                // 1-3 metallic pings (jingle tags)
                let pings = 1 + rng.below(3) as usize;
                let dur = 0.1 * pings as f32 + 0.45;
                let n = (dur * sr) as usize;
                let mut out = vec![0.0f32; n];
                for p in 0..pings {
                    let start = (p as f32 * 0.09 * sr + rng.range(0.0, 0.03) * sr) as usize;
                    let f = rng.range(2600.0, 4200.0);
                    let f2 = f * rng.range(1.4, 1.7);
                    let len = (0.4 * sr) as usize;
                    let mut ph = 0.0f32;
                    let mut ph2 = 0.0f32;
                    for i in 0..len.min(n.saturating_sub(start)) {
                        let t = i as f32 / sr;
                        ph += 2.0 * PI * f * (1.0 - t * 0.02) / sr;
                        ph2 += 2.0 * PI * f2 / sr;
                        let e = (-(t * 14.0)).exp() * env(t, 0.001, 1.0, 0.4);
                        out[start + i] += (ph.sin() * 0.6 + ph2.sin() * 0.25) * e * 0.16;
                    }
                }
                out
            }
            DogEvent::SettleDown => {
                // soft body flump + exhale tail
                let dur = rng.range(0.7, 1.1) * body.cbrt();
                let n = (dur * sr) as usize;
                let mut lp = NoiseLp::new(300.0 * body, sr);
                let mut out = Vec::with_capacity(n);
                let flump_f = step_thump * 0.6;
                let mut ph = 0.0f32;
                for i in 0..n {
                    let t = i as f32 / sr;
                    ph += 2.0 * PI * flump_f / sr;
                    let flump = ph.sin() * (-(t * 9.0).exp()) * 0.8;
                    let breath = lp.next(rng.gauss() * 0.5) * env(t, 0.15, 1.8, dur);
                    out.push((flump + breath * 0.5) * 0.6);
                }
                out
            }
            DogEvent::Move => {
                // soft body-shuffle noise (fur/scruffle while shifting)
                let dur = rng.range(0.4, 0.8);
                let n = (dur * sr) as usize;
                let mut lp = NoiseLp::new(700.0, sr);
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    let e = env(t, 0.1, 1.6, dur);
                    out.push(lp.next(rng.gauss() * 0.6) * e * 0.45);
                }
                out
            }
            DogEvent::Bark => {
                // impulse train -> formant bandpass sweep + attack noise
                let dur = rng.range(0.14, 0.22) * body.cbrt();
                let n = (dur * sr) as usize;
                let f0 = base_f * rng.range(0.85, 1.15);
                let form = 750.0 + base_f;
                let mut bp = Biquad::new(BiquadKind::Peak { freq: form, q: 1.4, gain_db: 9.0 }, sr);
                let mut lp = NoiseLp::new(5000.0, sr);
                let mut ph = 0.0f32;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    let e = env(t, 0.012, 1.9, dur);
                    ph += 2.0 * PI * f0 / sr;
                    let saw = (ph / PI).fract() * 2.0 - 1.0; // ~impulse-rich
                    let vox = bp.process(saw) * 1.6;
                    let att = lp.next(rng.gauss()) * (-(t * 60.0).exp()) * 0.4;
                    out.push((vox + att) * e * 0.85);
                }
                out
            }
            DogEvent::BarkCluster => {
                // 2-4 barks with plausible spacing (single coherent action)
                let count = 2 + rng.below(3) as usize;
                let gap = rng.range(0.28, 0.42) / body.cbrt();
                let dur = gap * count as f32 + 0.3;
                let n = (dur * sr) as usize;
                let mut out = vec![0.0f32; n];
                for b in 0..count {
                    let one = render(DogEvent::Bark, size, sr, rng);
                    let start = (b as f32 * gap * sr) as usize;
                    for (i, &s) in one.iter().enumerate() {
                        if start + i < n {
                            out[start + i] += s * rng.range(0.85, 1.0);
                        }
                    }
                }
                out
            }
        }
    }
}

// === sample voices ============================================================

const MAX_VOICES: usize = 4;
const VARIANTS: usize = 4;

struct Voice {
    buf_idx: usize,
    pos: usize,
    gain: f32,
    fade_in: usize,
    fade_out: usize,
    len: usize,
}

struct Bank {
    /// bufs[kind][variant]
    bufs: Vec<Vec<Vec<f32>>>,
    last_variant: Vec<usize>,
}

impl Bank {
    fn new(size: DogSize, seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0xD06A1B);
        let mut bufs = Vec::with_capacity(N_EVENTS);
        for &e in &EV {
            let mut v = Vec::with_capacity(VARIANTS);
            for _ in 0..VARIANTS {
                let mut b = synth::render(e, size, 48000.0, &mut rng);
                // conservative normalize to peak 0.7
                let peak = b.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
                let g = (0.7 / peak).min(2.5);
                for s in b.iter_mut() {
                    *s *= g;
                }
                v.push(b);
            }
            bufs.push(v);
        }
        Self { bufs, last_variant: vec![usize::MAX; N_EVENTS] }
    }
}

// === behavior engine ==========================================================

/// Plausible room waypoints as (distance_m, azimuth_deg) around the mic.
const WAY_BED: (f32, f32) = (2.2, -42.0);
const WAY_DESK: (f32, f32) = (0.9, 28.0);
const WAY_DOOR: (f32, f32) = (2.6, 62.0);
const WAY_WINDOW: (f32, f32) = (1.8, -64.0);
const WAY_NEARMIC: (f32, f32) = (0.55, 8.0);
const WAY_RUG: (f32, f32) = (1.4, -8.0);
const WAYPOINTS: [(f32, f32); 6] =
    [WAY_BED, WAY_DESK, WAY_DOOR, WAY_WINDOW, WAY_NEARMIC, WAY_RUG];

/// Scheduled sound event (kind + optional delay).
#[derive(Clone, Copy, Debug)]
struct Sched {
    ev: DogEvent,
    at_s: f32,
    gain_mul: f32,
}

struct Behavior {
    state: DogState,
    state_since: f32,
    dwell: f32,
    /// current position & travel target (distance_m, azimuth_deg)
    pos: (f32, f32),
    target: Option<(f32, f32)>,
    /// seconds until next footstep while moving
    next_step: f32,
    /// event schedule (at_s is absolute time)
    queue: VecDeque<Sched>,
    /// cooldowns
    bark_cooldown: f32,
    sniff_cooldown: f32,
    sigh_cooldown: f32,
    /// Stay latch: hold position, resting-level sounds only
    stay: bool,
}

impl Behavior {
    fn new() -> Self {
        Self {
            state: DogState::Resting,
            state_since: 0.0,
            dwell: 30.0,
            pos: WAY_BED,
            target: None,
            next_step: 0.0,
            queue: VecDeque::with_capacity(8),
            bark_cooldown: 20.0,
            sniff_cooldown: 0.0,
            sigh_cooldown: 0.0,
            stay: false,
        }
    }

    fn activity_scale(a: DogActivity) -> f32 {
        match a {
            DogActivity::Rare => 3.2,
            DogActivity::Occasional => 1.0,
            DogActivity::Frequent => 0.55,
        }
    }

    fn dwell_range(&self, s: DogState, cfg: &DogConfig, rng: &mut Rng) -> f32 {
        let (lo, hi) = match s {
            DogState::Resting => (40.0, 160.0),
            DogState::Wandering => (4.0, 14.0),
            DogState::Investigating => (5.0, 18.0),
            DogState::Attentive => (3.0, 10.0),
            DogState::Vocalizing => (1.0, 4.0),
            DogState::Settling => (3.0, 8.0),
        };
        let mut d = rng.range(lo, hi);
        // activity stretches calm states, compresses them; even Frequent rests
        if matches!(s, DogState::Resting | DogState::Attentive) {
            d *= Self::activity_scale(cfg.activity);
        } else if cfg.activity == DogActivity::Frequent {
            d *= 0.8;
        }
        d
    }

    /// pick the next state when dwell expires
    fn next_state(&self, cfg: &DogConfig, rng: &mut Rng, speech: bool) -> DogState {
        if self.stay {
            return DogState::Resting;
        }
        // weighted transitions conditioned on temperament
        let w = |c: f32, u: f32, p: f32| match cfg.temperament {
            DogTemperament::Calm => c,
            DogTemperament::Curious => u,
            DogTemperament::Playful => p,
        };
        let roll = rng.f32();
        match self.state {
            DogState::Resting => {
                if roll < w(0.55, 0.30, 0.20) {
                    DogState::Resting // keep resting — silence is normal
                } else if roll < w(0.85, 0.60, 0.45) {
                    DogState::Attentive
                } else if roll < w(0.97, 0.90, 0.80) {
                    DogState::Wandering
                } else if cfg.allow_bark && self.bark_cooldown <= 0.0 {
                    DogState::Vocalizing
                } else {
                    DogState::Attentive
                }
            }
            DogState::Attentive => {
                if speech && roll < 0.3 {
                    DogState::Attentive // speech keeps it listening a moment
                } else if roll < w(0.45, 0.25, 0.20) {
                    DogState::Resting
                } else if roll < w(0.90, 0.75, 0.65) {
                    DogState::Wandering
                } else if cfg.allow_bark && self.bark_cooldown <= 0.0 && roll > 0.96 {
                    DogState::Vocalizing
                } else {
                    DogState::Resting
                }
            }
            DogState::Wandering => {
                if roll < w(0.35, 0.55, 0.45) {
                    DogState::Investigating
                } else if roll < w(0.80, 0.80, 0.80) {
                    DogState::Settling
                } else {
                    DogState::Attentive
                }
            }
            DogState::Investigating => {
                if roll < w(0.40, 0.50, 0.40) {
                    DogState::Wandering
                } else if roll < w(0.75, 0.70, 0.65) {
                    DogState::Settling
                } else {
                    DogState::Attentive
                }
            }
            DogState::Vocalizing => DogState::Attentive,
            DogState::Settling => DogState::Resting,
        }
    }

    fn enter(&mut self, s: DogState, now: f32, cfg: &DogConfig, rng: &mut Rng) {
        self.state = s;
        self.state_since = 0.0;
        self.dwell = self.dwell_range(s, cfg, rng);
        match s {
            DogState::Resting => {
                self.target = None;
                if rng.chance(0.4) {
                    self.queue.push_back(Sched { ev: DogEvent::Breath, at_s: now + rng.range(2.0, 8.0), gain_mul: 1.0 });
                }
            }
            DogState::Wandering => {
                // pick a waypoint different from current
                let cur = self.pos;
                let mut wp = WAYPOINTS[rng.below(6) as usize];
                for _ in 0..4 {
                    let d = ((wp.0 - cur.0).powi(2) + (wp.1 - cur.1).powi(2) * 0.004).sqrt();
                    if d > 0.5 {
                        break;
                    }
                    wp = WAYPOINTS[rng.below(6) as usize];
                }
                self.target = Some(wp);
                // getting up first if we were resting
                if rng.chance(0.7) {
                    self.queue.push_back(Sched { ev: DogEvent::Move, at_s: now, gain_mul: 1.0 });
                }
                if rng.chance(0.5) {
                    self.queue.push_back(Sched { ev: DogEvent::Collar, at_s: now + rng.range(0.05, 0.3), gain_mul: 1.0 });
                }
            }
            DogState::Investigating => {
                self.target = None;
                self.queue.push_back(Sched { ev: DogEvent::Sniff, at_s: now + rng.range(0.0, 0.4), gain_mul: 1.0 });
            }
            DogState::Attentive => {
                self.target = None;
                if rng.chance(0.25) {
                    self.queue.push_back(Sched { ev: DogEvent::Whine, at_s: now + rng.range(1.0, 4.0), gain_mul: 0.8 });
                }
            }
            DogState::Vocalizing => {
                if cfg.allow_bark {
                    let cluster = rng.chance(match cfg.temperament {
                        DogTemperament::Playful => 0.5,
                        DogTemperament::Curious => 0.35,
                        DogTemperament::Calm => 0.2,
                    });
                    self.queue.push_back(Sched {
                        ev: if cluster { DogEvent::BarkCluster } else { DogEvent::Bark },
                        at_s: now + 0.05,
                        gain_mul: 0.9,
                    });
                    self.bark_cooldown = rng.range(35.0, 90.0);
                }
            }
            DogState::Settling => {
                // travel to bed, then settle
                self.target = Some(WAY_BED);
            }
        }
    }
}

// === dog engine ================================================================

#[derive(Clone, Debug)]
pub struct DogDebug {
    pub enabled: bool,
    pub state: DogState,
    pub distance_m: f32,
    pub azimuth_deg: f32,
    pub moving: bool,
    pub stay: bool,
    pub active_voices: usize,
    pub last_event: Option<DogEvent>,
    pub level_db: f32,
}

pub struct DogEngine {
    cfg: DogConfig,
    beh: Behavior,
    rng: Rng,
    bank: Bank,
    bank_seed_size: DogSize,
    voices: Vec<Voice>,
    time: f32,
    // spatial path
    lp: Biquad,
    lp_hz: f32,
    door: SmoothedParam,
    direct_gain: SmoothedParam,
    send_gain: f32,
    derived: ProximityDerived,
    room: Roomspace,
    volume: SmoothedParam,
    duck: SmoothedParam,
    last_event: Option<DogEvent>,
    event_counts: [u32; N_EVENTS],
    // meters
    peak: f32,
    sum_sq: f64,
    cnt: u32,
    sr: f32,
}

impl DogEngine {
    pub fn new(sr: f32, seed: u64) -> Self {
        let cfg = DogConfig::default();
        Self {
            bank: Bank::new(cfg.size, seed),
            bank_seed_size: cfg.size,
            cfg,
            beh: Behavior::new(),
            rng: Rng::new(seed ^ 0xD06A55ED),
            voices: Vec::with_capacity(MAX_VOICES),
            time: 0.0,
            lp: Biquad::new(BiquadKind::LowPass { freq: 17000.0, q: 0.6 }, sr),
            lp_hz: 17000.0,
            door: SmoothedParam::new(1.0, 500.0, sr),
            direct_gain: SmoothedParam::new(1.0, 30.0, sr),
            send_gain: 0.5,
            derived: ProximityDerived { direct_gain: 1.0, lp_hz: 17000.0, proximity_bass_db: 0.0, room_send: 1.0 },
            room: Roomspace::new(sr),
            volume: SmoothedParam::new(0.0, 300.0, sr),
            duck: SmoothedParam::new(1.0, 800.0, sr),
            last_event: None,
            event_counts: [0; N_EVENTS],
            peak: 0.0,
            sum_sq: 0.0,
            cnt: 0,
            sr,
        }
    }

    /// Apply a new config. Rebuilds the bank if size changed.
    pub fn configure(&mut self, cfg: &DogConfig, room_p: &RoomspaceParams) {
        if cfg.size != self.bank_seed_size || cfg.size != self.cfg.size {
            self.bank = Bank::new(cfg.size, 0xD06A1B ^ self.rng_seed_bits());
            self.bank_seed_size = cfg.size;
            self.voices.clear();
        }
        // manual position pin
        if let Some((d, az)) = cfg.manual_pos {
            self.beh.pos = (d.clamp(0.15, 3.0), az.clamp(-80.0, 80.0));
            self.beh.target = None;
            if self.cfg.manual_pos.is_none() {
                // dragged once -> becomes Stay semantics
                self.beh.stay = true;
                if self.beh.state != DogState::Resting {
                    self.enter(DogState::Resting);
                }
            }
        } else if self.cfg.manual_pos.is_some() {
            self.beh.stay = false;
        }
        self.room.update(room_p);
        self.cfg = cfg.clone();
        self.volume
            .set(if cfg.enabled { crate::envelope::from_db(cfg.volume_db.clamp(-48.0, -6.0)) } else { 0.0 });
    }

    /// Follow the voice chain's room so dog and voice share one space.
    /// Cheap: only rebuilds filters when the room spec actually changed.
    pub fn sync_room(&mut self, p: &RoomspaceParams) {
        self.room.update(p);
    }

    /// ROOMLOGIC door state, 0 = closed .. 1 = open (smoothed).
    pub fn set_door(&mut self, open: f32) {
        self.door.set(open.clamp(0.0, 1.0));
    }

    /// Deliberate user preview: a couple of benign non-bark events at the
    /// current position. Never emits barks regardless of allow_bark.
    pub fn preview(&mut self) {
        self.start_voice(Sched { ev: DogEvent::Sniff, at_s: self.time, gain_mul: 0.9 });
        self.queue(DogEvent::Sigh, 0.6, 0.8);
    }

    fn rng_seed_bits(&self) -> u64 {
        // keep per-instance variation; doesn't need the original seed
        0x9E37_79B9 ^ self.bank_seed_size as u64
    }

    fn enter(&mut self, s: DogState) {
        self.beh.enter(s, self.time, &self.cfg, &mut self.rng);
    }

    pub fn command(&mut self, cmd: DogCommand) {
        match cmd {
            DogCommand::Come => {
                self.beh.stay = false;
                self.beh.target = Some(WAY_NEARMIC);
                self.enter(DogState::Wandering);
                self.beh.state = DogState::Wandering; // ensure not overridden
            }
            DogCommand::Settle => {
                self.beh.stay = false;
                self.enter(DogState::Settling);
            }
            DogCommand::Stay => {
                self.beh.stay = true;
                self.beh.target = None;
                self.enter(DogState::Resting);
            }
        }
    }

    /// Advance the behavior model by dt seconds. Called at control rate
    /// (once per pump tick), NOT per-sample.
    pub fn update(&mut self, dt: f32, speech_level: f32) {
        let b = &mut self.beh;
        b.state_since += dt;
        b.bark_cooldown = (b.bark_cooldown - dt).max(0.0);
        b.sniff_cooldown = (b.sniff_cooldown - dt).max(0.0);
        b.sigh_cooldown = (b.sigh_cooldown - dt).max(0.0);
        let speech_active = speech_level > -40.0;

        // speech ducking (conservative voice-priority)
        self.duck
            .set(if speech_active && !self.cfg.enabled { 1.0 } else if speech_active { 0.4 } else { 1.0 });

        // --- movement toward target (in mic-plane coords) ---
        if let Some(tgt) = b.target {
            let (cx, cy) = to_xy(b.pos);
            let (tx, ty) = to_xy(tgt);
            let speed = match self.cfg.size {
                DogSize::Small => 1.1,
                DogSize::Medium => 0.9,
                DogSize::Large => 0.7,
            };
            let dx = tx - cx;
            let dy = ty - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let step = speed * dt;
            if dist <= step.max(0.02) {
                b.pos = tgt;
                b.target = None;
                // arrived: settle finishes or investigation begins
                match b.state {
                    DogState::Settling => {
                        self.queue(DogEvent::SettleDown, 0.0, 1.0);
                        self.enter(DogState::Resting);
                    }
                    DogState::Wandering => {
                        if self.rng.chance(0.55) {
                            self.enter(DogState::Investigating);
                        }
                    }
                    _ => {}
                }
            } else {
                b.pos = from_xy(cx + dx / dist * step, cy + dy / dist * step);
                // footsteps along the way
                b.next_step -= dt;
                if b.next_step <= 0.0 {
                    let cad = match self.cfg.size {
                        DogSize::Small => 0.34,
                        DogSize::Medium => 0.50,
                        DogSize::Large => 0.66,
                    };
                    b.next_step = cad * self.rng.range(0.85, 1.2);
                    let sg = self.rng.range(0.7, 1.0);
                    self.queue(DogEvent::Step, 0.0, sg);
                    if self.rng.chance(0.12) {
                        let d0 = self.rng.range(0.0, 0.15);
                        self.queue(DogEvent::Collar, d0, 0.7);
                    }
                }
            }
        }

        // --- state dwell / transitions ---
        let b = &mut self.beh;
        // Settling always completes its walk to the bed before resting —
        // dwell expiry mid-route would strand the dog away from its spot.
        let settling_travel = b.state == DogState::Settling && b.target.is_some();
        if b.state_since >= b.dwell && !settling_travel {
            let next = b.next_state(&self.cfg, &mut self.rng, speech_active);
            let now = self.time;
            let cfg = self.cfg.clone();
            b.enter(next, now, &cfg, &mut self.rng);
            // arrival logic for Settling handled by movement above
        }

        // ambient in-state scheduling (sparse, non-periodic)
        match self.beh.state {
            DogState::Resting => {
                if self.beh.queue.is_empty() && self.rng.chance(dt * 0.05 / Self::act_boost(&self.cfg)) {
                    if self.beh.sigh_cooldown <= 0.0 && self.rng.chance(0.3) {
                        self.queue(DogEvent::Sigh, 0.0, 0.9);
                        self.beh.sigh_cooldown = 60.0;
                    } else {
                        self.queue(DogEvent::Breath, 0.0, 0.9);
                    }
                }
            }
            DogState::Investigating => {
                if self.beh.sniff_cooldown <= 0.0 && self.rng.chance(dt * 0.5 / Self::act_boost(&self.cfg)) {
                    self.queue(DogEvent::Sniff, 0.0, 1.0);
                    self.beh.sniff_cooldown = self.rng.range(4.0, 12.0);
                    // small positional wiggle (a sniff-step, not a teleport)
                    let (x, y) = to_xy(self.beh.pos);
                    let jx = self.rng.range(-0.04, 0.04);
                    let jy = self.rng.range(-0.03, 0.03);
                    self.beh.pos = from_xy(x + jx, y + jy);
                }
            }
            DogState::Attentive => {
                if self.rng.chance(dt * 0.03) {
                    self.queue(DogEvent::Pant, 0.0, 0.7);
                }
            }
            _ => {}
        }

        self.time += dt;
    }

    fn act_boost(cfg: &DogConfig) -> f32 {
        Behavior::activity_scale(cfg.activity)
    }

    fn queue(&mut self, ev: DogEvent, delay: f32, gain: f32) {
        if self.beh.queue.len() >= 8 {
            self.beh.queue.pop_front();
        }
        self.beh.queue.push_back(Sched { ev, at_s: self.time + delay, gain_mul: gain });
    }

    /// Fire due events into voices, render n frames of dog bus (stereo).
    /// `out` is interleaved stereo, length 2n. Real-time safe: no alloc.
    pub fn render(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        // fire due scheduled events (bounded: at most queue depth per call)
        let now = self.time;
        while let Some(&s) = self.beh.queue.front() {
            if s.at_s > now {
                break;
            }
            self.beh.queue.pop_front();
            self.start_voice(s);
        }

        let vol = self.volume.get();
        if vol <= 1e-6 && self.voices.is_empty() {
            // still update spatial so a re-enabled dog doesn't jump; out
            // stays whatever the caller passed (we own writing it below)
            out.iter_mut().for_each(|s| *s = 0.0);
            return;
        }

        // spatial derivation at block rate — ROOMLOGIC: dog position maps to
        // the shared scene (mic at origin); cross-room dog = occluded dog.
        let az_r = self.beh.pos.1.to_radians();
        let (sx, sy) = (self.beh.pos.0 * az_r.cos(), self.beh.pos.0 * az_r.sin());
        let pr = crate::scene::propagate(sx, sy, self.door.get());
        let pos = PositionState {
            distance_m: pr.eff_distance_m.max(self.beh.pos.0),
            azimuth_deg: self.beh.pos.1,
            facing_deg: 0.0,
        };
        let d = derive_from_position(&pos, 0.8);
        self.derived = d;
        let lp_target = d.lp_hz.min(pr.lp_hz);
        if (lp_target - self.lp_hz).abs() > 20.0 {
            self.lp.set(BiquadKind::LowPass { freq: lp_target, q: 0.6 }, self.sr);
            self.lp_hz = lp_target;
        }
        self.direct_gain.set(d.direct_gain * pr.gain);
        // dog sits "in the room": slightly more room contribution than voice
        self.send_gain = (0.45 * d.room_send * pr.send_mul).clamp(0.3, 1.2);

        let az = self.beh.pos.1.to_radians();
        // subtle physical tilt only — mic pickup, not a panner (±2.5 dB max)
        let tilt = (az.sin() * 0.75).clamp(-0.75, 0.75);
        let gl = 1.0 - tilt.max(0.0);
        let gr = 1.0 + tilt.min(0.0);

        for i in 0..n {
            let mut x = 0.0f32;
            // mix active voices (bounded)
            for v in self.voices.iter_mut() {
                if v.pos < v.len {
                    let b = &self.bank.bufs[v.buf_idx >> 8][v.buf_idx & 0xff];
                    let s = b[v.pos];
                    let mut g = v.gain;
                    if v.pos < v.fade_in {
                        g *= v.pos as f32 / v.fade_in.max(1) as f32;
                    }
                    let rem = v.len - v.pos;
                    if rem < v.fade_out {
                        g *= rem as f32 / v.fade_out.max(1) as f32;
                    }
                    x += s * g;
                    v.pos += 1;
                }
            }
            self.voices.retain(|v| v.pos < v.len);

            let g_total = self.volume.next() * self.duck.next();
            // direct path: pickup LP + distance gain + slight physical tilt
            let dry = self.lp.process(x) * self.direct_gain.next();
            let (wl, wr) = self.room.process(x * self.send_gain);
            let l = dry * gl + wl;
            let r = dry * gr + wr;
            out[2 * i] = l * g_total;
            out[2 * i + 1] = r * g_total;

            self.peak = self.peak.max(l.abs() * g_total).max(r.abs() * g_total);
            self.sum_sq += ((l * l + r * r) * 0.5 * g_total * g_total) as f64;
            self.cnt += 1;
        }
    }

    fn start_voice(&mut self, s: Sched) {
        if self.voices.len() >= MAX_VOICES {
            // steal the oldest
            self.voices.remove(0);
        }
        let kind = ev_idx(s.ev);
        // variant selection avoiding immediate repeats
        let last = self.bank.last_variant[kind];
        let mut var = self.rng.below(VARIANTS as u64) as usize;
        if var == last {
            var = (var + 1) % VARIANTS;
        }
        self.bank.last_variant[kind] = var;
        let len = self.bank.bufs[kind][var].len();
        let fade_in = (0.01 * self.sr) as usize;
        let fade_out = (0.05 * self.sr) as usize;
        // slight per-play variation (bounded, natural)
        let gain = s.gain_mul * self.rng.range(0.85, 1.05);
        self.voices.push(Voice {
            buf_idx: (kind << 8) | var,
            pos: 0,
            gain,
            fade_in: fade_in.min(len / 4),
            fade_out: fade_out.min(len / 3),
            len,
        });
        self.last_event = Some(s.ev);
        self.event_counts[kind] += 1;
    }

    /// Dog bus level for telemetry; clears the accumulator.
    pub fn take_levels(&mut self) -> (f32, f32) {
        let rms = if self.cnt > 0 { (self.sum_sq / self.cnt as f64).sqrt() as f32 } else { 0.0 };
        let p = self.peak;
        self.peak = 0.0;
        self.sum_sq = 0.0;
        self.cnt = 0;
        (crate::envelope::to_db(rms), crate::envelope::to_db(p))
    }

    pub fn debug(&self) -> DogDebug {
        DogDebug {
            enabled: self.cfg.enabled,
            state: self.beh.state,
            distance_m: self.beh.pos.0,
            azimuth_deg: self.beh.pos.1,
            moving: self.beh.target.is_some(),
            stay: self.beh.stay,
            active_voices: self.voices.len(),
            last_event: self.last_event,
            level_db: 0.0,
        }
    }

    /// test hooks
    pub fn state(&self) -> DogState {
        self.beh.state
    }
    pub fn position(&self) -> (f32, f32) {
        self.beh.pos
    }
    pub fn set_seed(&mut self, seed: u64) {
        self.rng = Rng::new(seed);
    }
    pub fn queue_len(&self) -> usize {
        self.beh.queue.len()
    }
    pub fn bark_cooldown(&self) -> f32 {
        self.beh.bark_cooldown
    }
    /// Cumulative events started, per kind (tests/sim analysis).
    pub fn event_count(&self, ev: DogEvent) -> u32 {
        self.event_counts[ev_idx(ev)]
    }
    /// Total events started.
    pub fn total_events(&self) -> u32 {
        self.event_counts.iter().sum()
    }
    /// Is the stay latch engaged?
    pub fn is_stay(&self) -> bool {
        self.beh.stay
    }
}

fn to_xy(p: (f32, f32)) -> (f32, f32) {
    let a = p.1.to_radians();
    (p.0 * a.sin(), p.0 * a.cos())
}
fn from_xy(x: f32, y: f32) -> (f32, f32) {
    let d = (x * x + y * y).sqrt().clamp(0.15, 3.0);
    let az = x.atan2(y).to_degrees().clamp(-80.0, 80.0);
    (d, az)
}

// === tests =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with(cfg: DogConfig, seed: u64) -> DogEngine {
        let mut e = DogEngine::new(48000.0, seed);
        e.configure(&cfg, &RoomspaceParams::default());
        e
    }

    fn cfg(f: impl Fn(&mut DogConfig)) -> DogConfig {
        let mut c = DogConfig { enabled: true, ..Default::default() };
        f(&mut c);
        c
    }

    fn simulate(e: &mut DogEngine, secs: f32, speech: f32) -> [u32; 6] {
        let mut occ = [0u32; 6];
        let dt = 0.02;
        let steps = (secs / dt) as usize;
        let mut buf = vec![0.0f32; 960 * 2];
        for i in 0..steps {
            e.update(dt, speech);
            occ[state_idx(e.state())] += 1;
            if i % 4 == 0 {
                e.render(&mut buf);
            }
        }
        occ
    }

    fn state_idx(s: DogState) -> usize {
        match s {
            DogState::Resting => 0,
            DogState::Wandering => 1,
            DogState::Investigating => 2,
            DogState::Attentive => 3,
            DogState::Vocalizing => 4,
            DogState::Settling => 5,
        }
    }

    #[test]
    fn no_bark_when_disabled_even_playful_frequent() {
        let c = cfg(|c| {
            c.allow_bark = false;
            c.temperament = DogTemperament::Playful;
            c.activity = DogActivity::Frequent;
        });
        let mut e = engine_with(c, 7);
        simulate(&mut e, 1800.0, -80.0);
        assert_eq!(e.event_count(DogEvent::Bark), 0);
        assert_eq!(e.event_count(DogEvent::BarkCluster), 0);
        assert!(e.total_events() > 0, "dog was comatose");
    }

    #[test]
    fn barks_only_when_allowed_and_cooled_down() {
        let c = cfg(|c| {
            c.allow_bark = true;
            c.temperament = DogTemperament::Playful;
            c.activity = DogActivity::Frequent;
        });
        let mut e = engine_with(c, 11);
        simulate(&mut e, 3600.0, -80.0);
        let barks = e.event_count(DogEvent::Bark) + e.event_count(DogEvent::BarkCluster);
        assert!(barks > 0, "no barks in an hour of playful/frequent");
        assert!(barks < 60, "barking out of control: {barks}/h");
    }

    #[test]
    fn resting_dominates_calm_rare() {
        let c = cfg(|c| {
            c.temperament = DogTemperament::Calm;
            c.activity = DogActivity::Rare;
        });
        let mut e = engine_with(c, 3);
        let occ = simulate(&mut e, 3600.0, -80.0);
        let total: u32 = occ.iter().sum();
        let rest = occ[0] + occ[3];
        let frac = rest as f32 / total as f32;
        assert!(frac > 0.6, "calm/rare dog too busy: rest frac {frac:.2}");
    }

    #[test]
    fn frequent_activity_is_not_constant_noise() {
        let c = cfg(|c| {
            c.temperament = DogTemperament::Playful;
            c.activity = DogActivity::Frequent;
        });
        let mut e = engine_with(c, 9);
        simulate(&mut e, 3600.0, -80.0);
        let ev = e.total_events();
        assert!(ev < 900, "{ev} events/h is a soundboard, not a dog");
        assert!(ev > 30, "{ev} events/h is too dead for Frequent");
    }

    #[test]
    fn deterministic_same_seed() {
        let c = cfg(|c| c.activity = DogActivity::Occasional);
        let mut a = engine_with(c.clone(), 42);
        let mut b = engine_with(c, 42);
        for i in 0..5000 {
            a.update(0.02, -80.0);
            b.update(0.02, -80.0);
            let (sa, pa) = (a.state(), a.position());
            let (sb, pb) = (b.state(), b.position());
            assert_eq!(sa, sb, "state diverged at {i}");
            assert!((pa.0 - pb.0).abs() < 1e-5, "pos diverged at {i}");
        }
    }

    #[test]
    fn come_moves_dog_toward_mic() {
        let c = cfg(|_| {});
        let mut e = engine_with(c, 5);
        simulate(&mut e, 5.0, -80.0);
        let d0 = e.position().0;
        e.command(DogCommand::Come);
        simulate(&mut e, 8.0, -80.0);
        let d1 = e.position().0;
        assert!(d1 < d0 || d1 < 0.8, "come didn't approach: {d0} -> {d1}");
        assert!(e.event_count(DogEvent::Step) > 0, "come produced no steps");
    }

    #[test]
    fn settle_goes_to_bed_and_rest() {
        let c = cfg(|_| {});
        let mut e = engine_with(c, 5);
        e.command(DogCommand::Come);
        simulate(&mut e, 6.0, -80.0);
        e.command(DogCommand::Settle);
        let occ = simulate(&mut e, 30.0, -80.0);
        assert_eq!(e.state(), DogState::Resting);
        assert!(occ[5] > 0 || occ[0] > 0, "settle path missing");
        let (d, az) = e.position();
        assert!((d - WAY_BED.0).abs() < 0.4 && (az - WAY_BED.1).abs() < 20.0,
            "dog settled at {d},{az} not the bed {WAY_BED:?}");
    }

    #[test]
    fn stay_holds_position() {
        let c = cfg(|_| {});
        let mut e = engine_with(c, 5);
        simulate(&mut e, 10.0, -80.0);
        e.command(DogCommand::Stay);
        assert!(e.is_stay());
        let p0 = e.position();
        simulate(&mut e, 60.0, -80.0);
        let p1 = e.position();
        assert!((p0.0 - p1.0).abs() < 0.4, "stay drifted: {p0:?} -> {p1:?}");
    }

    #[test]
    fn manual_pos_pins_dog() {
        let c = cfg(|c| c.manual_pos = Some((0.7, 15.0)));
        let mut e = engine_with(c, 5);
        simulate(&mut e, 20.0, -80.0);
        let (d, az) = e.position();
        assert!((d - 0.7).abs() < 0.35 && (az - 15.0).abs() < 15.0,
            "manual pin drifted to {d},{az}");
    }

    #[test]
    fn movement_is_continuous_not_teleporting() {
        let c = cfg(|c| {
            c.temperament = DogTemperament::Curious;
            c.activity = DogActivity::Frequent;
        });
        let mut e = engine_with(c, 13);
        let mut last = e.position();
        for _ in 0..10000 {
            e.update(0.02, -80.0);
            let p = e.position();
            let dd = (p.0 - last.0).abs();
            let daz = (p.1 - last.1).abs();
            assert!(dd < 0.06, "teleport in distance: {dd}");
            assert!(daz < 6.0, "teleport in azimuth: {daz}");
            last = p;
        }
    }

    #[test]
    fn render_is_finite_and_bounded() {
        let c = cfg(|c| {
            c.allow_bark = true;
            c.temperament = DogTemperament::Playful;
            c.activity = DogActivity::Frequent;
            c.volume_db = -6.0;
        });
        let mut e = engine_with(c, 21);
        let mut buf = vec![0.0f32; 480 * 2];
        let mut peak = 0.0f32;
        e.preview();
        for _ in 0..6000 {
            e.update(0.01, -80.0);
            e.render(&mut buf);
            for &s in &buf {
                assert!(s.is_finite(), "non-finite dog sample");
                peak = peak.max(s.abs());
            }
        }
        assert!(peak > 1e-4, "enabled playful dog stayed silent");
        assert!(peak < 4.0, "dog clipped absurdly: {peak}");
    }

    #[test]
    fn nearer_dog_is_louder() {
        let base = cfg(|c| {
            c.volume_db = -6.0;
            c.manual_pos = Some((0.3, 0.0));
        });
        let mut e = engine_with(base.clone(), 7);
        e.preview();
        let mut buf = vec![0.0f32; 480 * 2];
        let mut near_energy = 0.0f64;
        for _ in 0..400 {
            e.update(0.01, -80.0);
            e.render(&mut buf);
            near_energy += buf.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
        }
        let mut far_cfg = base;
        far_cfg.manual_pos = Some((2.5, 0.0));
        e.configure(&far_cfg, &RoomspaceParams::default());
        e.preview();
        let mut far_energy = 0.0f64;
        for _ in 0..400 {
            e.update(0.01, -80.0);
            e.render(&mut buf);
            far_energy += buf.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
        }
        assert!(near_energy > far_energy * 2.0,
            "proximity not audible: near {near_energy:.3} far {far_energy:.3}");
    }

    #[test]
    fn disabled_dog_is_silent() {
        let mut e = DogEngine::new(48000.0, 5);
        let mut buf = vec![0.0f32; 480 * 2];
        for _ in 0..500 {
            e.update(0.02, -80.0);
            e.render(&mut buf);
        }
        assert!(buf.iter().all(|s| s.abs() < 1e-6));
    }

    #[test]
    fn speech_ducking_attenuates() {
        let c = cfg(|c| c.volume_db = -6.0);
        let mut e = engine_with(c, 7);
        e.preview();
        let mut buf = vec![0.0f32; 480 * 2];
        let mut quiet = 0.0f64;
        for _ in 0..300 {
            e.update(0.01, -80.0);
            e.render(&mut buf);
            quiet += buf.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
        }
        e.preview();
        let mut loud = 0.0f64;
        for _ in 0..300 {
            e.update(0.01, -20.0);
            e.render(&mut buf);
            loud += buf.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
        }
        assert!(loud < quiet * 0.5, "ducking weak: quiet {quiet:.3} speech {loud:.3}");
    }

    #[test]
    fn size_profiles_sound_different() {
        let mut out = Vec::new();
        for size in [DogSize::Small, DogSize::Medium, DogSize::Large] {
            let c = cfg(|cc| {
                cc.size = size;
                cc.allow_bark = true;
            });
            let mut e = DogEngine::new(48000.0, 99);
            e.configure(&c, &RoomspaceParams::default());
            e.start_voice(super::Sched { ev: DogEvent::Bark, at_s: 0.0, gain_mul: 1.0 });
            let mut buf = vec![0.0f32; 480 * 2];
            let mut rms_acc = 0.0f64;
            let mut corr_acc = 0.0f64;
            for _ in 0..40 {
                e.render(&mut buf);
                rms_acc += buf.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
                corr_acc += buf.windows(2).map(|w| (w[0] * w[1]) as f64).sum::<f64>();
            }
            out.push((rms_acc as f32, corr_acc as f32));
        }
        assert!((out[0].1 - out[1].1).abs() > 1e-4 || (out[0].0 - out[1].0).abs() > 1e-4,
            "small and medium identical");
        assert!((out[1].1 - out[2].1).abs() > 1e-4 || (out[1].0 - out[2].0).abs() > 1e-4,
            "medium and large identical");
    }
}
