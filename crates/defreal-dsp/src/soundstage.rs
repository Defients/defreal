//! SOUNDSTAGE — independently positioned environmental emitters.
//!
//! Everyday room sounds that live in the ROOMLOGIC scene: each emitter has a
//! metric position, a room, its own scheduling personality, and is captured
//! by the virtual microphone through the same propagation model as the dog
//! and voice — including door occlusion when it sits in the hallway.
//!
//! Provenance: like DefDog, all event sounds are procedurally synthesized
//! (deterministic, documented). Continuous beds (fan/vent/traffic) are
//! filtered-noise textures; events (keys, chair, taps, footsteps) are shaped
//! transients. Not recordings — the MANIFEST says so.

use crate::biquad::{Biquad, BiquadKind};
use crate::motion::PositionState;
use crate::proximity::derive_from_position;
use crate::rng::Rng;
use crate::roomspace::{Roomspace, RoomspaceParams};
use crate::scene::{self, Prop, RoomId};
use crate::smooth::SmoothedParam;
use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

// === categories ================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmitterCat {
    Fan,
    Vent,
    Traffic,
    Keys,
    Chair,
    Desk,
    Footsteps,
}
pub const N_CATS: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmitterActivity {
    Quiet,
    Natural,
    Busy,
}

/// Persistent emitter configuration (scene file / UI).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterCfg {
    pub id: u32,
    pub cat: EmitterCat,
    pub x: f32,
    pub y: f32,
    pub gain_db: f32,
    pub activity: EmitterActivity,
    pub enabled: bool,
    pub muted: bool,
}

impl Default for EmitterCfg {
    fn default() -> Self {
        Self {
            id: 0,
            cat: EmitterCat::Fan,
            x: 0.9,
            y: 0.4,
            gain_db: -30.0,
            activity: EmitterActivity::Natural,
            enabled: true,
            muted: false,
        }
    }
}

pub const MAX_EMITTERS: usize = 8;

// === procedural audio ==========================================================

mod syn {
    use super::*;
    use crate::biquad::{Biquad, BiquadKind};

    pub struct Lp { y: f32, k: f32 }
    impl Lp {
        pub fn new(hz: f32, sr: f32) -> Self {
            Self { y: 0.0, k: 1.0 - (-2.0 * PI * hz / sr).exp() }
        }
        pub fn next(&mut self, w: f32) -> f32 {
            self.y += self.k * (w - self.y);
            self.y
        }
    }

    fn env(t: f32, a: f32, total: f32) -> f32 {
        if t < a { t / a.max(1e-4) } else { (1.0 - (t - a) / (total - a).max(1e-4)).max(0.0) }
    }

    /// one event buffer for a category (48k mono)
    pub fn event(cat: EmitterCat, sr: f32, rng: &mut Rng) -> Vec<f32> {
        match cat {
            EmitterCat::Keys => {
                // 1-3 keystrokes: click + tiny body
                let n_keys = 1 + rng.below(3) as usize;
                let dur = 0.09 * n_keys as f32 + 0.12;
                let n = (dur * sr) as usize;
                let mut out = vec![0.0f32; n];
                let mut lp = Lp::new(4000.0, sr);
                for k in 0..n_keys {
                    let start = (k as f32 * 0.09 * sr) as usize;
                    let len = (0.05 * sr) as usize;
                    let f = rng.range(1800.0, 2600.0);
                    let mut ph = 0.0f32;
                    for i in 0..len.min(n - start) {
                        let t = i as f32 / sr;
                        ph += 2.0 * PI * f / sr;
                        let body = ph.sin() * (-(t * 160.0).exp()) * 0.3;
                        let click = lp.next(rng.gauss()) * (-(t * 400.0).exp()) * 0.8;
                        out[start + i] += (body + click) * env(t, 0.001, 0.05);
                    }
                }
                out
            }
            EmitterCat::Chair => {
                // creak: slow warbling resonant tone + noise
                let dur = rng.range(0.3, 0.7);
                let n = (dur * sr) as usize;
                let mut lp = Lp::new(1500.0, sr);
                let mut bp = Biquad::new(BiquadKind::Peak { freq: rng.range(300.0, 500.0), q: 6.0, gain_db: 12.0 }, sr);
                let mut ph = 0.0f32;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    let e = env(t, 0.08, dur);
                    let wob = rng.range(0.0, 1.0); // baked per-render, cheap
                    let f = 260.0 * (1.0 + 0.15 * (t * 7.0 + wob).sin());
                    ph += 2.0 * PI * f / sr;
                    out.push(bp.process(lp.next(rng.gauss() * 0.4) + ph.sin() * 0.15) * e * 0.5);
                }
                out
            }
            EmitterCat::Desk => {
                // tap/bump: low thump + surface contact noise
                let dur = 0.12;
                let n = (dur * sr) as usize;
                let f = rng.range(90.0, 150.0);
                let mut lp = Lp::new(2500.0, sr);
                let mut ph = 0.0f32;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let t = i as f32 / sr;
                    ph += 2.0 * PI * f / sr;
                    out.push((ph.sin() * (-(t * 22.0).exp()) * 0.8
                        + lp.next(rng.gauss()) * (-(t * 120.0).exp()) * 0.25)
                        * env(t, 0.002, dur));
                }
                out
            }
            EmitterCat::Footsteps => {
                // 2-4 soft steps
                let steps = 2 + rng.below(3) as usize;
                let gap = rng.range(0.4, 0.55);
                let dur = gap * steps as f32 + 0.2;
                let n = (dur * sr) as usize;
                let mut out = vec![0.0f32; n];
                for s in 0..steps {
                    let start = (s as f32 * gap * sr) as usize;
                    let f = rng.range(70.0, 110.0);
                    let len = (0.12 * sr) as usize;
                    let mut ph = 0.0f32;
                    let mut lp = Lp::new(900.0, sr);
                    for i in 0..len.min(n - start) {
                        let t = i as f32 / sr;
                        ph += 2.0 * PI * f / sr;
                        out[start + i] += (ph.sin() * (-(t * 30.0).exp()) * 0.7
                            + lp.next(rng.gauss()) * (-(t * 80.0).exp()) * 0.2)
                            * env(t, 0.004, 0.12)
                            * rng.range(0.8, 1.0);
                    }
                }
                out
            }
            _ => Vec::new(), // continuous cats have no event buffers
        }
    }
}

fn is_continuous(cat: EmitterCat) -> bool {
    matches!(cat, EmitterCat::Fan | EmitterCat::Vent | EmitterCat::Traffic)
}

/// base event rate (events/min) per category at Natural
fn cat_rate(cat: EmitterCat) -> f32 {
    match cat {
        EmitterCat::Keys => 6.0,
        EmitterCat::Chair => 2.0,
        EmitterCat::Desk => 3.0,
        EmitterCat::Footsteps => 1.2,
        _ => 0.0,
    }
}

// === emitter ============================================================

struct Voice {
    buf: usize,   // index into owner's bank
    pos: usize,
    len: usize,
    gain: f32,
}

struct Emitter {
    cfg: EmitterCfg,
    // continuous generator state
    lp: syn::Lp,
    lp2: syn::Lp,
    lfo_ph: f32,
    rumble_ph: f32,
    // event scheduler
    next_event: f32,
    seq_left: u8, // remaining events in a burst sequence (e.g. typing burst)
    seq_gap: f32,
    voices: Vec<Voice>,
    bank: Vec<Vec<f32>>,
    last_var: usize,
}

impl Emitter {
    fn new(cfg: EmitterCfg, sr: f32, rng: &mut Rng) -> Self {
        let mut bank = Vec::new();
        if !is_continuous(cfg.cat) {
            for _ in 0..4 {
                bank.push(syn::event(cfg.cat, sr, rng));
            }
        }
        Self {
            cfg,
            lp: syn::Lp::new(700.0, sr),
            lp2: syn::Lp::new(220.0, sr),
            lfo_ph: rng.f32() * 6.28,
            rumble_ph: rng.f32() * 6.28,
            next_event: rng.range(3.0, 20.0),
            seq_left: 0,
            seq_gap: 0.0,
            voices: Vec::with_capacity(2),
            bank,
            last_var: usize::MAX,
        }
    }

    fn rebuild_bank(&mut self, sr: f32, rng: &mut Rng) {
        self.bank.clear();
        if !is_continuous(self.cfg.cat) {
            for _ in 0..4 {
                self.bank.push(syn::event(self.cfg.cat, sr, rng));
            }
        }
        self.voices.clear();
    }

    fn act_scale(a: EmitterActivity) -> f32 {
        match a {
            EmitterActivity::Quiet => 0.35,
            EmitterActivity::Natural => 1.0,
            EmitterActivity::Busy => 2.6,
        }
    }

    /// continuous texture per-sample (called at sample rate only when audible)
    #[inline]
    fn continuous(&mut self, rng: &mut Rng, sr: f32) -> f32 {
        self.lfo_ph += 2.0 * PI * 0.11 / sr;
        if self.lfo_ph > 6.283 { self.lfo_ph -= 6.283; }
        match self.cfg.cat {
            EmitterCat::Fan => {
                // steady mech airflow + slow amplitude wobble + faint hum
                let wob = 1.0 + self.lfo_ph.sin() * 0.12;
                self.rumble_ph += 2.0 * PI * 120.0 / sr;
                let hum = self.rumble_ph.sin() * 0.05;
                (self.lp.next(rng.gauss() * 0.35) + hum) * wob * 0.35
            }
            EmitterCat::Vent => {
                let wob = 1.0 + self.lfo_ph.sin() * 0.2;
                self.lp.next(rng.gauss() * 0.4) * wob * 0.3
            }
            EmitterCat::Traffic => {
                // low exterior rumble with slow pass swells
                self.rumble_ph += 2.0 * PI * 0.05 / sr;
                if self.rumble_ph > 6.283 { self.rumble_ph -= 6.283; }
                let swell = (self.rumble_ph.sin() * 0.5 + 0.5).powi(2);
                (self.lp2.next(rng.gauss() * 0.5) * 0.5
                    + self.lp.next(rng.gauss() * 0.15) * swell)
                    * 0.4
            }
            _ => 0.0,
        }
    }

    fn start_event(&mut self, rng: &mut Rng) {
        if self.bank.is_empty() || self.voices.len() >= 2 {
            return;
        }
        let mut v = rng.below(self.bank.len() as u64) as usize;
        if v == self.last_var {
            v = (v + 1) % self.bank.len();
        }
        self.last_var = v;
        let len = self.bank[v].len();
        self.voices.push(Voice { buf: v, pos: 0, len, gain: rng.range(0.8, 1.0) });
    }
}

// === soundstage engine =========================================================

pub struct EmitterInfo {
    pub id: u32,
    pub cat: EmitterCat,
    pub x: f32,
    pub y: f32,
    pub room: RoomId,
    pub active: bool,
    pub level_db: f32,
}

pub struct SoundStage {
    emitters: Vec<Emitter>,
    rng: Rng,
    next_id: u32,
    door: SmoothedParam,   // 0..1 door openness
    enabled: bool,
    master_gain: SmoothedParam,
    // per-room spatial paths
    room_a: Roomspace,
    room_b: Roomspace,
    lp: [Biquad; 4],      // per-emitter prop LP (indexed by emitter idx, capped)
    lp_hz: [f32; 4],
    sr: f32,
    // meters
    sum_sq: f64,
    peak: f32,
    cnt: u32,
    event_counts: [u32; N_CATS],
}

impl SoundStage {
    pub fn new(sr: f32, seed: u64) -> Self {
        Self {
            emitters: Vec::new(),
            rng: Rng::new(seed ^ 0x5CE7E),
            next_id: 1,
            door: SmoothedParam::new(1.0, 500.0, sr),
            enabled: false,
            master_gain: SmoothedParam::new(1.0, 300.0, sr),
            room_a: Roomspace::new(sr),
            room_b: Roomspace::new(sr),
            lp: [
                Biquad::new(BiquadKind::LowPass { freq: 17000.0, q: 0.6 }, sr),
                Biquad::new(BiquadKind::LowPass { freq: 17000.0, q: 0.6 }, sr),
                Biquad::new(BiquadKind::LowPass { freq: 17000.0, q: 0.6 }, sr),
                Biquad::new(BiquadKind::LowPass { freq: 17000.0, q: 0.6 }, sr),
            ],
            lp_hz: [17000.0; 4],
            sr,
            sum_sq: 0.0,
            peak: 0.0,
            cnt: 0,
            event_counts: [0; N_CATS],
        }
    }

    pub fn set_enabled(&mut self, en: bool) {
        self.enabled = en;
        self.master_gain.set(if en { 1.0 } else { 0.0 });
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_door(&mut self, open: f32) {
        self.door.set(open.clamp(0.0, 1.0));
    }
    pub fn door(&self) -> f32 {
        self.door.get()
    }
    pub fn sync_rooms(&mut self, a: &RoomspaceParams, b: &RoomspaceParams) {
        self.room_a.update(a);
        self.room_b.update(b);
    }

    pub fn add(&mut self, mut cfg: EmitterCfg) -> Option<u32> {
        if self.emitters.len() >= MAX_EMITTERS {
            return None;
        }
        // caller may supply an id (persisted scenes); else auto-assign
        let id = if cfg.id != 0 { cfg.id } else { self.next_id };
        self.next_id = self.next_id.max(id + 1);
        cfg.id = id;
        let (x, y) = scene::clamp_pos(cfg.x, cfg.y);
        cfg.x = x;
        cfg.y = y;
        self.emitters.push(Emitter::new(cfg, self.sr, &mut self.rng));
        Some(id)
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let n = self.emitters.len();
        self.emitters.retain(|e| e.cfg.id != id);
        self.emitters.len() != n
    }

    pub fn emitter_mut(&mut self, id: u32, f: impl FnOnce(&mut EmitterCfg)) -> bool {
        if let Some(e) = self.emitters.iter_mut().find(|e| e.cfg.id == id) {
            let mut c = e.cfg.clone();
            f(&mut c);
            let (x, y) = scene::clamp_pos(c.x, c.y);
            c.x = x;
            c.y = y;
            let cat_changed = c.cat != e.cfg.cat;
            e.cfg = c;
            if cat_changed {
                let mut r = Rng::new(self.rng.next_u64());
                e.rebuild_bank(self.sr, &mut r);
            }
            true
        } else {
            false
        }
    }

    pub fn info(&self) -> Vec<EmitterInfo> {
        self.emitters
            .iter()
            .map(|e| EmitterInfo {
                id: e.cfg.id,
                cat: e.cfg.cat,
                x: e.cfg.x,
                y: e.cfg.y,
                room: scene::room_of(e.cfg.x),
                active: !e.voices.is_empty() || is_continuous(e.cfg.cat),
                level_db: 0.0,
            })
            .collect()
    }

    /// Control-rate scheduling. dt seconds.
    pub fn update(&mut self, dt: f32) {
        if !self.enabled {
            return;
        }
        for e in self.emitters.iter_mut() {
            if !e.cfg.enabled || e.cfg.muted || is_continuous(e.cfg.cat) {
                continue;
            }
            // burst sequence continuation
            if e.seq_left > 0 {
                e.seq_gap -= dt;
                if e.seq_gap <= 0.0 {
                    e.seq_left -= 1;
                    e.start_event(&mut self.rng);
                    e.seq_gap = self.rng.range(0.12, 0.45);
                }
                continue;
            }
            e.next_event -= dt * Emitter::act_scale(e.cfg.activity);
            if e.next_event <= 0.0 {
                let rate = cat_rate(e.cfg.cat); // events/min at Natural
                e.next_event = 60.0 / rate * self.rng.range(0.5, 1.8);
                // bursts: keys type in sessions, footsteps come in walks
                e.seq_left = match e.cfg.cat {
                    EmitterCat::Keys => self.rng.below(6) as u8,
                    EmitterCat::Footsteps => self.rng.below(2) as u8,
                    _ => 0,
                };
                e.seq_gap = 0.0;
                e.start_event(&mut self.rng);
                self.event_counts[cat_idx(e.cfg.cat)] += 1;
            }
        }
    }

    /// Render the soundstage bus into interleaved stereo `out` (length 2n).
    /// Real-time safe: no allocation, bounded voices, preallocated state.
    pub fn render(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        if !self.enabled {
            out.iter_mut().for_each(|s| *s = 0.0);
            return;
        }
        let door = self.door.next_block(n);
        for i in 0..n {
            let mut l = 0.0f32;
            let mut r = 0.0f32;
            for (ei, e) in self.emitters.iter_mut().enumerate() {
                if !e.cfg.enabled || e.cfg.muted {
                    continue;
                }
                let li = ei.min(3);
                // raw source sample
                let mut x = e.continuous(&mut self.rng, self.sr);
                for v in e.voices.iter_mut() {
                    if v.pos < v.len {
                        x += e.bank[v.buf][v.pos] * v.gain;
                        v.pos += 1;
                    }
                }
                if x.abs() < 1e-7 {
                    e.voices.retain(|v| v.pos < v.len);
                    continue;
                }
                e.voices.retain(|v| v.pos < v.len);

                // propagation: same-room direct vs cross-room through door
                let pr: Prop = scene::propagate(e.cfg.x, e.cfg.y, door);
                let pos = PositionState {
                    distance_m: pr.eff_distance_m,
                    azimuth_deg: (e.cfg.y).atan2(e.cfg.x.abs().max(0.05)).to_degrees().clamp(-80.0, 80.0),
                    facing_deg: 0.0,
                };
                let der = derive_from_position(&pos, 0.8);
                let lp_target = der.lp_hz.min(pr.lp_hz);
                if (lp_target - self.lp_hz[li]).abs() > 25.0 {
                    self.lp[li].set(BiquadKind::LowPass { freq: lp_target, q: 0.6 }, self.sr);
                    self.lp_hz[li] = lp_target;
                }
                let direct = self.lp[li].process(x) * der.direct_gain * pr.gain;
                // slight physical tilt like the dog path (mic pickup, not pan)
                let tilt = (pos.azimuth_deg.to_radians().sin() * 0.6).clamp(-0.6, 0.6);
                let send = x * 0.4 * der.room_send * pr.send_mul;
                let (wl, wr) = if scene::room_of(e.cfg.x) == RoomId::Studio {
                    self.room_a.process(send)
                } else {
                    self.room_b.process(send)
                };
                let g = crate::envelope::from_db(e.cfg.gain_db.clamp(-48.0, 0.0))
                    * self.master_gain.next();
                l += (direct * (1.0 - tilt.max(0.0)) + wl) * g;
                r += (direct * (1.0 + tilt.min(0.0)) + wr) * g;
            }
            out[2 * i] = l;
            out[2 * i + 1] = r;
            self.peak = self.peak.max(l.abs()).max(r.abs());
            self.sum_sq += ((l * l + r * r) * 0.5) as f64;
            self.cnt += 1;
        }
    }

    pub fn take_levels(&mut self) -> (f32, f32) {
        let rms = if self.cnt > 0 { (self.sum_sq / self.cnt as f64).sqrt() as f32 } else { 0.0 };
        let p = self.peak;
        self.peak = 0.0;
        self.sum_sq = 0.0;
        self.cnt = 0;
        (crate::envelope::to_db(rms), crate::envelope::to_db(p))
    }

    pub fn event_count(&self, cat: EmitterCat) -> u32 {
        self.event_counts[cat_idx(cat)]
    }
    pub fn emitter_count(&self) -> usize {
        self.emitters.len()
    }
    /// test hook: force an event
    pub fn force_event(&mut self, id: u32) {
        if let Some(e) = self.emitters.iter_mut().find(|e| e.cfg.id == id) {
            e.start_event(&mut self.rng);
        }
    }
}

fn cat_idx(c: EmitterCat) -> usize {
    match c {
        EmitterCat::Fan => 0,
        EmitterCat::Vent => 1,
        EmitterCat::Traffic => 2,
        EmitterCat::Keys => 3,
        EmitterCat::Chair => 4,
        EmitterCat::Desk => 5,
        EmitterCat::Footsteps => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / v.len().max(1) as f64).sqrt() as f32
    }

    fn drive(st: &mut SoundStage, secs: f32, block: usize) -> f32 {
        let mut acc = Vec::new();
        let mut buf = vec![0.0f32; block * 2];
        let n_blocks = (secs * 48000.0) as usize / block;
        for _ in 0..n_blocks {
            st.update(block as f32 / 48000.0);
            st.render(&mut buf);
            acc.extend_from_slice(&buf);
        }
        rms(&acc)
    }

    #[test]
    fn disabled_stage_is_silent() {
        let mut st = SoundStage::new(48000.0, 7);
        let id = st.add(EmitterCfg { cat: EmitterCat::Fan, ..Default::default() }).unwrap();
        assert!(id > 0);
        assert!(drive(&mut st, 2.0, 240) < 1e-6);
    }

    #[test]
    fn fan_produces_continuous_audio() {
        let mut st = SoundStage::new(48000.0, 7);
        st.add(EmitterCfg { cat: EmitterCat::Fan, gain_db: -18.0, ..Default::default() });
        st.set_enabled(true);
        let r = drive(&mut st, 1.0, 240);
        assert!(r > 1e-4, "fan silent: {r}");
    }

    #[test]
    fn closed_door_muffles_hallway_source() {
        let run = |door: f32| {
            let mut st = SoundStage::new(48000.0, 7);
            st.add(EmitterCfg { cat: EmitterCat::Fan, x: 2.2, y: 0.2, gain_db: -12.0, ..Default::default() });
            st.set_enabled(true);
            st.set_door(door);
            drive(&mut st, 3.0, 240) // door smooths over ~500ms; measure steady state
        };
        let open = run(1.0);
        let closed = run(0.0);
        assert!(closed < open * 0.7, "door inaudible: open {open} closed {closed}");
        assert!(closed > 1e-5, "closed door should muffle, not silence");
    }

    #[test]
    fn same_room_source_ignores_door() {
        let run = |door: f32| {
            let mut st = SoundStage::new(48000.0, 7);
            st.add(EmitterCfg { cat: EmitterCat::Fan, x: 0.8, y: 0.3, gain_db: -12.0, ..Default::default() });
            st.set_enabled(true);
            st.set_door(door);
            drive(&mut st, 2.5, 240)
        };
        let open = run(1.0);
        let closed = run(0.0);
        assert!((open - closed).abs() < open * 0.15, "same-room fan changed by door: {open} vs {closed}");
    }

    #[test]
    fn keys_are_bursts_not_constant() {
        let mut st = SoundStage::new(48000.0, 11);
        st.add(EmitterCfg { cat: EmitterCat::Keys, gain_db: -18.0, activity: EmitterActivity::Natural, ..Default::default() });
        st.set_enabled(true);
        let mut buf = vec![0.0f32; 480 * 2];
        let mut voiced = 0u32;
        // 10 simulated minutes
        for _ in 0..(600 * 100) {
            st.update(0.01);
            st.render(&mut buf);
            if rms(&buf) > 1e-4 {
                voiced += 1;
            }
        }
        let frac = voiced as f32 / (600.0 * 100.0);
        assert!(frac < 0.25, "keyboard too busy: {frac}");
        assert!(st.event_count(EmitterCat::Keys) > 10, "no keyboard events fired");
    }

    #[test]
    fn deterministic_same_seed() {
        let run = || {
            let mut st = SoundStage::new(48000.0, 42);
            st.add(EmitterCfg { cat: EmitterCat::Chair, gain_db: -15.0, ..Default::default() });
            st.set_enabled(true);
            drive(&mut st, 4.0, 240)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn output_is_finite_and_bounded() {
        let mut st = SoundStage::new(48000.0, 5);
        for cat in [EmitterCat::Fan, EmitterCat::Keys, EmitterCat::Desk, EmitterCat::Footsteps] {
            st.add(EmitterCfg { cat, gain_db: -12.0, activity: EmitterActivity::Busy, ..Default::default() });
        }
        st.set_enabled(true);
        let mut buf = vec![0.0f32; 480];
        for _ in 0..5000 {
            st.update(0.005);
            st.render(&mut buf);
        }
        assert!(buf.iter().all(|s| s.is_finite() && s.abs() < 4.0));
    }

    #[test]
    fn emitter_cap_enforced() {
        let mut st = SoundStage::new(48000.0, 1);
        for _ in 0..MAX_EMITTERS {
            assert!(st.add(EmitterCfg::default()).is_some());
        }
        assert!(st.add(EmitterCfg::default()).is_none());
    }
}
