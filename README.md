# DefReal

**Artificial Voice. Natural Presence.**

> *"RVC changes WHO you sound like. DefReal changes WHERE and HOW you sound."*

DefReal is a standalone Windows desktop app that turns clinically-clean real-time
voice — AI-converted speech from RVC, a sterile USB mic, or anything routed through
a virtual cable — into audio that sounds like it was actually recorded by a real
microphone in a real room. Perfectly Imperfect Audio.

It is **not** a voice changer and does **not** require RVC. No models, no GPU, no
cloud, no accounts, no API keys. It's a general-purpose real-time acoustic realism
processor: mic → DefReal → headphones / speakers / OBS / Discord / WAV file.

## What it actually does

The chain, applied live at 48 kHz internal rate:

| Module | What it does |
|---|---|
| **Mic Print** | Mic coloration: high/low-pass voicing, presence shelf, proximity bass, soft saturation, gentle compression. Profiles: Neutral, Clean Desk Mic, Casual Headset, Webcam Mic, Warm Broadcast. |
| **Humanize** | Recordist dynamics: gentle compression, noise-floor expander, slow gain "wander" (mic-technique drift), mild saturation. |
| **Proximity** | Speaker-to-mic distance: inverse-square level, air-absorption low-pass, off-axis darkening, room-send coupling. Driven by the motion engine or manual position. |
| **Room Space** | Early reflections + feedback-delay reverb tail. Rooms: Bedroom, Home Office, Treated Studio, Small Living Room. |
| **Ambience** | The noise floor that makes it real: room tone, computer fan, ventilation, electric hum — seeded/deterministic. |
| **Natural Motion** | The signature feature: a seeded state machine (Neutral → Lean In → Lean Back → Turn Aside → Reposition → Return) that moves the *virtual speaker position* in a physically-plausible way — preferably during speech pauses — and drives every module coherently. |

One **Natural Presence / Realism** macro (0–100) scales the whole system smoothly.
0% is genuinely transparent (bit-transparent dry path); 100% is full processing.
Hard **bypass** is instant and click-free.

## Quick start

```bash
# prerequisites: Rust (x86_64-pc-windows-msvc), Node 20+, WebView2 (preinstalled on Win10/11)
cd frontend && npm install
cd .. && cargo run -p defreal-app            # dev build → launches the app
```

In the app:
1. **ROUTING** tab → pick a capture device (input) and a playback device (output).
2. Press **START**. Speak into the source; hear the processed result on the output.
3. Turn the **REALISM** dial — 30–60 is the sweet spot.
4. Watch the proximity view as Natural Motion moves the virtual speaker.
5. **BYPASS** A/Bs instantly.

See **[docs/QUICKSTART.md](docs/QUICKSTART.md)** for routing recipes including
RVC → DefReal → Discord/OBS, and **[docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)**.

## Testing without hardware

The **LAB** tab renders any WAV through the chain offline and produces A/B
comparison files (original / processed / loudness-matched processed).

```bash
# CLI equivalent:
cargo run -p defreal-audio --example render -- in.wav out.wav "Lived-In Room" ab_dir
```

## Verified status (honest)

- ✅ 64 DSP unit tests + 9 synthetic pipeline tests pass (`cargo test --workspace`)
- ✅ Offline render verified: 98% of samples differ from input, level-matched A/B
- ✅ Real device enumeration verified (all inputs/outputs listed)
- ✅ Real **playback** stream verified (opened + ran on Realtek speakers)
- ✅ DSP cost measured: ~100 µs per 5 ms block (~2% of budget), ~50× realtime
- ⚠️ Real **capture** stream could NOT be verified in the dev environment —
  every capture endpoint returned Windows `E_ACCESSDENIED (0x80070005)`
  (environment sandboxing; playback works). The full duplex path is exercised
  end-to-end by the synthetic `Pump` tests, but the first real-mic run on a
  normal desktop is still the true gate.
- ⚠️ The installer's output artifacts are listed in the final report; running
  the installer itself was not performed.

## Layout

```
crates/defreal-dsp    — pure DSP: modules, chain, motion engine, presets
crates/defreal-audio  — cpal devices, duplex engine, SRC+drift, telemetry, offline render
src-tauri             — Tauri 2 app: commands, persistence, telemetry events
frontend              — React 19 + Tailwind 4 "CosmoTech" UI
docs/                 — quickstart, RVC routing, troubleshooting, architecture
scripts/              — icon generator
```

More detail: **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.
