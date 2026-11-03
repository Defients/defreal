# DefReal Architecture

## Crates

```
crates/defreal-dsp     pure DSP, no OS deps — fully unit-testable
crates/defreal-audio   cpal I/O, ring buffers, resampling, engine thread, offline render
src-tauri              Tauri 2 shell: commands, config/preset persistence, telemetry events
frontend               React 19 + Tailwind 4 + Zustand, Vite
```

## Signal path (live engine)

```
cpal input callback (device rate, any channel count)
  → mono fold → lock-free ring buffer
    → engine thread (5 ms blocks):
        rubato SRC → internal 48 kHz mono
        → DspChain::process() → stereo @ 48k
        → rubato SRC → output device rate
        → lock-free ring buffer
          → cpal output callback (device rate, native channels)
```

- Device callbacks never block, never allocate, never lock — they only
  push/pop ring-buffer samples. All DSP lives on the engine thread.
- **Drift**: the output resampler tracks ring-buffer fill and slews its ratio
  ±PPM to compensate for unsynchronized device clocks.
- Internal rate is always 48 kHz mono; stereo widening happens inside Room Space
  / Ambience. `process_to_mono` exists for mono outputs.
- **Telemetry**: every value the UI shows is a real atomic published by the
  engine (meters, buffer fills, SRC drift, per-block DSP time, latency estimate,
  motion state). Nothing is simulated.

## DSP chain (order)

```
input gain → Micprint → Humanize → Proximity (direct path, position-driven)
  → mix with Roomspace send return → + Ambience texture
  → output gain → safety limiter → wet/dry (realism × master)
```

The dry path is the unmodified input after input gain, so `realism = 0` is
bit-transparent (there is a test asserting this).

## Realism macro

`realism::apply()` maps the 0–1 REALISM knob into the *effective* module params
with per-parameter curves (some params scale linearly, some ease in late — e.g.
noise only appears where realism > ~15%). `wet_dry()` gives the master crossfade.
User params are the "100%" endpoint; the macro attenuates toward neutral as the
knob drops.

## Natural Motion Engine

- Seeded RNG → deterministic, reproducible behaviour (same seed = same moves).
- States: Neutral / LeanIn / LeanBack / TurnAside / Reposition / ReturnToNeutral.
- Dwell-then-transition model with cooldowns; pending moves wait for the speech
  gate (VAD) to close when `speech_aware` is on, so jumps land in pauses.
- All position output is a smooth position lerp — distance, azimuth, facing —
  consumed coherently by Proximity (direct path) and Roomspace (send), so a lean
  changes level, tone, ER timing, and reverb send together.
- Manual mode holds a user position; automation resumes on release.

## Presets

Versioned JSON (`version: 1`) containing the full `ChainParams`. Factories ship
compiled-in; user presets go to `%APPDATA%/DefReal/presets/*.json`.
Validation clamps every field to its legal range — a hand-edited or future-file
can never produce NaN/unsafe params (tested).

## Persistence

`%APPDATA%/DefReal/config.json` — last input/output device names, block
size, pre-buffer, last preset, reduced-motion flag. Restored on launch;
missing devices simply show as unselected.

## Safety

- Hard bypass: output = input, no DSP touched. Instant.
- Safety limiter after the mix: output can never clip regardless of settings
  (tested at realism=1 with hot input).
- Feedback guard: suspicious input→output pairings trigger an explicit warning
  before the engine starts.
- Engine `stop()` never joins a blocked stream-open forever — bounded shutdown.

## Offline render

`defreal_audio::offline` runs the identical `DspChain` on file input (any common
rate/channels → 48 kHz mono → stereo out). A/B writes three files; the matched
file is gain-compensated to the original's RMS for fair comparison.

## Known limitations / tradeoffs

- **No real-capture verification yet** in the dev environment (OS blocked all
  capture endpoints). Duplex logic is covered by synthetic pump tests + a
  verified playback stream.
- Motion azimuth/facing influence tone+direction subtly but the UI proximity
  view currently visualizes distance primarily.
- Wet/dry crossfade is level-compensated coarsely (equal-power-ish), not
  perfectly loudness-matched at every knob position.
- No per-device latency reporting from the driver; latency estimate is
  (buffers + SRC lookahead + prebuffer), not measured loopback.
