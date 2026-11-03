//! Offline render example:
//!   cargo run -p defreal-audio --example render -- in.wav out.wav [preset_name] [ab_dir]

use defreal_audio::offline::{render_ab, render_wav};
use defreal_dsp::preset::factory_presets;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render <in.wav> <out.wav> [preset] [ab_dir]");
        std::process::exit(1);
    }
    let preset_name = args.get(3).cloned().unwrap_or_else(|| "Natural Presence".into());
    let presets = factory_presets();
    let preset = presets
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(&preset_name))
        .unwrap_or(&presets[0]);

    println!("preset: {}", preset.name);
    match render_wav(
        Path::new(&args[1]),
        Path::new(&args[2]),
        preset,
        0xDEF1EA7,
    ) {
        Ok(rep) => println!("render ok: {rep:?}"),
        Err(e) => {
            eprintln!("render failed: {e}");
            std::process::exit(1);
        }
    }

    if let Some(dir) = args.get(4) {
        std::fs::create_dir_all(dir).ok();
        match render_ab(Path::new(&args[1]), Path::new(dir), preset, 0xDEF1EA7) {
            Ok(rep) => println!("ab ok: {rep:?}"),
            Err(e) => eprintln!("ab failed: {e}"),
        }
    }
}
