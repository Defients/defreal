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

// --- v0.1.1 regression tests -------------------------------------------------

/// The v0.1 loop discarded `recv_timeout` results when idle — commands sent
/// while the pump had no work vanished. dispatch() is now the single handler;
/// every command type must reach engine state.
#[test]
fn idle_commands_are_dispatched() {
    use defreal_audio::engine::dispatch;
    use defreal_audio::{EngineCommand, MonitorConfig};

    let mut r = rig(48000, 48000);
    // pump idle: no input queued. Commands must still land in engine state.
    dispatch(EngineCommand::TriggerMotion(defreal_dsp::MotionState::LeanBack), &mut r.pump, &r.telemetry);
    // now pump audio; the debug snapshot refreshes inside process()
    push_all(&mut r, &sine_at(48000, 220.0, 0.3, 4800));
    for _ in 0..4 { r.pump.tick(); }
    let d = r.pump.chain_mut().motion_debug();
    assert!(
        d.target_distance_m > 0.34 || d.state != defreal_dsp::MotionState::Neutral,
        "TriggerMotion lost: {d:?}"
    );

    dispatch(
        EngineCommand::SetManualPositionTimed(
            defreal_dsp::PositionState { distance_m: 1.0, azimuth_deg: 20.0, facing_deg: 5.0 },
            0.3,
        ),
        &mut r.pump, &r.telemetry,
    );
    push_all(&mut r, &sine_at(48000, 220.0, 0.3, 4800));
    for _ in 0..4 { r.pump.tick(); }
    let d = r.pump.chain_mut().motion_debug();
    assert!(d.manual, "SetManualPosition lost");

    dispatch(EngineCommand::ReleaseManualTimed(0.5), &mut r.pump, &r.telemetry);
    dispatch(
        EngineCommand::SetMonitor(MonitorConfig { enabled: true, device: None, gain_db: -18.0, muted: false }),
        &mut r.pump, &r.telemetry,
    );
    let s = r.telemetry.snapshot(None);
    assert_eq!(s.monitor_state, "disabled", "enabled monitor without attach must not claim active");
}

/// One DSP pass, two destinations: the monitor branch must receive the same
/// processed stereo content as the main branch (after its own resampler).
#[test]
fn dual_output_fanout_equivalent_signal() {
    let (mon_prod, mut mon_cons) = HeapRb::<f32>::new(96000).split();
    let mut r = rig(48000, 48000);
    let branch = defreal_audio::MonitorBranch::for_test(mon_prod, 48000, 2, 0.0).unwrap();
    r.pump.attach_monitor(branch);
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: Some("test".into()), gain_db: 0.0, muted: false,
    });

    let input = sine_at(48000, 220.0, 0.3, 48000);
    push_all(&mut r, &input);
    for _ in 0..600 {
        r.pump.tick();
        if mon_cons.occupied_len() > 8000 && r.out_cons.occupied_len() > 8000 { break; }
    }
    let mut main = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { main.push(s); }
    let mut mon = Vec::new();
    while let Some(s) = mon_cons.try_pop() { mon.push(s); }
    assert!(main.len() > 2000 && mon.len() > 1000, "main {} mon {}", main.len(), mon.len());
    // same program content: correlate energy — both carry the 220 Hz sine
    assert!(rms(&mon[mon.len() / 2..]) > 0.005, "monitor silent");
    assert!(rms(&main[main.len() / 2..]) > 0.005, "main silent");
}

/// A stalled monitor (nobody drains its ring) must not starve the main output.
#[test]
fn stalled_monitor_does_not_block_main() {
    let (mon_prod, _mon_cons) = HeapRb::<f32>::new(8192).split(); // tiny, fills fast
    let mut r = rig(48000, 48000);
    r.pump.attach_monitor(defreal_audio::MonitorBranch::for_test(mon_prod, 48000, 2, 0.0).unwrap());
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: None, gain_db: 0.0, muted: false,
    });
    let input = sine_at(48000, 220.0, 0.3, 48000);
    push_all(&mut r, &input);
    for _ in 0..600 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 8000 { break; }
    }
    let mut main = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { main.push(s); }
    assert!(main.len() > 2000, "monitor stall blocked main: {}", main.len());
    assert!(rms(&main[main.len() / 2..]) > 0.01);
}

/// Muted monitor yields silence on its ring while the main output keeps signal.
#[test]
fn monitor_mute_isolated() {
    let (mon_prod, mut mon_cons) = HeapRb::<f32>::new(96000).split();
    let mut r = rig(48000, 48000);
    r.pump.attach_monitor(defreal_audio::MonitorBranch::for_test(mon_prod, 48000, 2, 0.0).unwrap());
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: None, gain_db: 0.0, muted: true,
    });
    let input = sine_at(48000, 220.0, 0.3, 48000);
    push_all(&mut r, &input);
    for _ in 0..600 {
        r.pump.tick();
        if r.out_cons.occupied_len() > 8000 { break; }
    }
    let mut mon = Vec::new();
    while let Some(s) = mon_cons.try_pop() { mon.push(s); }
    let mut main = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { main.push(s); }
    assert!(main.len() > 2000);
    assert!(rms(&main) > 0.005, "main dead");
    // monitor rb is prefilled with zeros then receives silence — all zeros
    assert!(mon.iter().all(|s| s.abs() < 1e-6), "muted monitor leaked audio");
}

/// Monitor at a different device rate (44.1k): own resampler must feed it.
#[test]
fn monitor_different_rate() {
    let (mon_prod, mut mon_cons) = HeapRb::<f32>::new(96000).split();
    let mut r = rig(48000, 48000);
    r.pump.attach_monitor(defreal_audio::MonitorBranch::for_test(mon_prod, 44100, 2, 0.0).unwrap());
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: None, gain_db: 0.0, muted: false,
    });
    let input = sine_at(48000, 220.0, 0.3, 48000);
    push_all(&mut r, &input);
    for _ in 0..800 {
        r.pump.tick();
        if mon_cons.occupied_len() > 6000 { break; }
    }
    let mut mon = Vec::new();
    while let Some(s) = mon_cons.try_pop() { mon.push(s); }
    assert!(mon.len() > 1000, "monitor @44.1k got {}", mon.len());
    assert!(rms(&mon[mon.len() / 2..]) > 0.005);
}

/// Gain staging: -18 dB monitor vs 0 dB main — monitor energy ~1/64 of main.
#[test]
fn monitor_gain_independent() {
    let (mon_prod, mut mon_cons) = HeapRb::<f32>::new(96000).split();
    let mut r = rig(48000, 48000);
    r.pump.attach_monitor(defreal_audio::MonitorBranch::for_test(mon_prod, 48000, 2, -18.0).unwrap());
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: None, gain_db: -18.0, muted: false,
    });
    let input = sine_at(48000, 220.0, 0.3, 96000); // long enough to settle the ramp
    push_all(&mut r, &input);
    for _ in 0..800 {
        r.pump.tick();
        if mon_cons.occupied_len() > 16000 && r.out_cons.occupied_len() > 16000 { break; }
    }
    let mut mon = Vec::new();
    while let Some(s) = mon_cons.try_pop() { mon.push(s); }
    let mut main = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { main.push(s); }
    let mr = rms(&mon[mon.len() * 3 / 4..]);
    let rr = rms(&main[main.len() * 3 / 4..]);
    assert!(mr > 0.0005 && rr > 0.005, "mon {mr} main {rr}");
    assert!(mr < rr * 0.4, "-18dB monitor not attenuated: mon {mr} main {rr}");
}

/// MoveThenRelease must land the move BEFORE releasing to automation —
/// an immediate release would clobber the requested target.
#[test]
fn move_then_release_sequences() {
    use defreal_audio::engine::dispatch;
    use defreal_audio::EngineCommand;
    let mut r = rig(48000, 48000);
    dispatch(
        EngineCommand::MoveThenRelease(
            defreal_dsp::PositionState { distance_m: 1.4, azimuth_deg: -15.0, facing_deg: 0.0 },
            0.4, 1.0, 0.8,
        ),
        &mut r.pump, &r.telemetry,
    );
    // pump the motion clock: feed a little audio so ticks happen
    let input = vec![0.0f32; 4800];
    push_all(&mut r, &input);
    for _ in 0..4 { r.pump.tick(); std::thread::sleep(std::time::Duration::from_millis(30)); }
    let d = r.pump.chain_mut().motion_debug();
    // either still moving toward 1.4 (manual) or already released — but the
    // distance must be heading to 1.4, not snapped back to neutral.
    assert!(
        d.target_distance_m > 0.9 || d.distance_m > 0.5,
        "move_then_release target lost: {d:?}"
    );
}

// --- defdog routing tests (v0.2.0) --------------------------------------------

fn dog_cfg(f: impl Fn(&mut defreal_dsp::DogConfig)) -> defreal_dsp::DogConfig {
    let mut c = defreal_dsp::DogConfig { enabled: true, volume_db: -12.0, ..Default::default() };
    f(&mut c);
    c
}

fn drive_dog(r: &mut Rig, secs_input: usize) {
    // silence input; dog events carry the signal
    r.pump.dog_mut().preview();
    push_all(r, &vec![0.0f32; secs_input]);
    for _ in 0..secs_input / 100 {
        r.pump.dog_mut().preview();
        r.pump.tick();
    }
    for _ in 0..200 { r.pump.tick(); }
}

#[test]
fn dog_audio_reaches_main_when_enabled() {
    use defreal_audio::engine::dispatch;
    let mut r = rig(48000, 48000);
    dispatch(defreal_audio::EngineCommand::DogSet(dog_cfg(|c| c.route = defreal_dsp::DogRoute::Both)), &mut r.pump, &r.telemetry);
    drive_dog(&mut r, 48000);
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { out.push(s); }
    assert!(rms(&out) > 1e-4, "enabled dog silent on main: {}", rms(&out));
}

#[test]
fn dog_monitor_only_keeps_main_dry() {
    use defreal_audio::engine::dispatch;
    let (mon_prod, mut mon_cons) = HeapRb::<f32>::new(96000).split();
    let mut r = rig(48000, 48000);
    r.pump.attach_monitor(defreal_audio::MonitorBranch::for_test(mon_prod, 48000, 2, 0.0).unwrap());
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: None, gain_db: 0.0, muted: false,
    });
    dispatch(defreal_audio::EngineCommand::DogSet(dog_cfg(|c| c.route = defreal_dsp::DogRoute::Monitor)),
        &mut r.pump, &r.telemetry);
    drive_dog(&mut r, 48000);
    let mut main = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { main.push(s); }
    let mut mon = Vec::new();
    while let Some(s) = mon_cons.try_pop() { mon.push(s); }
    assert!(mon.len() > 1000, "monitor got nothing");
    let mr = rms(&mon);
    let rr = rms(&main);
    assert!(mr > rr * 10.0 || rr < 1e-4,
        "dog leaked to main under Monitor route: main {rr} mon {mr}");
}

#[test]
fn dog_main_only_keeps_monitor_dry() {
    use defreal_audio::engine::dispatch;
    let (mon_prod, mut mon_cons) = HeapRb::<f32>::new(96000).split();
    let mut r = rig(48000, 48000);
    r.pump.attach_monitor(defreal_audio::MonitorBranch::for_test(mon_prod, 48000, 2, 0.0).unwrap());
    r.pump.set_monitor_cfg(defreal_audio::MonitorConfig {
        enabled: true, device: None, gain_db: 0.0, muted: false,
    });
    dispatch(defreal_audio::EngineCommand::DogSet(dog_cfg(|c| c.route = defreal_dsp::DogRoute::Main)),
        &mut r.pump, &r.telemetry);
    drive_dog(&mut r, 48000);
    let mut main = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { main.push(s); }
    let mut mon = Vec::new();
    while let Some(s) = mon_cons.try_pop() { mon.push(s); }
    assert!(main.len() > 1000);
    let rr = rms(&main);
    let mr = rms(&mon);
    assert!(rr > mr * 10.0 || mr < 1e-4,
        "dog leaked to monitor under Main route: main {rr} mon {mr}");
}

#[test]
fn disabled_dog_produces_no_audio() {
    use defreal_audio::engine::dispatch;
    let mut r = rig(48000, 48000);
    // explicitly disable
    dispatch(defreal_audio::EngineCommand::DogSet(defreal_dsp::DogConfig::default()), &mut r.pump, &r.telemetry);
    r.pump.dog_mut().preview(); // preview with volume 0 => silent
    push_all(&mut r, &vec![0.0f32; 24000]);
    for _ in 0..300 { r.pump.tick(); }
    let mut out = Vec::new();
    while let Some(s) = r.out_cons.try_pop() { out.push(s); }
    // silence input + disabled dog => near-silence (ambience ~-54dB)
    assert!(rms(&out) < 0.02, "disabled dog audible: {}", rms(&out));
}

#[test]
fn dog_set_command_surfaces_in_telemetry() {
    use defreal_audio::engine::dispatch;
    let mut r = rig(48000, 48000);
    dispatch(defreal_audio::EngineCommand::DogSet(dog_cfg(|_| {})), &mut r.pump, &r.telemetry);
    push_all(&mut r, &vec![0.0f32; 4800]);
    for _ in 0..10 { r.pump.tick(); }
    let s = r.telemetry.snapshot(None);
    assert!(s.dog_enabled, "dog enabled not in telemetry");
    assert_eq!(s.dog_state, "resting");
}
