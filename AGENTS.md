# DefReal — agent notes

## Build / test / verify

```bash
cargo test --workspace          # 64 DSP unit + 9 audio pipeline tests
cargo build -p defreal-app      # dev app build
cargo run -p defreal-app        # launch dev app (needs frontend/node_modules)
cd frontend && npm install && npm run build   # typecheck + bundle → frontend/dist
frontend/node_modules/.bin/tauri build        # release exe + NSIS/MSI installers
cargo run -p defreal-dsp --release --example bench    # DSP cost: ~100µs/5ms block
cargo run -p defreal-audio --example render -- in.wav out.wav "Lived-In Room" ab_dir
cargo run -p defreal-audio --example probe            # list/inspect real devices
```

## Environment gotchas

- Windows x64 MSVC. WebView2 required at runtime (present on this machine, v156).
- This dev environment BLOCKS all audio capture endpoints with
  `E_ACCESSDENIED (0x80070005)` — playback streams open fine. Don't chase it;
  it's OS-level sandboxing, not a code bug. Duplex path is covered by
  `crates/defreal-audio/tests/pipeline.rs` (synthetic Pump).
- Frontend: Tailwind v4 (`@import "tailwindcss"` + `@tailwindcss/vite`, no
  config file). Tauri command args are camelCase on the JS side.
- App data: `%APPDATA%/DefReal/` (config.json + presets/*.json).

## Conventions

- Telemetry values must come from the engine — never fake meters/numbers in UI.
- Internal DSP rate: 48 kHz mono → stereo out. `realism = 0` must stay
  bit-transparent (chain test asserts it).
- Keep seeded determinism: pass seeds through (motion, ambience) rather than
  using entropy at runtime.
