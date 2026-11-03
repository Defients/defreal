import { useStore } from "../store";

function Row(props: { k: string; v: string; warn?: boolean }) {
  return (
    <div className="flex justify-between py-1 border-b border-[var(--color-edge)] last:border-0">
      <span className="text-[11px] text-faint">{props.k}</span>
      <span className={`text-[11px] mono ${props.warn ? "text-[var(--color-amber)]" : "text-dim"}`}>
        {props.v}
      </span>
    </div>
  );
}

export function DiagnosticsTab() {
  const t = useStore((s) => s.telemetry);
  const devices = useStore((s) => s.devices);
  const devicesError = useStore((s) => s.devicesError);
  const running = useStore((s) => s.running);

  return (
    <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
      <div className="panel p-4">
        <div className="panel-title mb-2">Engine telemetry</div>
        {t ? (
          <>
            <Row k="State" v={t.state} warn={t.state === "failed"} />
            <Row k="Input rate" v={`${t.in_rate} Hz · ${t.in_channels}ch`} />
            <Row k="Output rate" v={`${t.out_rate} Hz · ${t.out_channels}ch`} />
            <Row k="Est. total latency" v={`${t.est_latency_ms.toFixed(1)} ms`} />
            <Row k="DSP avg / max" v={`${t.proc_avg_us.toFixed(0)} µs / ${t.proc_max_us.toFixed(0)} µs per block`} />
            <Row k="Input buffer fill" v={`${(t.in_fill * 100).toFixed(0)}%`} warn={t.in_fill > 0.85 || t.in_fill < 0.1} />
            <Row k="Output buffer fill" v={`${(t.out_fill * 100).toFixed(0)}%`} warn={t.out_fill > 0.85 || t.out_fill < 0.1} />
            <Row k="SRC drift correction" v={`${t.resample_drift_ppm.toFixed(1)} ppm`} />
            <Row k="Input overflows" v={`${t.input_overflows}`} warn={t.input_overflows > 0} />
            <Row k="Output underflows" v={`${t.output_underflows}`} warn={t.output_underflows > 0} />
            <Row k="Limiter gain" v={`${t.limiter_gain_db.toFixed(1)} dB`} warn={t.limiter_gain_db < -1} />
            <Row k="Speech gate" v={`${t.speech_open ? "open" : "closed"} (${(t.speech_level * 100).toFixed(0)}%)`} />
            <Row k="Mic comp / hum comp" v={`${t.mic_comp_db.toFixed(1)} dB / ${t.hum_comp_db.toFixed(1)} dB`} />
            {t.error && <Row k="Last error" v={t.error} warn />}
          </>
        ) : (
          <div className="text-[11px] text-faint">
            {running ? "Waiting for telemetry…" : "Engine stopped — telemetry appears here when running."}
          </div>
        )}
      </div>

      <div className="panel p-4">
        <div className="panel-title mb-2">Devices ({devices.inputs.length} in / {devices.outputs.length} out)</div>
        {devicesError && <div className="text-[11px] text-[var(--color-red)] mb-2">{devicesError}</div>}
        <div className="max-h-80 overflow-y-auto text-[10px] mono space-y-1">
          {devices.inputs.map((d) => (
            <div key={`i${d.name}`} className="text-dim">
              <span className="text-[var(--color-cyan)]">IN </span> {d.name}
              <span className="text-faint"> — {d.default_channels}ch @{d.default_rate}Hz</span>
            </div>
          ))}
          {devices.outputs.map((d) => (
            <div key={`o${d.name}`} className="text-dim">
              <span className="text-[var(--color-violet)]">OUT</span> {d.name}
              <span className="text-faint"> — {d.default_channels}ch @{d.default_rate}Hz</span>
            </div>
          ))}
        </div>
        <p className="text-[10px] text-faint mt-3 leading-relaxed">
          Note: Windows may block capture for some devices (exclusive mode, privacy settings, or
          sandbox restrictions). If <span className="mono">engine_start</span> reports
          "Access is denied", try another input device or check Windows Settings → Privacy →
          Microphone. Playback-only outputs are not affected.
        </p>
      </div>
    </div>
  );
}
