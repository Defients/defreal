# Troubleshooting

## "Access is denied (0x80070005)" when pressing START

Windows refused to open the capture device. Common causes:

- **Windows privacy**: Settings → Privacy & security → Microphone → let desktop
  apps access your microphone.
- **Device held exclusively**: another app (Discord, OBS, a DAW, an RVC process)
  may own the device. Close it or disable "exclusive mode" in the device's
  Windows Sound properties (Advanced tab).
- **Sandboxed/elevated contexts**: some environments block capture endpoints
  entirely while still allowing playback. The DefReal dev environment itself hit
  this — playback streams opened fine, every capture endpoint failed. On a
  normal desktop session this shouldn't occur.
- **WebView2-hosted apps**: the app runs inside WebView2; if you launched it
  from an unusual context (e.g. a service or SSH session), capture can be denied.

DefReal reports the error instead of hanging. Pick another device or fix the OS
block and retry.

## Nothing audible after START

- Check the **OUTPUT meter** — if it moves, DefReal is producing audio and the
  problem is downstream (cable direction, OBS input pick, muted device).
- Virtual cables: *Input* is where audio goes in, *Output* is where apps read it.
  DefReal must capture from the cable's **Output** end and play into a different
  cable's **Input** end. Never route a cable's Input to its own Output — instant loop.
- Press **BYPASS**: if bypass is audible but processed isn't, crank REALISM down
  and check the module toggles — something may be over-attenuating.

## Crackles / dropouts

- ROUTING → raise **Pre-buffer** (e.g. 12 → 20–40 ms).
- DIAGNOSTICS → watch **Output underflows** / **Input overflows**. Nonzero and
  growing = buffer problem.
- Close other high-CPU apps; DefReal itself uses ~2% of a 5 ms block on a modern
  CPU, but the OS mixer chain adds load.

## Feedback warning on START

You routed input → output in a way that likely feeds back acoustically (e.g. mic
→ speakers in the same room, or a cable looping into itself). Acknowledge and
continue only if you know it's safe — e.g. that "mic" is actually a silent cable.

## App won't launch / blank window

- WebView2 Runtime is required (preinstalled on Windows 10/11).
- Dev builds need the frontend: `cd frontend && npm install` first.

## Sound is identical at all settings

- REALISM = 0 is intentionally transparent. Raise it.
- Check module enabled toggles — all six may be off.
- In bypass mode everything is ignored.
