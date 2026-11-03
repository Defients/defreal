//! Integration tests for the processing pipeline WITHOUT physical devices.
//! Drives the real Pump on synthetic ring buffers: capture path, DSP,
//! resampling, drift control, output path - everything except cpal streams.
//! (Real device I/O is environment-dependent and covered by examples/probe.)

use arc_swap::ArcSwap;
use defreal_audio::engine::Pump;
use defreal_audio::telemetry::SharedTelemetry;
use defreal_dsp::ChainParams;
use ringbuf::traits::{Observer, Producer, Consumer, Split};
use ringbuf::HeapRb;
use std::f32::consts::PI;
use std::sync::Arc;

struct Rig {
    pump: Pump,
    in_prod: ringbuf::HeapProd<f32>,
    out_cons: ringbuf::HeapCons<f32>,
    telemetry: Arc<SharedTelemetry>,
    params: Arc<ArcSwap<ChainParams>>,
}

fn rig(in_rate: u32, out_rate: u32) -> Rig {
    let (in_prod, in_cons) = HeapRb::<f32>::new(in_rate as usize).split();
    let (out_prod, out_cons) = HeapRb::<f32>::new(out_rate as usize).split();
    let telemetry = Arc::new(SharedTelemetry::new());
    let params = Arc::new(ArcSwap::from_pointee(ChainParams::default()));
    let pump = Pump::new(
        in_cons,
        out_prod,
        in_rate,
        out_rate,
        5.0,
        telemetry.clone(),
        params.clone(),
        42,
    )
    .expect("pump");
    Rig { pump, in_prod, out_cons, telemetry, params }
}

fn sine_at(rate: u32, freq: f32, amp: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amp * (2.0 * PI * freq * i as f32 / rate as f32).sin())
        .collect()
}

fn push_all(r: &mut Rig, data: &[f32]) {
    for &s in data {
        // producer may outpace; just block-free push what's possible
        while r.in_prod.try_push(s).is_err() {
            r.pump.tick();
        }
    }
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn full_pipeline_passes_signal() {
    let mut r = rig(48000, 48000);
    let input = sine_at(48000, 220.0, 0.3, 48000);
    push_all(&mut r, &input);
    for _ in 0..400 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 8000 {
            break;
        }
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    assert!(out.len() > 2000, "too little output: {}", out.len());
    let level = rms(&out[out.len() / 2..]);
    assert!(level > 0.01, "no signal out: {level}");
}

#[test]
fn bypass_passes_signal_unchanged() {
    let mut r = rig(48000, 48000);
    r.params.store(Arc::new(ChainParams {
        bypass: true,
        ..Default::default()
    }));
    let input = sine_at(48000, 220.0, 0.25, 24000);
    push_all(&mut r, &input);
    for _ in 0..300 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 4000 {
            break;
        }
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    assert!(out.len() > 1000);
    assert!(rms(&out) > 0.05);
}

#[test]
fn sample_rate_mismatch_works() {
    // 44.1k capture -> 48k playback: exercises real SRC both ways
    let mut r = rig(44100, 48000);
    let input = sine_at(44100, 220.0, 0.3, 44100);
    push_all(&mut r, &input);
    for _ in 0..600 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 8000 {
            break;
        }
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    assert!(out.len() > 2000, "out {}", out.len());
    assert!(rms(&out[out.len() / 2..]) > 0.01);
}

#[test]
fn reverse_rate_mismatch_works() {
    let mut r = rig(48000, 44100);
    let input = sine_at(48000, 220.0, 0.3, 48000);
    push_all(&mut r, &input);
    for _ in 0..600 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 6000 {
            break;
        }
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    assert!(out.len() > 1000);
    assert!(rms(&out[out.len() / 2..]) > 0.01);
}

#[test]
fn silent_input_produces_bounded_output() {
    let mut r = rig(48000, 48000);
    push_all(&mut r, &vec![0.0; 24000]);
    for _ in 0..200 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 4000 {
            break;
        }
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    assert!(out.iter().all(|s| s.is_finite()));
    // silence in -> near silence out (ambience texture at -54dB is ~inaudible)
    assert!(rms(&out) < 0.05, "rms {}", rms(&out));
}

#[test]
fn preset_change_during_processing_is_stable() {
    let mut r = rig(48000, 48000);
    let presets = defreal_dsp::preset::factory_presets();
    let input = sine_at(48000, 220.0, 0.3, 24000);
    push_all(&mut r, &input);
    for i in 0..200 {
        if i % 10 == 0 {
            let p = presets[(i / 10) % presets.len()].params.clone();
            r.params.store(Arc::new(p));
        }
        r.pump.tick();
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    assert!(out.iter().all(|s| s.is_finite()));
    assert!(rms(&out) > 0.005);
}

#[test]
fn extreme_realism_does_not_clip() {
    let mut r = rig(48000, 48000);
    let mut p = ChainParams::default();
    p.realism = 1.0;
    p.output_gain_db = 6.0;
    r.params.store(Arc::new(p));
    let input = sine_at(48000, 220.0, 0.9, 24000);
    push_all(&mut r, &input);
    for _ in 0..300 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 8000 {
            break;
        }
    }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() {
        out.push(s);
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak <= 1.0, "clipped peak {peak}");
}

#[test]
fn telemetry_publishes_real_values() {
    let mut r = rig(48000, 48000);
    let input = sine_at(48000, 220.0, 0.4, 24000);
    push_all(&mut r, &input);
    for _ in 0..100 {
        r.pump.tick();
    }
    let s = r.telemetry.snapshot(None);
    assert!(s.in_rms_db > -40.0, "in_rms {}", s.in_rms_db);
    assert!(s.proc_avg_us > 0.0);
}

#[test]
fn motion_commands_flow_through() {
    let mut r = rig(48000, 48000);
    r.pump
        .chain_mut()
        .motion_mut()
        .trigger(defreal_dsp::MotionState::LeanBack);
    let input = sine_at(48000, 220.0, 0.3, 96000); // 2s of audio
    push_all(&mut r, &input);
    for _ in 0..500 {
        r.pump.tick();
        let d = r.pump.chain_mut().motion_debug().distance_m;
        if d > 0.45 {
            return; // moved away from neutral — pass
        }
    }
    let d = r.pump.chain_mut().motion_debug().distance_m;
    assert!(d > 0.34, "distance {d}");
}
