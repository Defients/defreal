# Using DefReal with RVC (or any voice changer)

DefReal needs nothing from RVC — no plugins, no special builds. You just route
RVC's output through DefReal using normal Windows audio devices, exactly like
you'd route any audio between apps.

```
 mic ──→ RVC ──→ (virtual cable A) ──→ DefReal ──→ (virtual cable B) ──→ OBS / Discord
       WHO you sound like            WHERE/HOW you sound
```

## What you need

- Two virtual cables. If you use VB-Audio: `CABLE-A` and `CABLE-B`
  (or `CABLE` + `CABLE-A`, etc.). VoiceMeeter users can use VAIO/AUX buses.
- RVC (or any real-time voice converter) outputting audio.

## Wiring

1. **RVC**: set the output/monitor device to `CABLE-A Input (VB-Audio)`.
   RVC's converted voice now flows into cable A.
2. **DefReal → ROUTING**:
   - Capture from: `CABLE-A Output (VB-Audio)` — the *other end* of cable A.
   - Send to: `CABLE-B Input (VB-Audio)` — the *input end* of cable B.
   - Press START.
3. **OBS / Discord / the target app**: pick `CABLE-B Output (VB-Audio)` as your
   microphone.

RVC voice in, DefReal-processed voice out. Set REALISM ≈ 30–50 — RVC output is
usually already clean/dry, so mid values add convincing room + mic character
without overprocessing.

## Why two cables?

Cable A carries the raw RVC voice into DefReal. Cable B carries the *processed*
voice to your apps. If you ever connect a cable's input straight back to its own
output you create a loop — Windows won't stop you, your ears will. Keep A ≠ B.

## Hearing yourself

Pick **headphones** as the DefReal output instead of cable B if you just want to
monitor. For processed-out AND monitoring, either:

- Set output = cable B and use the target app's own monitoring (Discord: voice
  settings → input sensitivity / OBS: advanced audio → monitoring), or
- Use VoiceMeeter: send DefReal to a VAIO input, then VoiceMeeter buses it to
  both cable B and your headphones.

## Other sources

The same routing works for anything that can play audio to a device: another AI
voice tool, a soundboard app, a media player — any app's output → a cable's
Input → DefReal's Capture → onward.
