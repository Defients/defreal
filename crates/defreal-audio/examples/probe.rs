//! Device probe: enumerate inputs/outputs, optionally run the engine live.
//!
//!   cargo run -p defreal-audio --example probe           # list devices
//!   cargo run -p defreal-audio --example probe -- run 5  # run engine 5s

use defreal_audio::engine::{AudioEngine, EngineConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let inputs = defreal_audio::device::list_inputs().unwrap_or_default();
    let outputs = defreal_audio::device::list_outputs().unwrap_or_default();

    println!("=== INPUT DEVICES ({}) ===", inputs.len());
    for d in &inputs {
        println!(
            "  {}{}  {}ch @{}Hz ({}..{}Hz)",
            d.name,
            if d.is_default { " [default]" } else { "" },
            d.default_channels,
            d.default_rate,
            d.min_rate,
            d.max_rate
        );
    }
    println!("=== OUTPUT DEVICES ({}) ===", outputs.len());
    for d in &outputs {
        println!(
            "  {}{}  {}ch @{}Hz ({}..{}Hz)",
            d.name,
            if d.is_default { " [default]" } else { "" },
            d.default_channels,
            d.default_rate,
            d.min_rate,
            d.max_rate
        );
    }

    if args.iter().any(|a| a == "run") {
        let secs = args
            .windows(2)
            .find(|w| w[0] == "run")
            .and_then(|w| w[1].parse::<u64>().ok())
            .unwrap_or(5);
        let in_name = args
            .windows(2)
            .find(|w| w[0] == "--in")
            .map(|w| w[1].clone());
        let out_name = args
            .windows(2)
            .find(|w| w[0] == "--out")
            .map(|w| w[1].clone());

        println!("\n=== RUNNING ENGINE for {}s ===", secs);
        println!("  in : {:?}", in_name);
        println!("  out: {:?}", out_name);
        let mut eng = AudioEngine::new();
        let tel = eng.telemetry();
        match eng.start(EngineConfig {
            input_device: in_name,
            output_device: out_name,
            ..Default::default()
        }) {
            Ok(()) => {
                println!("  started OK");
                for i in 0..secs * 2 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let s = tel.snapshot(eng.last_error());
                    println!(
                        "  t={:.1}s state={:?} in_rms={:.1}dB out_rms={:.1}dB fill(in={:.2},out={:.2}) underruns={} overflows={} proc={:.0}us lat={:.1}ms motion={} d={:.2}m",
                        i as f32 * 0.5,
                        s.state,
                        s.in_rms_db,
                        s.out_rms_db,
                        s.in_fill,
                        s.out_fill,
                        s.output_underflows,
                        s.input_overflows,
                        s.proc_avg_us,
                        s.est_latency_ms,
                        s.motion_state,
                        s.motion_distance_m,
                    );
                }
                eng.stop();
                println!("  stopped OK");
            }
            Err(e) => {
                println!("  START FAILED: {e}");
            }
        }
    }
}
