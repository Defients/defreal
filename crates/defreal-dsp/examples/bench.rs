//! Measure per-block DSP cost of the full chain at internal rate 48 kHz.
//! Run: cargo run -p defreal-dsp --release --example bench
use defreal_dsp::chain::{ChainParams, DspChain};
use std::time::Instant;

fn main() {
    let sr = 48_000.0f32;
    let block = 240; // 5 ms at 48 kHz
    let mut params = ChainParams::default();
    params.realism = 1.0; // worst case: all modules engaged
    params.ambience.enabled = true;
    params.roomspace.enabled = true;
    let mut chain = DspChain::new(sr, 0xDEF1EA7);

    let mut input = vec![0.0f32; block];
    let mut out = vec![0.0f32; block * 2];
    let mut phase = 0.0f32;

    for _ in 0..200 {
        fill(&mut input, &mut phase, sr);
        chain.process(&input, &mut out, &params);
    }

    let iters = 4_000usize; // ~20 s of audio
    let t0 = Instant::now();
    let mut worst = 0.0f64;
    for i in 0..iters {
        fill(&mut input, &mut phase, sr);
        let t = Instant::now();
        chain.process(&input, &mut out, &params);
        let d = t.elapsed().as_secs_f64() * 1e6;
        if i > 50 {
            worst = worst.max(d);
        }
    }
    let total = t0.elapsed().as_secs_f64();
    let audio_secs = iters as f64 * block as f64 / sr as f64;
    let avg_us = total * 1e6 / iters as f64;
    println!("block: {block} samples (5 ms) @ {sr} Hz, {iters} iters");
    println!("avg: {avg_us:.1} µs/block  worst: {worst:.1} µs/block");
    println!(
        "audio processed: {audio_secs:.2} s in {total:.3} s -> realtime factor {:.1}x",
        audio_secs / total
    );
    println!("DSP headroom: {:.2}% of a 5 ms block budget", avg_us / 5000.0 * 100.0);
}

fn fill(buf: &mut [f32], phase: &mut f32, sr: f32) {
    for s in buf.iter_mut() {
        *phase += 140.0 / sr;
        if *phase > 1.0 {
            *phase -= 1.0;
        }
        *s = (2.0 * std::f32::consts::PI * *phase).sin() * 0.2
            + (2.0 * std::f32::consts::PI * *phase * 2.7).sin() * 0.07;
    }
}
