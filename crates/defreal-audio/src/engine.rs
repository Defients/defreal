//! The real-time duplex engine.
//!
//! Threading model:
//!   - CPAL input callback:  convert to f32 mono -> push input ring buffer
//!                           (no alloc, no locks, bounded drop on overflow)
//!   - Processing thread:    drain input rb -> resample to 48k -> DspChain
//!                           -> resample per branch -> output rb(s)
//!                           (all DSP + alloc happens here, NOT in a callback)
//!   - CPAL output callback: pop interleaved f32 -> device buffer
//!                           (zero-fill + counter on underflow)
//!
//! ONE processed signal, TWO output branches. The monitor branch gets its own
//! staging buffer, resampler, drift control and lifecycle — it can never starve
//! or block the main output. Monitor streams are opened on a worker thread so
//! a slow/failed device open never stalls the live pump.
//!
//! Param changes go through an ArcSwap snapshot; event commands through an
//! mpsc channel consumed by the processing thread. Commands are NEVER dropped:
//! recv_timeout results are dispatched like any other.

use crate::device;
use crate::resample::{InputResampler, OutputResampler};
use crate::telemetry::{BufferStats, EngineState, MonitorState, SharedTelemetry};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use defreal_dsp::motion::{MotionState, PositionState};
use defreal_dsp::smooth::SmoothedParam;
use defreal_dsp::{ChainParams, DspChain};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const INTERNAL_RATE: u32 = 48000;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// None = system default
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    /// processing block size at internal rate, ms
    pub block_ms: f32,
    /// output pre-buffer (covers scheduling jitter), ms
    pub prebuffer_ms: f32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            block_ms: 5.0,
            prebuffer_ms: 12.0,
        }
    }
}

/// Direct-monitor (second output) configuration. Device identity is the
/// disambiguated `DeviceInfo::id` ("name" or "name#n").
#[derive(Clone, Debug, Default)]
pub struct MonitorConfig {
    pub enabled: bool,
    pub device: Option<String>,
    pub gain_db: f32,
    pub muted: bool,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0}")]
    Device(#[from] device::DeviceError),
    #[error("stream build failed: {0}")]
    Stream(String),
    #[error("resampler failed: {0}")]
    Resample(#[from] crate::resample::ResampleError),
    #[error("engine already running")]
    AlreadyRunning,
    #[error("engine thread died: {0}")]
    ThreadDied(String),
}

pub enum EngineCommand {
    TriggerMotion(MotionState),
    SetManualPosition(PositionState),
    /// Timed manual move (acoustic actions / snapshot recall).
    SetManualPositionTimed(PositionState, f32),
    /// Timed move, hold, then auto-release to automation (engine-side sequencing).
    MoveThenRelease(PositionState, f32, f32, f32),
    ReleaseManual,
    /// Timed automation resume (TOUCH mode).
    ReleaseManualTimed(f32),
    /// Latch at the current position.
    FreezeMotion,
    ResetChain,
    Reseed(u64),
    ResetCounters,
    /// Reconfigure the monitor branch (device/gain/mute/enable).
    SetMonitor(MonitorConfig),
    /// DefDog: full config replace.
    DogSet(defreal_dsp::DogConfig),
    /// ROOMLOGIC scene config (enabled/door/room-b profile)
    SceneSet(defreal_dsp::SceneConfig),
    EmitterAdd(defreal_dsp::EmitterCfg),
    EmitterRemove(u32),
    EmitterUpdate(u32, defreal_dsp::EmitterCfg),
    /// DefDog: Come/Settle/Stay.
    DogCmd(defreal_dsp::DogCommand),
    /// DefDog: deliberate non-bark preview.
    DogPreview,
}

pub struct AudioEngine {
    telemetry: Arc<SharedTelemetry>,
    params: Arc<ArcSwap<ChainParams>>,
    cmd_tx: Option<Sender<EngineCommand>>,
    stop_flag: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    last_error: Arc<Mutex<Option<String>>>,
    config: EngineConfig,
}

impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            telemetry: Arc::new(SharedTelemetry::new()),
            params: Arc::new(ArcSwap::from_pointee(ChainParams::default())),
            cmd_tx: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            join: None,
            last_error: Arc::new(Mutex::new(None)),
            config: EngineConfig::default(),
        }
    }

    pub fn telemetry(&self) -> Arc<SharedTelemetry> {
        self.telemetry.clone()
    }

    pub fn params_handle(&self) -> Arc<ArcSwap<ChainParams>> {
        self.params.clone()
    }

    /// Update DSP params (cheap; the processing thread picks it up next block).
    pub fn set_params(&self, p: ChainParams) {
        self.params.store(Arc::new(p));
    }

    pub fn send(&self, cmd: EngineCommand) -> Result<(), EngineError> {
        match &self.cmd_tx {
            Some(tx) => tx.send(cmd).map_err(|_| EngineError::ThreadDied("cmd channel closed".into())),
            None => Err(EngineError::ThreadDied("not running".into())),
        }
    }

    /// Try to send; silently drops when not running (UI safety).
    pub fn try_send(&self, cmd: EngineCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(cmd);
        }
    }

    pub fn is_running(&self) -> bool {
        self.telemetry.state() == EngineState::Running
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|e| e.clone())
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Start the engine. Blocks until streams AND the processing pump are
    /// fully up (bounded ~5s), so the caller knows the real outcome.
    pub fn start(&mut self, config: EngineConfig) -> Result<(), EngineError> {
        if self.is_running() {
            return Err(EngineError::AlreadyRunning);
        }
        self.config = config.clone();
        self.stop_flag.store(false, Ordering::SeqCst);
        if let Ok(mut e) = self.last_error.lock() {
            *e = None;
        }
        self.telemetry.reset_counters();
        self.telemetry.set_state(EngineState::Starting);

        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let (ready_tx, ready_rx) = channel::<Result<String, EngineError>>();
        let telemetry = self.telemetry.clone();
        let params = self.params.clone();
        let stop = self.stop_flag.clone();
        let last_error = self.last_error.clone();

        self.join = Some(
            std::thread::Builder::new()
                .name("defreal-engine".into())
                .spawn(move || {
                    engine_thread(config, cmd_rx, ready_tx, telemetry, params, stop, last_error);
                })
                .map_err(|e| EngineError::ThreadDied(e.to_string()))?,
        );
        self.cmd_tx = Some(cmd_tx);

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(_msg)) => Ok(()),
            Ok(Err(e)) => {
                self.stop();
                Err(e)
            }
            Err(_) => {
                self.stop();
                Err(EngineError::Stream("timed out starting streams".into()))
            }
        }
    }

    /// Stop processing and release devices. Waits for thread exit, bounded:
    /// if a device open is wedged inside the backend (seen on some WASAPI
    /// devices), we abandon the thread rather than hang the UI forever.
    pub fn stop(&mut self) {
        self.telemetry.set_state(EngineState::Stopping);
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(tx) = self.cmd_tx.take() {
            let _ = tx.send(EngineCommand::ResetCounters); // wake the loop
        }
        if let Some(j) = self.join.take() {
            if j.is_finished() {
                let _ = j.join();
            } else {
                // bounded join
                let t0 = Instant::now();
                while !j.is_finished() && t0.elapsed() < Duration::from_secs(2) {
                    std::thread::sleep(Duration::from_millis(10));
                }
                if j.is_finished() {
                    let _ = j.join();
                } else {
                    // leak the handle; the thread dies with the process.
                    // Streams it holds get released on process exit.
                    let _ = &j;
                }
            }
        }
        self.telemetry.set_state(EngineState::Idle);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// engine thread
// ---------------------------------------------------------------------------

/// What the run loop needs from stream construction.
struct LoopIo {
    in_stream: cpal::Stream,
    out_stream: cpal::Stream,
    in_cfg: StreamConfig,
    out_cfg: StreamConfig,
    in_cons: ringbuf::HeapCons<f32>,
    out_prod: ringbuf::HeapProd<f32>,
}

fn engine_thread(
    config: EngineConfig,
    cmd_rx: Receiver<EngineCommand>,
    ready_tx: Sender<Result<String, EngineError>>,
    telemetry: Arc<SharedTelemetry>,
    params: Arc<ArcSwap<ChainParams>>,
    stop: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
) {
    match build_streams(&config, &telemetry) {
        Ok(io) => run_loop(config, io, cmd_rx, Some(ready_tx), telemetry, params, stop, last_error),
        Err(e) => {
            telemetry.set_state(EngineState::Failed);
            if let Ok(mut le) = last_error.lock() {
                *le = Some(e.to_string());
            }
            let _ = ready_tx.send(Err(e));
        }
    }
}

fn build_streams(
    config: &EngineConfig,
    telemetry: &Arc<SharedTelemetry>,
) -> Result<LoopIo, EngineError> {
    let in_dev = device::find_input(config.input_device.as_deref())?;
    let out_dev = device::find_output(config.output_device.as_deref())?;

    let in_supported = in_dev
        .default_input_config()
        .map_err(|e| EngineError::Stream(format!("input config: {e}")))?;
    let out_supported = out_dev
        .default_output_config()
        .map_err(|e| EngineError::Stream(format!("output config: {e}")))?;

    let in_cfg: StreamConfig = in_supported.clone().into();
    let out_cfg: StreamConfig = out_supported.clone().into();

    // ~1s mono input buffer; ~250ms interleaved output buffer
    let in_cap = in_cfg.sample_rate.0 as usize;
    let out_cap = (out_cfg.sample_rate.0 as usize / 4).max(4096) * 2; // interleaved
    let (in_prod, in_cons) = HeapRb::<f32>::new(in_cap.max(8192)).split();
    let (out_prod, out_cons) = HeapRb::<f32>::new(out_cap.max(8192)).split();

    telemetry.set_stream_info(
        in_cfg.sample_rate.0 as f32,
        out_cfg.sample_rate.0 as f32,
        in_cfg.channels as u8,
        out_cfg.channels as u8,
    );

    let in_stream = build_input(&in_dev, &in_cfg, in_supported.sample_format(), in_prod, telemetry.clone())?;
    let out_stream = build_output(
        &out_dev,
        &out_cfg,
        out_supported.sample_format(),
        out_cons,
        telemetry.clone(),
        OutBranch::Main,
    )?;

    in_stream
        .play()
        .map_err(|e| EngineError::Stream(format!("input play: {e}")))?;
    out_stream
        .play()
        .map_err(|e| EngineError::Stream(format!("output play: {e}")))?;

    Ok(LoopIo {
        in_stream,
        out_stream,
        in_cfg,
        out_cfg,
        in_cons,
        out_prod,
    })
}

// --- input callback: any sample format -> mono f32 -> ring buffer ----------

fn build_input(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    fmt: SampleFormat,
    mut prod: ringbuf::HeapProd<f32>,
    telemetry: Arc<SharedTelemetry>,
) -> Result<cpal::Stream, EngineError> {
    let ch = cfg.channels as usize;
    let err_tel = telemetry.clone();
    let err_cb = move |_e: cpal::StreamError| {
        err_tel.error_code.store(1, Ordering::Relaxed);
    };

    // `push_mono` inlined per format: mix channels to mono, bounded push.
    macro_rules! build {
        ($ty:ty, $conv:expr) => {
            dev.build_input_stream(
                cfg,
                move |data: &[$ty], _| {
                    let conv: fn($ty) -> f32 = $conv;
                    let frames = data.len() / ch;
                    let tel = &telemetry;
                    for f in 0..frames {
                        let mut acc = 0.0f32;
                        for c in 0..ch {
                            acc += conv(data[f * ch + c]);
                        }
                        let mono = acc / ch as f32;
                        // bounded, non-blocking; drop-newest on overload
                        if prod.try_push(mono).is_err() {
                            tel.input_overflows.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    tel.input_blocks.fetch_add(1, Ordering::Relaxed);
                },
                err_cb,
                None,
            )
        };
    }

    match fmt {
        SampleFormat::F32 => build!(f32, |x: f32| x),
        SampleFormat::I16 => build!(i16, |x: i16| x as f32 / 32768.0),
        SampleFormat::U16 => build!(u16, |x: u16| (x as f32 - 32768.0) / 32768.0),
        other => Err(EngineError::Stream(format!("unsupported input format {other}")))?,
    }
    .map_err(|e| EngineError::Stream(format!("build input: {e}")))
}

// --- output callback: ring buffer -> interleaved device buffer -------------

/// Which output branch a stream belongs to — selects the underflow counter
/// so main and monitor starvation are reported independently.
#[derive(Clone, Copy)]
pub enum OutBranch {
    Main,
    Monitor,
}

/// Generic stereo-interleaved output callback.
fn build_output(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    fmt: SampleFormat,
    mut cons: ringbuf::HeapCons<f32>,
    telemetry: Arc<SharedTelemetry>,
    branch: OutBranch,
) -> Result<cpal::Stream, EngineError> {
    let ch = cfg.channels as usize;
    let err_tel = telemetry.clone();
    let err_cb = move |_e: cpal::StreamError| {
        err_tel.error_code.store(2, Ordering::Relaxed);
    };

    macro_rules! build {
        ($ty:ty, $conv:expr) => {
            dev.build_output_stream(
                cfg,
                move |data: &mut [$ty], _| {
                    let conv: fn(f32) -> $ty = $conv;
                    let frames = data.len() / ch;
                    let tel = &telemetry;
                    for f in 0..frames {
                        let l = match cons.try_pop() {
                            Some(v) => v,
                            None => {
                                match branch {
                                    OutBranch::Main => tel.output_underflows.fetch_add(1, Ordering::Relaxed),
                                    OutBranch::Monitor => tel.mon_underflows.fetch_add(1, Ordering::Relaxed),
                                };
                                0.0
                            }
                        };
                        let r = cons.try_pop().unwrap_or(l);
                        for c in 0..ch {
                            data[f * ch + c] = conv(if c == 0 { l } else if c == 1 { r } else { 0.0 });
                        }
                    }
                    tel.output_blocks.fetch_add(1, Ordering::Relaxed);
                },
                err_cb,
                None,
            )
        };
    }

    match fmt {
        SampleFormat::F32 => build!(f32, |x: f32| x),
        SampleFormat::I16 => build!(i16, |x: f32| (x.clamp(-1.0, 1.0) * 32767.0) as i16),
        SampleFormat::U16 => build!(u16, |x: f32| (x.clamp(-1.0, 1.0) * 32767.0 + 32768.0) as u16),
        other => Err(EngineError::Stream(format!("unsupported output format {other}")))?,
    }
    .map_err(|e| EngineError::Stream(format!("build output: {e}")))
}

// ---------------------------------------------------------------------------
// monitor branch (second, independent output)
// ---------------------------------------------------------------------------

/// Everything the pump needs to feed a monitor stream. The stream itself is
/// !Send (WASAPI COM object) so it is created and destroyed on its own
/// worker thread — the engine holds only a MonitorHandle (stop signal +
/// join handle), and the pump only ever holds the Send producer half.
#[doc(hidden)]
pub struct MonitorBranch {
    prod: ringbuf::HeapProd<f32>,
    mid_l: HeapRb<f32>,
    mid_r: HeapRb<f32>,
    res: OutputResampler,
    chunk: usize,
    rate: u32,
    channels: u8,
    gain: SmoothedParam,
    // level metering (pump-side, cheap)
    sum_sq: f64,
    peak: f32,
    cnt: u32,
}

/// Handle to a parked monitor-stream worker: signal stop + join (bounded).
/// Lives on the engine thread next to the pump, NOT inside the branch.
struct MonitorHandle {
    stop_tx: Sender<()>,
    join: JoinHandle<()>,
}

fn stop_monitor_handle(h: MonitorHandle) {
    let _ = h.stop_tx.send(());
    let t0 = Instant::now();
    while !h.join.is_finished() && t0.elapsed() < Duration::from_millis(500) {
        std::thread::sleep(Duration::from_millis(5));
    }
    if h.join.is_finished() {
        let _ = h.join.join();
    }
}

/// What the worker sends back on success (all Send): the producer half and
/// stream format. The stream itself never crosses a thread boundary.
struct MonitorReady {
    prod: ringbuf::HeapProd<f32>,
    rate: u32,
    channels: u8,
}

/// Spawn the monitor worker. The returned handle stops it (and the stream).
/// The worker opens the stream on ITS OWN thread — so the !Send stream is
/// never moved — reports the Send parts, then parks until stopped.
fn spawn_monitor_worker(
    device_sel: Option<String>,
    telemetry: Arc<SharedTelemetry>,
    result_tx: Sender<(MonitorConfig, Result<MonitorReady, String>)>,
    cfg_for_result: MonitorConfig,
) -> Result<MonitorHandle, String> {
    let (stop_tx, stop_rx) = channel::<()>();
    let join = std::thread::Builder::new()
        .name("defreal-monitor".into())
        .spawn(move || {
            let opened = (|| -> Result<(ringbuf::HeapProd<f32>, u32, u8, cpal::Stream), String> {
                let dev = device::find_output(device_sel.as_deref()).map_err(|e| e.to_string())?;
                let supported = dev
                    .default_output_config()
                    .map_err(|e| format!("monitor config: {e}"))?;
                let cfg: StreamConfig = supported.clone().into();
                let cap = (cfg.sample_rate.0 as usize / 4).max(4096) * 2;
                let (prod, cons) = HeapRb::<f32>::new(cap.max(8192)).split();
                let stream = build_output(
                    &dev,
                    &cfg,
                    supported.sample_format(),
                    cons,
                    telemetry,
                    OutBranch::Monitor,
                )
                .map_err(|e| e.to_string())?;
                stream.play().map_err(|e| format!("monitor play: {e}"))?;
                Ok((prod, cfg.sample_rate.0, cfg.channels as u8, stream))
            })();

            match opened {
                Ok((prod, rate, channels, stream)) => {
                    let _ = result_tx.send((
                        cfg_for_result,
                        Ok(MonitorReady { prod, rate, channels }),
                    ));
                    // park holding the stream; dropped here when stopped
                    let _stream = stream;
                    let _ = stop_rx.recv();
                }
                Err(e) => {
                    let _ = result_tx.send((cfg_for_result, Err(e)));
                }
            }
        })
        .map_err(|e| format!("monitor thread: {e}"))?;
    Ok(MonitorHandle { stop_tx, join })
}

impl MonitorBranch {
    fn from_parts(parts: MonitorReady, gain_db: f32) -> Result<Self, EngineError> {
        let chunk = (5.0 * 0.001 * parts.rate as f32).max(16.0) as usize;
        let res = OutputResampler::new(parts.rate, INTERNAL_RATE, chunk)?;
        let mid_cap = INTERNAL_RATE as usize / 2;
        // prefill ~12 ms so playback starts without an underflow burst
        let prefill = (parts.rate as f32 * 0.012) as usize;
        let mut prod = parts.prod;
        for _ in 0..prefill.min(prod.capacity().get() / 2) {
            let _ = prod.try_push(0.0);
            let _ = prod.try_push(0.0);
        }
        Ok(Self {
            prod,
            mid_l: HeapRb::new(mid_cap),
            mid_r: HeapRb::new(mid_cap),
            res,
            chunk,
            rate: parts.rate,
            channels: parts.channels,
            // start silent: ramping up on enable avoids a burst on headphones
            gain: {
                let mut g = SmoothedParam::new(0.0, 30.0, INTERNAL_RATE as f32);
                g.set(defreal_dsp::envelope::from_db(gain_db.clamp(-60.0, 0.0)));
                g
            },
            sum_sq: 0.0,
            peak: 0.0,
            cnt: 0,
        })
    }

    /// Build a branch without a live stream (tests only): caller supplies the
    /// producer half of a ring buffer it can drain to verify fan-out.
    #[doc(hidden)]
    pub fn for_test(
        prod: ringbuf::HeapProd<f32>,
        rate: u32,
        channels: u8,
        gain_db: f32,
    ) -> Result<Self, EngineError> {
        Self::from_parts(MonitorReady { prod, rate, channels }, gain_db)
    }
}

// --- processing loop --------------------------------------------------------

/// The stream-independent processing core: drains the input ring buffer,
/// resamples to internal rate, runs the DspChain ONCE, fans the same stereo
/// result out to the main output and (if attached) the monitor branch.
/// Unit/integration tests drive `tick()` directly on synthetic ring buffers;
/// the live loop just calls it.
pub struct Pump {
    in_cons: ringbuf::HeapCons<f32>,
    out_prod: ringbuf::HeapProd<f32>,
    mid_l: HeapRb<f32>,
    mid_r: HeapRb<f32>,
    in_res: InputResampler,
    out_res: OutputResampler,
    chain: DspChain,
    telemetry: Arc<SharedTelemetry>,
    params: Arc<ArcSwap<ChainParams>>,
    // monitor (optional second output)
    mon: Option<MonitorBranch>,
    mon_cfg: MonitorConfig,
    // defdog: independent ambient source, mixed per-branch at fan-out
    dog: defreal_dsp::DogEngine,
    dog_cfg: defreal_dsp::DogConfig,
    dog_buf: Vec<f32>,
    // SOUNDSTAGE: environmental emitters in the shared ROOMLOGIC scene,
    // program-level bus fans out with the voice to every enabled branch
    stage: defreal_dsp::SoundStage,
    stage_buf: Vec<f32>,
    // preallocated scratch (allocated once, reused every tick)
    dev_in: Vec<f32>,
    res48: Vec<f32>,
    interleaved: Vec<f32>,
    il: Vec<f32>,
    ir: Vec<f32>,
    ol: Vec<f32>,
    or: Vec<f32>,
    ml: Vec<f32>,
    mr: Vec<f32>,
    in_capacity: usize,
    out_capacity: usize,
    in_chunk: usize,
    out_chunk: usize,
    mid_target: usize,
    in_rate: u32,
    out_rate: u32,
}

impl Pump {
    pub fn new(
        in_cons: ringbuf::HeapCons<f32>,
        out_prod: ringbuf::HeapProd<f32>,
        in_rate: u32,
        out_rate: u32,
        block_ms: f32,
        telemetry: Arc<SharedTelemetry>,
        params: Arc<ArcSwap<ChainParams>>,
        seed: u64,
    ) -> Result<Self, EngineError> {
        let block_frames = (block_ms * 0.001 * INTERNAL_RATE as f32) as usize;
        let in_chunk = (block_ms * 0.001 * in_rate as f32).max(16.0) as usize;
        let out_chunk = (block_ms * 0.001 * out_rate as f32).max(16.0) as usize;
        let in_capacity = in_cons.capacity().get();
        let out_capacity = out_prod.capacity().get();
        let in_res = InputResampler::new(in_rate, INTERNAL_RATE, in_chunk)?;
        let out_res = OutputResampler::new(out_rate, INTERNAL_RATE, out_chunk)?;
        let mid_cap = INTERNAL_RATE as usize / 2;
        Ok(Self {
            in_cons,
            out_prod,
            mid_l: HeapRb::new(mid_cap),
            mid_r: HeapRb::new(mid_cap),
            in_res,
            out_res,
            chain: DspChain::new(INTERNAL_RATE as f32, seed),
            telemetry,
            params,
            mon: None,
            mon_cfg: MonitorConfig::default(),
            dog: defreal_dsp::DogEngine::new(INTERNAL_RATE as f32, seed ^ 0xD06),
            dog_cfg: defreal_dsp::DogConfig::default(),
            dog_buf: vec![0.0; block_frames * 4 * 2],
            stage: defreal_dsp::SoundStage::new(INTERNAL_RATE as f32, seed ^ 0x57A6E),
            stage_buf: vec![0.0; block_frames * 4 * 2],
            dev_in: vec![0.0; in_chunk],
            res48: Vec::with_capacity(block_frames * 4),
            interleaved: vec![0.0; block_frames * 4 * 2],
            il: vec![0.0; block_frames * 4],
            ir: vec![0.0; block_frames * 4],
            ol: vec![0.0; out_chunk],
            or: vec![0.0; out_chunk],
            ml: vec![0.0; block_frames * 4],
            mr: vec![0.0; block_frames * 4],
            in_capacity,
            out_capacity,
            in_chunk,
            out_chunk,
            mid_target: (INTERNAL_RATE as f32 * 0.008) as usize,
            in_rate,
            out_rate,
        })
    }

    /// Prime the output buffer with silence so playback starts smoothly.
    pub fn prefill(&mut self, frames: usize) {
        for _ in 0..frames.min(self.out_capacity / 2) {
            let _ = self.out_prod.try_push(0.0);
            let _ = self.out_prod.try_push(0.0);
        }
    }

    pub fn chain_mut(&mut self) -> &mut DspChain {
        &mut self.chain
    }

    /// DefDog access (config handled via DogSet command; tests use this).
    #[doc(hidden)]
    pub fn dog_mut(&mut self) -> &mut defreal_dsp::DogEngine {
        &mut self.dog
    }
    #[doc(hidden)]
    /// SOUNDSTAGE access (config handled via commands; tests use this).
    pub fn stage_mut(&mut self) -> &mut defreal_dsp::SoundStage {
        &mut self.stage
    }

    pub fn dog_cfg_mut(&mut self) -> &mut defreal_dsp::DogConfig {
        &mut self.dog_cfg
    }

    /// Attach a ready monitor branch (constructed off-thread).
    #[doc(hidden)]
    pub fn attach_monitor(&mut self, b: MonitorBranch) {
        self.mon = Some(b);
    }

    /// Detach monitor stream + buffers (disable / device change).
    pub fn detach_monitor(&mut self) {
        self.mon = None;
    }

    /// Gain/mute/etc for the attached monitor (no stream rebuild).
    pub fn set_monitor_cfg(&mut self, cfg: MonitorConfig) {
        self.mon_cfg = cfg.clone();
        if let Some(m) = &mut self.mon {
            m.gain
                .set(defreal_dsp::envelope::from_db(cfg.gain_db.clamp(-60.0, 0.0)));
        }
        self.telemetry
            .set_monitor_gain(cfg.gain_db, cfg.muted || !cfg.enabled);
    }

    /// Drain one monitor chunk: staging -> resample -> gain/mute -> ring.
    fn pump_monitor(&mut self) {
        let Some(m) = &mut self.mon else { return };
        let muted = self.mon_cfg.muted || !self.mon_cfg.enabled;
        while m.mid_l.occupied_len() >= m.res.input_needed()
            && m.prod.vacant_len() >= m.chunk * 2
        {
            let need = m.res.input_needed().min(self.ml.len());
            for i in 0..need {
                self.ml[i] = m.mid_l.try_pop().unwrap_or(0.0);
                self.mr[i] = m.mid_r.try_pop().unwrap_or(0.0);
            }
            if m
                .res
                .process(&self.ml[..need], &self.mr[..need], &mut self.ol, &mut self.or)
                .is_err()
            {
                self.telemetry.error_code.store(4, Ordering::Relaxed);
                break;
            }
            // per-branch gain (ramped; mute -> gain 0) then push
            for i in 0..m.chunk {
                let g = if muted { 0.0 } else { m.gain.next() };
                let l = self.ol[i] * g;
                let r = self.or[i] * g;
                let _ = m.prod.try_push(l);
                let _ = m.prod.try_push(r);
                // meter on the post-gain signal
                m.sum_sq += ((l * l + r * r) * 0.5) as f64;
                m.peak = m.peak.max(l.abs()).max(r.abs());
                m.cnt += 1;
            }
            // drift control: keep monitor staging near the same 8ms target
            let fill = m.mid_l.occupied_len() as f64;
            let err = self.mid_target as f64 - fill;
            m.res.set_drift_correction(err, 3.0);
        }
        // publish branch stats every tick it's attached
        if m.cnt > 0 {
            let rms = (m.sum_sq / m.cnt as f64).sqrt() as f32;
            self.telemetry.publish_monitor(
                m.prod.occupied_len() as f32 / m.prod.capacity().get() as f32,
                defreal_dsp::envelope::to_db(rms),
                defreal_dsp::envelope::to_db(m.peak),
                m.rate as f32,
                m.channels,
            );
            m.sum_sq = 0.0;
            m.peak = 0.0;
            m.cnt = 0;
        }
    }

    /// One pump iteration: all queued input -> DSP -> staged -> output rb(s).
    /// Returns whether any work happened.
    pub fn tick(&mut self) -> bool {
        let mut did_work = false;

        // --- input: device rate -> 48k -> DSP -> mid staging (both branches) ---
        while self.in_cons.occupied_len() >= self.in_chunk {
            did_work = true;
            for s in self.dev_in.iter_mut() {
                *s = self.in_cons.try_pop().unwrap_or(0.0);
            }
            let produced = match self.in_res.process(&self.dev_in, &mut self.res48) {
                Ok(v) => v,
                Err(_) => {
                    self.telemetry.error_code.store(3, Ordering::Relaxed);
                    return did_work;
                }
            };
            if produced == 0 {
                continue;
            }
            let n = produced.min(self.interleaved.len() / 2);
            let t0 = Instant::now();
            let p = self.params.load();
            self.chain.process(&self.res48[..n], &mut self.interleaved[..n * 2], &p);
            drop(p);
            let proc_us = t0.elapsed().as_micros() as f32;

            // DefDog: update behavior at block rate, render the dog bus.
            // Same underlying events are fanned into whichever branch(es)
            // the route selects — never scheduled twice.
            let speech_level = self.chain.metrics().speech_level;
            self.dog.update(n as f32 / INTERNAL_RATE as f32, speech_level);
            {
                let rp = self.params.load();
                self.dog.sync_room(&rp.roomspace);
            }
            self.dog.render(&mut self.dog_buf[..n * 2]);
            // SOUNDSTAGE: schedule + render the emitter bus (program level,
            // fans out alongside the voice to both branches).
            self.stage.update(n as f32 / INTERNAL_RATE as f32);
            self.stage.render(&mut self.stage_buf[..n * 2]);
            let (dog_main, dog_mon) = match self.dog_cfg.route {
                defreal_dsp::DogRoute::Both => (true, true),
                defreal_dsp::DogRoute::Main => (true, false),
                defreal_dsp::DogRoute::Monitor => (false, true),
            };

            // ONE processed signal fans out to every branch staging queue.
            // Bounded: a stalled branch drops here, never blocks the others.
            let has_mon = self.mon.is_some();
            for i in 0..n {
                let dl = self.dog_buf[i * 2];
                let dr = self.dog_buf[i * 2 + 1];
                let vl = self.interleaved[i * 2] + self.stage_buf[i * 2];
                let vr = self.interleaved[i * 2 + 1] + self.stage_buf[i * 2 + 1];
                let _ = self.mid_l.try_push(vl + if dog_main { dl } else { 0.0 });
                let _ = self.mid_r.try_push(vr + if dog_main { dr } else { 0.0 });
                if has_mon {
                    let m = self.mon.as_mut().unwrap();
                    let _ = m.mid_l.try_push(vl + if dog_mon { dl } else { 0.0 });
                    let _ = m.mid_r.try_push(vr + if dog_mon { dr } else { 0.0 });
                }
            }

            let stats = BufferStats {
                in_fill: self.in_cons.occupied_len() as f32 / self.in_capacity as f32,
                out_fill: self.out_prod.occupied_len() as f32 / self.out_capacity as f32,
                drift_ppm: self.out_res.drift_ppm() as f32,
                latency_ms: estimate_latency_ms(
                    self.in_cons.occupied_len(),
                    self.mid_l.occupied_len(),
                    self.out_prod.occupied_len(),
                    self.in_rate,
                    self.out_rate,
                    self.in_res.output_delay(),
                    self.out_res.output_delay(),
                ),
            };
            self.telemetry
                .publish_block(&self.chain.metrics(), self.chain.motion_debug(), &stats, proc_us);
            {
                let (dr, dp) = self.dog.take_levels();
                let dd = self.dog.debug();
                self.telemetry.publish_dog(&dd, dr, dp);
                let (sr_, sp) = self.stage.take_levels();
                self.telemetry
                    .publish_stage(self.stage.door(), self.stage.enabled(), sr_, sp);
            }
        }

        // --- main output: mid staging -> device rate -> out rb ---
        while self.mid_l.occupied_len() >= self.out_res.input_needed()
            && self.out_prod.vacant_len() >= self.out_chunk * 2
        {
            did_work = true;
            let need = self.out_res.input_needed().min(self.il.len());
            for i in 0..need {
                self.il[i] = self.mid_l.try_pop().unwrap_or(0.0);
                self.ir[i] = self.mid_r.try_pop().unwrap_or(0.0);
            }
            if self
                .out_res
                .process(&self.il[..need], &self.ir[..need], &mut self.ol, &mut self.or)
                .is_err()
            {
                self.telemetry.error_code.store(4, Ordering::Relaxed);
                break;
            }
            for i in 0..self.out_chunk {
                let _ = self.out_prod.try_push(self.ol[i]);
                let _ = self.out_prod.try_push(self.or[i]);
            }

            // drift control: keep the mid staging near target occupancy
            let fill = self.mid_l.occupied_len() as f64;
            let err = self.mid_target as f64 - fill;
            // err>0 = staging draining = produce faster (raise ratio)
            self.out_res.set_drift_correction(err, 3.0);
        }

        // --- monitor output: independent staging -> own resampler -> own rb ---
        if self.mon.is_some() {
            self.pump_monitor();
        }

        did_work
    }
}

// --- command dispatch --------------------------------------------------------

/// Central command handler — every command lands here exactly once.
/// Extracted so the "dispatch commands received while idle" path is testable
/// and identical to the hot path.
#[doc(hidden)]
pub fn dispatch(cmd: EngineCommand, pump: &mut Pump, telemetry: &Arc<SharedTelemetry>) {
    match cmd {
        EngineCommand::TriggerMotion(s) => pump.chain_mut().motion_mut().trigger(s),
        EngineCommand::SetManualPosition(p) => {
            pump.chain_mut().motion_mut().set_manual(p)
        }
        EngineCommand::SetManualPositionTimed(p, d) => {
            pump.chain_mut().motion_mut().set_manual_timed(p, d)
        }
        EngineCommand::MoveThenRelease(p, md, hd, rd) => {
            pump.chain_mut().motion_mut().move_then_release(p, md, hd, rd)
        }
        EngineCommand::ReleaseManual => pump.chain_mut().motion_mut().release_manual(),
        EngineCommand::ReleaseManualTimed(d) => {
            pump.chain_mut().motion_mut().release_manual_timed(d)
        }
        EngineCommand::FreezeMotion => pump.chain_mut().motion_mut().freeze(),
        EngineCommand::ResetChain => pump.chain_mut().reset(),
        EngineCommand::Reseed(s) => {
            pump.chain_mut().reset();
            pump.chain_mut().motion_mut().reseed(s);
        }
        EngineCommand::ResetCounters => telemetry.reset_counters(),
        EngineCommand::SetMonitor(_) => {
            // handled in run_loop (needs the open-result channel), never here
        }
        EngineCommand::DogSet(cfg) => {
            pump.dog_cfg = cfg.clone();
            let room = pump.params.load().roomspace.clone();
            pump.dog.configure(&cfg, &room);
        }
        EngineCommand::DogCmd(c) => pump.dog.command(c),
        EngineCommand::DogPreview => pump.dog.preview(),
        EngineCommand::SceneSet(cfg) => {
            pump.stage.set_enabled(cfg.enabled);
            pump.stage.set_door(cfg.door_open);
            pump.dog.set_door(cfg.door_open);
            let b = defreal_dsp::roomspace::RoomspaceParams {
                room: cfg.room_b,
                ..Default::default()
            };
            let a = pump.params.load().roomspace.clone();
            pump.stage.sync_rooms(&a, &b);
            telemetry.publish_emitters(&pump.stage.info());
        }
        EngineCommand::EmitterAdd(cfg) => {
            pump.stage.add(cfg);
            telemetry.publish_emitters(&pump.stage.info());
        }
        EngineCommand::EmitterRemove(id) => {
            pump.stage.remove(id);
            telemetry.publish_emitters(&pump.stage.info());
        }
        EngineCommand::EmitterUpdate(id, cfg) => {
            let ok = pump.stage.emitter_mut(id, |c| *c = cfg.clone());
            if ok {
                telemetry.publish_emitters(&pump.stage.info());
            }
        }
    }
}

fn run_loop(
    config: EngineConfig,
    io: LoopIo,
    cmd_rx: Receiver<EngineCommand>,
    ready_tx: Option<Sender<Result<String, EngineError>>>,
    telemetry: Arc<SharedTelemetry>,
    params: Arc<ArcSwap<ChainParams>>,
    stop: Arc<AtomicBool>,
    last_error: Arc<Mutex<Option<String>>>,
) {
    let LoopIo {
        in_stream,
        out_stream,
        in_cfg,
        out_cfg,
        in_cons,
        out_prod,
        ..
    } = io;
    let _ = (&in_stream, &out_stream); // kept alive for the loop's duration

    let in_rate = in_cfg.sample_rate.0;
    let out_rate = out_cfg.sample_rate.0;
    let prebuffer_frames = (config.prebuffer_ms * 0.001 * out_rate as f32) as usize;

    let mut pump = match Pump::new(
        in_cons,
        out_prod,
        in_rate,
        out_rate,
        config.block_ms,
        telemetry.clone(),
        params.clone(),
        0xDEF1EA7,
    ) {
        Ok(p) => p,
        Err(e) => {
            telemetry.set_state(EngineState::Failed);
            if let Ok(mut le) = last_error.lock() {
                *le = Some(e.to_string());
            }
            if let Some(rt) = &ready_tx {
                let _ = rt.send(Err(e));
            }
            return;
        }
    };
    pump.prefill(prebuffer_frames);

    // Fully ready only now: streams live, pump built, buffer primed.
    telemetry.set_state(EngineState::Running);
    if let Some(rt) = &ready_tx {
        let _ = rt.send(Ok("started".into()));
    }

    // monitor open results arrive here from worker threads
    let (mon_tx, mon_rx) = channel::<(MonitorConfig, Result<MonitorReady, String>)>();
    // desired monitor config — gains/mute live-update without stream rebuild
    let mut mon_cfg = MonitorConfig::default();
    // handle to the parked monitor-stream worker (its stream never moves)
    let mut mon_handle: Option<MonitorHandle> = None;
    telemetry.set_monitor_state(MonitorState::Disabled);

    while !stop.load(Ordering::Relaxed) {
        // 1) drain every queued command; SetMonitor routes to branch mgmt,
        //    the rest dispatch into the pump.
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let EngineCommand::SetMonitor(cfg) = cmd {
                apply_monitor_cmd(
                    cfg,
                    &mut mon_cfg,
                    &mut pump,
                    &telemetry,
                    &mon_tx,
                    &mut mon_handle,
                );
            } else {
                dispatch(cmd, &mut pump, &telemetry);
            }
        }

        // 2) pick up finished monitor-open results (non-blocking)
        while let Ok((cfg, res)) = mon_rx.try_recv() {
            // stale result: user picked a different device while this opened
            if cfg.device != mon_cfg.device || !mon_cfg.enabled {
                continue; // stream dropped with `parts`
            }
            match res {
                Ok(parts) => {
                    let dev_name = cfg.device.clone().unwrap_or_default();
                    match MonitorBranch::from_parts(parts, mon_cfg.gain_db) {
                        Ok(b) => {
                            pump.attach_monitor(b);
                            pump.set_monitor_cfg(mon_cfg.clone());
                            telemetry.set_monitor_device(dev_name);
                            telemetry.set_monitor_state(MonitorState::Active);
                        }
                        Err(e) => {
                            telemetry.set_monitor_error(dev_name, Some(e.to_string()));
                            telemetry.set_monitor_state(MonitorState::Error);
                        }
                    }
                }
                Err(e) => {
                    telemetry.set_monitor_error(
                        cfg.device.clone().unwrap_or_default(),
                        Some(e),
                    );
                    telemetry.set_monitor_state(MonitorState::Error);
                }
            }
        }

        if !pump.tick() {
            // nothing to do: wait for input or a command.
            // FIX (v0.1.1): a command returned by recv_timeout was previously
            // DISCARDED — commands sent while the pump idled were silently
            // lost. Now we dispatch it like any other.
            match cmd_rx.recv_timeout(Duration::from_millis(2)) {
                Ok(EngineCommand::SetMonitor(cfg)) => apply_monitor_cmd(
                    cfg,
                    &mut mon_cfg,
                    &mut pump,
                    &telemetry,
                    &mon_tx,
                    &mut mon_handle,
                ),
                Ok(cmd) => dispatch(cmd, &mut pump, &telemetry),
                Err(_) => {}
            }
        }
    }

    if let Some(h) = mon_handle.take() {
        stop_monitor_handle(h);
    }
    telemetry.set_state(EngineState::Idle);
}

/// Route a SetMonitor command: enable/device-change => async open on a worker
/// thread (never blocks the pump); gain/mute => live-update the attached
/// branch; disable => detach.
fn apply_monitor_cmd(
    cfg: MonitorConfig,
    mon_cfg: &mut MonitorConfig,
    pump: &mut Pump,
    telemetry: &Arc<SharedTelemetry>,
    mon_tx: &Sender<(MonitorConfig, Result<MonitorReady, String>)>,
    mon_handle: &mut Option<MonitorHandle>,
) {
    let wants_active = cfg.enabled && cfg.device.is_some();
    let device_changed = cfg.device != mon_cfg.device;

    if !wants_active {
        pump.detach_monitor();
        if let Some(h) = mon_handle.take() {
            stop_monitor_handle(h);
        }
        *mon_cfg = MonitorConfig {
            enabled: false,
            device: cfg.device,
            gain_db: cfg.gain_db,
            muted: cfg.muted,
        };
        pump.set_monitor_cfg(mon_cfg.clone());
        telemetry.set_monitor_state(MonitorState::Disabled);
        return;
    }

    // track the desired config immediately so late-arriving opens attach
    // correctly and device-change detection stays stable
    *mon_cfg = cfg.clone();

    if device_changed || telemetry.monitor_state() != MonitorState::Active {
        // stop any existing/pending worker first
        pump.detach_monitor();
        if let Some(h) = mon_handle.take() {
            stop_monitor_handle(h);
        }
        telemetry.set_monitor_state(MonitorState::Opening);
        telemetry.set_monitor_device(cfg.device.clone().unwrap_or_default());
        match spawn_monitor_worker(
            cfg.device.clone(),
            telemetry.clone(),
            mon_tx.clone(),
            cfg.clone(),
        ) {
            Ok(h) => *mon_handle = Some(h),
            Err(e) => {
                telemetry.set_monitor_error(cfg.device.clone().unwrap_or_default(), Some(e));
                telemetry.set_monitor_state(MonitorState::Error);
            }
        }
    } else {
        // same device, already active: just update gain/mute live
        pump.set_monitor_cfg(cfg);
    }
}

fn estimate_latency_ms(
    in_fill: usize,
    mid_fill: usize,
    out_fill: usize,
    in_rate: u32,
    out_rate: u32,
    in_delay: usize,
    out_delay: usize,
) -> f32 {
    let in_ms = in_fill as f32 / in_rate as f32 * 1000.0;
    let mid_ms = mid_fill as f32 / INTERNAL_RATE as f32 * 1000.0;
    let out_ms = (out_fill as f32 / 2.0) / out_rate as f32 * 1000.0;
    let res_ms = (in_delay + out_delay) as f32 / INTERNAL_RATE as f32 * 1000.0;
    in_ms + mid_ms + out_ms + res_ms
}
