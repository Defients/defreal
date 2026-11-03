//! The real-time duplex engine.
//!
//! Threading model:
//!   - CPAL input callback:  convert to f32 mono -> push input ring buffer
//!                           (no alloc, no locks, bounded drop on overflow)
//!   - Processing thread:    drain input rb -> resample to 48k -> DspChain
//!                           -> resample to device rate -> output rb
//!                           (all DSP + alloc happens here, NOT in a callback)
//!   - CPAL output callback: pop interleaved f32 -> device buffer
//!                           (zero-fill + counter on underflow)
//!
//! Param changes go through an ArcSwap snapshot; event commands through an
//! mpsc channel consumed by the processing thread.

use crate::device;
use crate::resample::{InputResampler, OutputResampler};
use crate::telemetry::{BufferStats, EngineState, SharedTelemetry};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use defreal_dsp::motion::{MotionState, PositionState};
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
    ReleaseManual,
    ResetChain,
    Reseed(u64),
    ResetCounters,
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

    /// Start the engine. Blocks until streams are up or init fails
    /// (bounded ~5s), so the caller knows the real outcome.
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
        Ok(io) => {
            let _ = ready_tx.send(Ok("started".into()));
            run_loop(config, io, cmd_rx, telemetry, params, stop, last_error);
        }
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
    eprintln!("[engine] resolving devices");
    let in_dev = device::find_input(config.input_device.as_deref())?;
    let out_dev = device::find_output(config.output_device.as_deref())?;
    eprintln!("[engine] devices resolved");

    let in_supported = in_dev
        .default_input_config()
        .map_err(|e| EngineError::Stream(format!("input config: {e}")))?;
    eprintln!("[engine] input cfg ok");
    let out_supported = out_dev
        .default_output_config()
        .map_err(|e| EngineError::Stream(format!("output config: {e}")))?;
    eprintln!("[engine] output cfg ok");

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

    eprintln!("[engine] building input stream");
    let in_stream = build_input(&in_dev, &in_cfg, in_supported.sample_format(), in_prod, telemetry.clone())?;
    eprintln!("[engine] building output stream");
    let out_stream = build_output(&out_dev, &out_cfg, out_supported.sample_format(), out_cons, telemetry.clone())?;

    eprintln!("[engine] play()");
    in_stream
        .play()
        .map_err(|e| EngineError::Stream(format!("input play: {e}")))?;
    eprintln!("[engine] input playing");
    out_stream
        .play()
        .map_err(|e| EngineError::Stream(format!("output play: {e}")))?;
    eprintln!("[engine] output playing");

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

fn build_output(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    fmt: SampleFormat,
    mut cons: ringbuf::HeapCons<f32>,
    telemetry: Arc<SharedTelemetry>,
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
                                tel.output_underflows.fetch_add(1, Ordering::Relaxed);
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

// --- processing loop --------------------------------------------------------

/// The stream-independent processing core: drains the input ring buffer,
/// resamples to internal rate, runs the DspChain, resamples to output rate,
/// fills the output ring buffer. Unit/integration tests drive `tick()`
/// directly on synthetic ring buffers; the live loop just calls it.
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
    // preallocated scratch (allocated once, reused every tick)
    dev_in: Vec<f32>,
    res48: Vec<f32>,
    interleaved: Vec<f32>,
    il: Vec<f32>,
    ir: Vec<f32>,
    ol: Vec<f32>,
    or: Vec<f32>,
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
            dev_in: vec![0.0; in_chunk],
            res48: Vec::with_capacity(block_frames * 4),
            interleaved: vec![0.0; block_frames * 4 * 2],
            il: vec![0.0; block_frames * 4],
            ir: vec![0.0; block_frames * 4],
            ol: vec![0.0; out_chunk],
            or: vec![0.0; out_chunk],
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

    /// One pump iteration: all queued input -> DSP -> staged -> output rb.
    /// Returns whether any work happened.
    pub fn tick(&mut self) -> bool {
        let mut did_work = false;

        // --- input: device rate -> 48k -> DSP -> mid staging ---
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

            for i in 0..n {
                let _ = self.mid_l.try_push(self.interleaved[i * 2]);
                let _ = self.mid_r.try_push(self.interleaved[i * 2 + 1]);
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
        }

        // --- output: mid staging -> device rate -> out rb ---
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

        did_work
    }
}

fn run_loop(
    config: EngineConfig,
    io: LoopIo,
    cmd_rx: Receiver<EngineCommand>,
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
            return;
        }
    };
    pump.prefill(prebuffer_frames);

    telemetry.set_state(EngineState::Running);

    while !stop.load(Ordering::Relaxed) {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                EngineCommand::TriggerMotion(s) => pump.chain_mut().motion_mut().trigger(s),
                EngineCommand::SetManualPosition(p) => {
                    pump.chain_mut().motion_mut().set_manual(p)
                }
                EngineCommand::ReleaseManual => pump.chain_mut().motion_mut().release_manual(),
                EngineCommand::ResetChain => pump.chain_mut().reset(),
                EngineCommand::Reseed(s) => {
                    pump.chain_mut().reset();
                    pump.chain_mut().motion_mut().reseed(s);
                }
                EngineCommand::ResetCounters => telemetry.reset_counters(),
            }
        }

        if !pump.tick() {
            // nothing to do: wait for input or a command
            let _ = cmd_rx.recv_timeout(Duration::from_millis(2));
        }
    }

    telemetry.set_state(EngineState::Idle);
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
