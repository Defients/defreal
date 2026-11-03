# DefReal Quickstart

## 1. Run it

Dev build from source:

```bash
cd frontend && npm install && cd ..
cargo run -p defreal-app
```

Release build + installer:

```bash
cd frontend && npm install && cd ..
frontend/node_modules/.bin/tauri build
# artifacts under src-tauri/target/release/bundle/
```

## 2. First listen (microphone → headphones)

1. Open **ROUTING**.
2. Capture: your mic (e.g. `Microphone (HyperX Quadcast)`).
3. Send to: your headphones.
   ⚠️ If your output is *speakers* while your input is a live mic, you may get
   acoustic feedback — DefReal warns you when the routing looks risky. Prefer
   headphones for the first run.
4. **START** → speak. Set REALISM ≈ 40.
5. Click **BYPASS** on/off to A/B.

## 3. RVC → DefReal → OBS / Discord (virtual cable routing)

You need a virtual audio cable (VB-Audio Virtual Cable or VoiceMeeter — both
already common in RVC setups).

1. In **RVC**, set the output device to `CABLE-A Input (VB-Audio)`.
2. In **DefReal → ROUTING**:
   - Capture from: `CABLE-A Output (VB-Audio)`
   - Send to: `CABLE-B Input (VB-Audio)`  (a *second* cable — do NOT loop back to the same cable)
3. In **OBS/Discord**, select `CABLE-B Output (VB-Audio)` as your mic.

Result: RVC voice → DefReal realism → apps hear the processed signal.

Monitoring yourself: enable **Direct Monitor** on the Routing tab and pick your
real headphones — the processed signal plays on the cable AND your headphones
at once. Monitor gain starts at −18 dB; its mute/volume are independent of the
main output. Keep the DefReal→OBS cable separate from the RVC→DefReal cable.

## 4. Live Presence Stage

- On the **PERFORM** tab, the stage shows the mic (violet diamond, bottom) and
  your virtual speaker (cyan dot). Distance rings are 0.25 / 0.5 / 1 / 1.5 / 2 m.
- **Drag the dot**: closer = more close-mic presence, farther = more room,
  sideways = off-axis coloration. Changes are audible immediately.
- **Arrow keys** nudge the speaker; **Home** returns to neutral.
- **AUTO**: automation roams; dragging temporarily overrides, release glides
  back. **TOUCH**: same but always returns on release. **LATCH**: the speaker
  stays where you drop it until you resume automation.
- **Actions** — Lean In / Lean Back / Turn Away / Across Room / Return /
  Freeze: one-tap acoustic moves on the same position the dot shows.
- **Snapshots** — Intimate / Conversational / Distant + your own (*save as…*).
  Recall glides to the position; it never touches your mic model, ambience,
  or routing.
- The graph below shows distance over the last 30 s.
- Motion trigger buttons (lean-in, lean-back, turn-aside, reposition) queue a
  move; with *Speech-aware* on, the move waits for a pause in your voice.

## 5. Presets

- Factory presets (e.g. **Natural Presence**, **Lived-In Room**) can't be
  overwritten — tweak them, then **Save preset** as your own.
- User presets live in `%APPDATA%/DefReal/presets/` as JSON; import /
  export from the **PRESETS** tab.
- Last-used preset + device routing restore on launch.

## 6. LAB (offline render + A/B)

- **Offline render**: input WAV → processed WAV through any preset.
  Deterministic — same seed, same output.
- **A/B comparison**: writes `a_original.wav`, `b_processed.wav`, and
  `b_processed_matched.wav` (loudness-matched) to a folder. Import all three
  into a player/DAW and flip between them — the matched file removes the
  "louder sounds better" bias.
