//! Lock-free telemetry: the processing thread publishes, the UI polls.
//! All atomics; no locks on any audio-adjacent path.

use defreal_dsp::motion::MotionState;
use defreal_dsp::EngineMetrics;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineState {
    Idle,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl EngineState {
    fn as_u8(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Starting => 1,
            Self::Running => 2,
            Self::Stopping => 3,
            Self::Failed => 4,
        }
    }
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::Stopping,
            4 => Self::Failed,
            _ => Self::Idle,
        }
    }
}

pub struct SharedTelemetry {
    state: AtomicU8,
    pub error_code: AtomicU8, // 0 none; >0 = engine-side error
    // meters (f32 bit patterns)
    in_rms_db: AtomicF32,
    in_peak_db: AtomicF32,
    out_rms_db: AtomicF32,
    out_peak_db: AtomicF32,
    limiter_gain_db: AtomicF32,
    speech_level: AtomicF32,
    speech_open: AtomicBool,
    mic_comp_db: AtomicF32,
    hum_comp_db: AtomicF32,
    // buffer health
    pub input_overflows: AtomicU64,
    pub output_underflows: AtomicU64,
    pub input_blocks: AtomicU64,
    pub output_blocks: AtomicU64,
    in_fill: AtomicF32,   // 0..1 occupancy
    out_fill: AtomicF32,
    resample_ppm: AtomicF32,
    proc_avg_us: AtomicF32,
    proc_max_us: AtomicF32,
    latency_ms: AtomicF32,
    // motion
    motion_state: AtomicU8,
    motion_distance: AtomicF32,
    motion_target: AtomicF32,
    motion_azimuth: AtomicF32,
    motion_facing: AtomicF32,
    motion_target_azimuth: AtomicF32,
    motion_target_facing: AtomicF32,
    motion_progress: AtomicF32,
    motion_age: AtomicF32,
    motion_manual: AtomicBool,
    motion_pending: AtomicU8,
    // stream config actually in use
    in_rate: AtomicF32,
    out_rate: AtomicF32,
    in_channels: AtomicU8,
    out_channels: AtomicU8,
    // monitor (second output branch)
    mon_state: AtomicU8,
    mon_fill: AtomicF32,
    pub mon_underflows: AtomicU64,
    mon_rms_db: AtomicF32,
    mon_peak_db: AtomicF32,
    mon_gain_db: AtomicF32,
    mon_muted: AtomicBool,
    mon_rate: AtomicF32,
    mon_channels: AtomicU8,
    /// device name + last error; UI-thread only, never touched in RT callbacks
    pub mon_meta: std::sync::Mutex<MonitorMeta>,
    // defdog (ambient canine source)
    dog_enabled: AtomicBool,
    dog_state: AtomicU8,
    dog_distance: AtomicF32,
    dog_azimuth: AtomicF32,
    dog_moving: AtomicBool,
    dog_rms_db: AtomicF32,
    dog_peak_db: AtomicF32,
    dog_last_event: AtomicU8,
}

/// Stringy monitor info that can't live in atomics.
#[derive(Clone, Debug, Default)]
pub struct MonitorMeta {
    pub device: String,
    pub error: Option<String>,
}

/// Lifecycle of the independent monitor branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorState {
    Disabled = 0,
    Opening = 1,
    Active = 2,
    Error = 3,
}

impl MonitorState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Opening => "opening",
            Self::Active => "active",
            Self::Error => "error",
        }
    }
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Opening,
            2 => Self::Active,
            3 => Self::Error,
            _ => Self::Disabled,
        }
    }
}

struct AtomicF32(AtomicU64);
impl AtomicF32 {
    fn store(&self, v: f32) {
        self.0.store(v.to_bits() as u64, Ordering::Relaxed);
    }
    fn load(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed) as u32)
    }
}

impl SharedTelemetry {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(EngineState::Idle.as_u8()),
            error_code: AtomicU8::new(0),
            in_rms_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            in_peak_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            out_rms_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            out_peak_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            limiter_gain_db: AtomicF32(AtomicU64::new(0f32.to_bits() as u64)),
            speech_level: AtomicF32(AtomicU64::new(0)),
            speech_open: AtomicBool::new(false),
            mic_comp_db: AtomicF32(AtomicU64::new(0)),
            hum_comp_db: AtomicF32(AtomicU64::new(0)),
            input_overflows: AtomicU64::new(0),
            output_underflows: AtomicU64::new(0),
            input_blocks: AtomicU64::new(0),
            output_blocks: AtomicU64::new(0),
            in_fill: AtomicF32(AtomicU64::new(0)),
            out_fill: AtomicF32(AtomicU64::new(0)),
            resample_ppm: AtomicF32(AtomicU64::new(0)),
            proc_avg_us: AtomicF32(AtomicU64::new(0)),
            proc_max_us: AtomicF32(AtomicU64::new(0)),
            latency_ms: AtomicF32(AtomicU64::new(0)),
            motion_state: AtomicU8::new(0),
            motion_distance: AtomicF32(AtomicU64::new(0.32f32.to_bits() as u64)),
            motion_target: AtomicF32(AtomicU64::new(0.32f32.to_bits() as u64)),
            motion_azimuth: AtomicF32(AtomicU64::new(0)),
            motion_facing: AtomicF32(AtomicU64::new(0)),
            motion_target_azimuth: AtomicF32(AtomicU64::new(0)),
            motion_target_facing: AtomicF32(AtomicU64::new(0)),
            motion_progress: AtomicF32(AtomicU64::new((1.0f32).to_bits() as u64)),
            motion_age: AtomicF32(AtomicU64::new(0)),
            motion_manual: AtomicBool::new(false),
            motion_pending: AtomicU8::new(255),
            in_rate: AtomicF32(AtomicU64::new(0)),
            out_rate: AtomicF32(AtomicU64::new(0)),
            in_channels: AtomicU8::new(0),
            out_channels: AtomicU8::new(0),
            mon_state: AtomicU8::new(MonitorState::Disabled as u8),
            mon_fill: AtomicF32(AtomicU64::new(0)),
            mon_underflows: AtomicU64::new(0),
            mon_rms_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            mon_peak_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            mon_gain_db: AtomicF32(AtomicU64::new((-18.0f32).to_bits() as u64)),
            mon_muted: AtomicBool::new(false),
            mon_rate: AtomicF32(AtomicU64::new(0)),
            mon_channels: AtomicU8::new(0),
            mon_meta: std::sync::Mutex::new(MonitorMeta::default()),
            dog_enabled: AtomicBool::new(false),
            dog_state: AtomicU8::new(0),
            dog_distance: AtomicF32(AtomicU64::new((2.2f32).to_bits() as u64)),
            dog_azimuth: AtomicF32(AtomicU64::new(0)),
            dog_moving: AtomicBool::new(false),
            dog_rms_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            dog_peak_db: AtomicF32(AtomicU64::new((-100.0f32).to_bits() as u64)),
            dog_last_event: AtomicU8::new(255),
        }
    }

    pub fn set_state(&self, s: EngineState) {
        self.state.store(s.as_u8(), Ordering::Relaxed);
    }
    pub fn state(&self) -> EngineState {
        EngineState::from_u8(self.state.load(Ordering::Relaxed))
    }

    pub fn set_stream_info(&self, in_rate: f32, out_rate: f32, in_ch: u8, out_ch: u8) {
        self.in_rate.store(in_rate);
        self.out_rate.store(out_rate);
        self.in_channels.store(in_ch, Ordering::Relaxed);
        self.out_channels.store(out_ch, Ordering::Relaxed);
    }

    /// Publish one block's worth of state from the processing thread.
    pub fn publish_block(&self, m: &EngineMetrics, dbg: &defreal_dsp::MotionDebug, buf: &BufferStats, proc_us: f32) {
        self.in_rms_db.store(m.in_rms_db);
        self.in_peak_db.store(m.in_peak_db);
        self.out_rms_db.store(m.out_rms_db);
        self.out_peak_db.store(m.out_peak_db);
        self.limiter_gain_db.store(m.limiter_gain_db);
        self.speech_level.store(m.speech_level);
        self.speech_open.store(m.speech_open, Ordering::Relaxed);
        self.mic_comp_db.store(m.mic_comp_db);
        self.hum_comp_db.store(m.hum_comp_db);
        self.in_fill.store(buf.in_fill);
        self.out_fill.store(buf.out_fill);
        self.resample_ppm.store(buf.drift_ppm);
        self.latency_ms.store(buf.latency_ms);
        // exponential average for proc time
        let prev = self.proc_avg_us.load();
        self.proc_avg_us.store(if prev == 0.0 { proc_us } else { prev * 0.95 + proc_us * 0.05 });
        if proc_us > self.proc_max_us.load() {
            self.proc_max_us.store(proc_us);
        }
        self.motion_state.store(motion_state_u8(dbg.state), Ordering::Relaxed);
        self.motion_distance.store(dbg.distance_m);
        self.motion_target.store(dbg.target_distance_m);
        self.motion_azimuth.store(dbg.azimuth_deg);
        self.motion_facing.store(dbg.facing_deg);
        self.motion_target_azimuth.store(dbg.target_azimuth_deg);
        self.motion_target_facing.store(dbg.target_facing_deg);
        self.motion_progress.store(dbg.transition_progress);
        self.motion_age.store(dbg.seconds_since_transition);
        self.motion_manual.store(dbg.manual, Ordering::Relaxed);
        self.motion_pending.store(dbg.pending.map(motion_state_u8).unwrap_or(255), Ordering::Relaxed);
    }

    /// Per-tick dog stats (processing thread only).
    pub fn publish_dog(&self, d: &defreal_dsp::DogDebug, rms_db: f32, peak_db: f32) {
        self.dog_enabled.store(d.enabled, Ordering::Relaxed);
        self.dog_state.store(dog_state_u8(d.state), Ordering::Relaxed);
        self.dog_distance.store(d.distance_m);
        self.dog_azimuth.store(d.azimuth_deg);
        self.dog_moving.store(d.moving, Ordering::Relaxed);
        self.dog_rms_db.store(rms_db);
        self.dog_peak_db.store(peak_db);
        self.dog_last_event
            .store(d.last_event.map(dog_event_u8).unwrap_or(255), Ordering::Relaxed);
    }

    /// Snapshot for the UI.
    pub fn snapshot(&self, error: Option<String>) -> TelemetrySnapshot {
        TelemetrySnapshot {
            state: self.state(),
            error,
            in_rms_db: self.in_rms_db.load(),
            in_peak_db: self.in_peak_db.load(),
            out_rms_db: self.out_rms_db.load(),
            out_peak_db: self.out_peak_db.load(),
            limiter_gain_db: self.limiter_gain_db.load(),
            speech_level: self.speech_level.load(),
            speech_open: self.speech_open.load(Ordering::Relaxed),
            mic_comp_db: self.mic_comp_db.load(),
            hum_comp_db: self.hum_comp_db.load(),
            input_overflows: self.input_overflows.load(Ordering::Relaxed),
            output_underflows: self.output_underflows.load(Ordering::Relaxed),
            in_fill: self.in_fill.load(),
            out_fill: self.out_fill.load(),
            resample_drift_ppm: self.resample_ppm.load(),
            proc_avg_us: self.proc_avg_us.load(),
            proc_max_us: self.proc_max_us.load(),
            est_latency_ms: self.latency_ms.load(),
            motion_state: motion_state_name(self.motion_state.load(Ordering::Relaxed)),
            motion_distance_m: self.motion_distance.load(),
            motion_target_m: self.motion_target.load(),
            motion_azimuth_deg: self.motion_azimuth.load(),
            motion_facing_deg: self.motion_facing.load(),
            motion_target_azimuth_deg: self.motion_target_azimuth.load(),
            motion_target_facing_deg: self.motion_target_facing.load(),
            motion_progress: self.motion_progress.load(),
            motion_age_s: self.motion_age.load(),
            motion_manual: self.motion_manual.load(Ordering::Relaxed),
            motion_pending: match self.motion_pending.load(Ordering::Relaxed) {
                255 => None,
                v => Some(motion_state_name(v).to_string()),
            },
            in_rate: self.in_rate.load(),
            out_rate: self.out_rate.load(),
            in_channels: self.in_channels.load(Ordering::Relaxed),
            out_channels: self.out_channels.load(Ordering::Relaxed),
            monitor_state: MonitorState::from_u8(self.mon_state.load(Ordering::Relaxed))
                .name(),
            monitor_fill: self.mon_fill.load(),
            monitor_underflows: self.mon_underflows.load(Ordering::Relaxed),
            monitor_rms_db: self.mon_rms_db.load(),
            monitor_peak_db: self.mon_peak_db.load(),
            monitor_gain_db: self.mon_gain_db.load(),
            monitor_muted: self.mon_muted.load(Ordering::Relaxed),
            monitor_rate: self.mon_rate.load(),
            monitor_channels: self.mon_channels.load(Ordering::Relaxed),
            monitor_device: self
                .mon_meta
                .lock()
                .map(|m| m.device.clone())
                .unwrap_or_default(),
            monitor_error: self.mon_meta.lock().ok().and_then(|m| m.error.clone()),
            dog_enabled: self.dog_enabled.load(Ordering::Relaxed),
            dog_state: dog_state_name(self.dog_state.load(Ordering::Relaxed)),
            dog_distance_m: self.dog_distance.load(),
            dog_azimuth_deg: self.dog_azimuth.load(),
            dog_moving: self.dog_moving.load(Ordering::Relaxed),
            dog_rms_db: self.dog_rms_db.load(),
            dog_peak_db: self.dog_peak_db.load(),
            dog_last_event: match self.dog_last_event.load(Ordering::Relaxed) {
                255 => None,
                v => Some(dog_event_name(v).to_string()),
            },
        }
    }

    pub fn set_monitor_state(&self, s: MonitorState) {
        self.mon_state.store(s as u8, Ordering::Relaxed);
    }
    pub fn monitor_state(&self) -> MonitorState {
        MonitorState::from_u8(self.mon_state.load(Ordering::Relaxed))
    }
    /// Per-tick monitor branch stats (processing thread only).
    pub fn publish_monitor(&self, fill: f32, rms_db: f32, peak_db: f32, rate: f32, ch: u8) {
        self.mon_fill.store(fill);
        self.mon_rms_db.store(rms_db);
        self.mon_peak_db.store(peak_db);
        self.mon_rate.store(rate);
        self.mon_channels.store(ch, Ordering::Relaxed);
    }
    pub fn set_monitor_gain(&self, db: f32, muted: bool) {
        self.mon_gain_db.store(db);
        self.mon_muted.store(muted, Ordering::Relaxed);
    }
    /// Record a monitor open/device error for the UI (non-RT path only).
    pub fn set_monitor_error(&self, device: String, error: Option<String>) {
        if let Ok(mut m) = self.mon_meta.lock() {
            m.device = device;
            m.error = error;
        }
    }
    pub fn set_monitor_device(&self, device: String) {
        if let Ok(mut m) = self.mon_meta.lock() {
            m.device = device;
            m.error = None;
        }
    }

    /// Reset max-hold stats (diagnostics "reset counters").
    pub fn reset_counters(&self) {
        self.input_overflows.store(0, Ordering::Relaxed);
        self.output_underflows.store(0, Ordering::Relaxed);
        self.proc_max_us.store(0.0);
    }
}

pub struct BufferStats {
    pub in_fill: f32,
    pub out_fill: f32,
    pub drift_ppm: f32,
    pub latency_ms: f32,
}

fn motion_state_u8(s: MotionState) -> u8 {
    match s {
        MotionState::Neutral => 0,
        MotionState::LeanIn => 1,
        MotionState::LeanBack => 2,
        MotionState::TurnAside => 3,
        MotionState::Reposition => 4,
        MotionState::ReturnToNeutral => 5,
    }
}

fn dog_state_u8(s: defreal_dsp::DogState) -> u8 {
    match s {
        defreal_dsp::DogState::Resting => 0,
        defreal_dsp::DogState::Wandering => 1,
        defreal_dsp::DogState::Investigating => 2,
        defreal_dsp::DogState::Attentive => 3,
        defreal_dsp::DogState::Vocalizing => 4,
        defreal_dsp::DogState::Settling => 5,
    }
}
fn dog_state_name(v: u8) -> &'static str {
    match v {
        1 => "wandering",
        2 => "investigating",
        3 => "attentive",
        4 => "vocalizing",
        5 => "settling",
        _ => "resting",
    }
}
fn dog_event_u8(e: defreal_dsp::DogEvent) -> u8 {
    match e {
        defreal_dsp::DogEvent::Breath => 0,
        defreal_dsp::DogEvent::Pant => 1,
        defreal_dsp::DogEvent::Sniff => 2,
        defreal_dsp::DogEvent::Whine => 3,
        defreal_dsp::DogEvent::Sigh => 4,
        defreal_dsp::DogEvent::Step => 5,
        defreal_dsp::DogEvent::Collar => 6,
        defreal_dsp::DogEvent::SettleDown => 7,
        defreal_dsp::DogEvent::Move => 8,
        defreal_dsp::DogEvent::Bark => 9,
        defreal_dsp::DogEvent::BarkCluster => 10,
    }
}
fn dog_event_name(v: u8) -> &'static str {
    match v {
        0 => "breath",
        1 => "pant",
        2 => "sniff",
        3 => "whine",
        4 => "sigh",
        5 => "step",
        6 => "collar",
        7 => "settle",
        8 => "move",
        9 => "bark",
        10 => "bark cluster",
        _ => "?",
    }
}

fn motion_state_name(v: u8) -> &'static str {
    match v {
        1 => "Lean In",
        2 => "Lean Back",
        3 => "Turn Aside",
        4 => "Reposition",
        5 => "Return to Neutral",
        _ => "Neutral",
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TelemetrySnapshot {
    pub state: EngineState,
    pub error: Option<String>,
    pub in_rms_db: f32,
    pub in_peak_db: f32,
    pub out_rms_db: f32,
    pub out_peak_db: f32,
    pub limiter_gain_db: f32,
    pub speech_level: f32,
    pub speech_open: bool,
    pub mic_comp_db: f32,
    pub hum_comp_db: f32,
    pub input_overflows: u64,
    pub output_underflows: u64,
    pub in_fill: f32,
    pub out_fill: f32,
    pub resample_drift_ppm: f32,
    pub proc_avg_us: f32,
    pub proc_max_us: f32,
    pub est_latency_ms: f32,
    pub motion_state: &'static str,
    pub motion_distance_m: f32,
    pub motion_target_m: f32,
    pub motion_azimuth_deg: f32,
    pub motion_facing_deg: f32,
    pub motion_target_azimuth_deg: f32,
    pub motion_target_facing_deg: f32,
    pub motion_progress: f32,
    pub motion_age_s: f32,
    pub motion_manual: bool,
    pub motion_pending: Option<String>,
    pub in_rate: f32,
    pub out_rate: f32,
    pub in_channels: u8,
    pub out_channels: u8,
    // monitor (second output) branch
    pub monitor_state: &'static str,
    pub monitor_fill: f32,
    pub monitor_underflows: u64,
    pub monitor_rms_db: f32,
    pub monitor_peak_db: f32,
    pub monitor_gain_db: f32,
    pub monitor_muted: bool,
    pub monitor_rate: f32,
    pub monitor_channels: u8,
    pub monitor_device: String,
    pub monitor_error: Option<String>,
    // defdog
    pub dog_enabled: bool,
    pub dog_state: &'static str,
    pub dog_distance_m: f32,
    pub dog_azimuth_deg: f32,
    pub dog_moving: bool,
    pub dog_rms_db: f32,
    pub dog_peak_db: f32,
    pub dog_last_event: Option<String>,
}
