import React from "react";
import { useStore } from "../store";
import { Slider, Toggle, Select } from "./widgets";
import type { AmbienceTexture, MicProfile, PositionMode, RoomProfile } from "../types";

function ModuleCard(props: {
  title: string;
  desc: string;
  enabled: boolean;
  onToggle: (v: boolean) => void;
  children: React.ReactNode;
}) {
  return (
    <div className={`panel p-4 ${props.enabled ? "" : "opacity-55"}`}>
      <div className="flex items-center justify-between mb-3">
        <div>
          <div className="text-[13px] font-semibold">{props.title}</div>
          <div className="text-[10px] text-faint mt-0.5">{props.desc}</div>
        </div>
        <span
          className={`toggle ${props.enabled ? "on" : ""}`}
          role="switch"
          aria-checked={props.enabled}
          aria-label={`${props.title} enabled`}
          tabIndex={0}
          onClick={() => props.onToggle(!props.enabled)}
          onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && props.onToggle(!props.enabled)}
        />
      </div>
      <div className="space-y-2.5" style={{ pointerEvents: props.enabled ? "auto" : "none" }}>
        {props.children}
      </div>
    </div>
  );
}

export function ModulePanels() {
  const params = useStore((s) => s.params);
  const setParam = useStore((s) => s.setParam);
  const triggerMotion = useStore((s) => s.triggerMotion);
  const setManual = useStore((s) => s.setManual);
  const releaseManual = useStore((s) => s.releaseManual);
  const t = useStore((s) => s.telemetry);
  const [manualD, setManualD] = React.useState(0.45);
  const [manualAz, setManualAz] = React.useState(0);
  const [manualFacing, setManualFacing] = React.useState(0);

  if (!params) return null;
  const p = params;

  const manualActive = p.proximity.mode === "manual";

  return (
    <div className="grid grid-cols-1 xl:grid-cols-2 gap-3">
      <ModuleCard
        title="Mic Print"
        desc="Microphone coloration — body, presence, proximity, saturation"
        enabled={p.micprint.enabled}
        onToggle={(v) => setParam("micprint.enabled", v)}
      >
        <Select<MicProfile>
          label="Profile"
          value={p.micprint.profile}
          onChange={(v) => setParam("micprint.profile", v)}
          options={[
            { value: "neutral", label: "Neutral" },
            { value: "clean-desk-mic", label: "Clean Desk Mic" },
            { value: "casual-headset", label: "Casual Headset" },
            { value: "webcam-mic", label: "Webcam Mic" },
            { value: "warm-broadcast", label: "Warm Broadcast" },
          ]}
        />
        <Slider label="Presence" value={p.micprint.presence_db} min={-9} max={9} step={0.1} unit=" dB" onChange={(v) => setParam("micprint.presence_db", v)} />
        <Slider label="Proximity bass" value={p.micprint.proximity_db} min={-6} max={9} step={0.1} unit=" dB" onChange={(v) => setParam("micprint.proximity_db", v)} />
        <Slider label="Saturation" value={p.micprint.saturation} min={0} max={1} step={0.01} onChange={(v) => setParam("micprint.saturation", v)} />
        <Slider label="Compression" value={p.micprint.compression} min={0} max={1} step={0.01} onChange={(v) => setParam("micprint.compression", v)} />
        <Slider label="Color amount" value={p.micprint.color_amount} min={0} max={1} step={0.01} onChange={(v) => setParam("micprint.color_amount", v)} />
      </ModuleCard>

      <ModuleCard
        title="Proximity"
        desc="Speaker-to-mic distance, angle, early reflections"
        enabled={p.proximity.enabled}
        onToggle={(v) => setParam("proximity.enabled", v)}
      >
        <Select<PositionMode>
          label="Position mode"
          value={p.proximity.mode}
          onChange={(v) => setParam("proximity.mode", v)}
          options={[
            { value: "auto", label: "Auto (Natural Motion)" },
            { value: "manual", label: "Manual" },
          ]}
        />
        {manualActive ? (
          <>
            <Slider label="Distance" value={manualD} min={0.1} max={2.5} step={0.01} unit=" m" violet onChange={(v) => { setManualD(v); setManual(v, manualAz, manualFacing); }} />
            <Slider label="Azimuth" value={manualAz} min={-90} max={90} step={1} unit="°" violet onChange={(v) => { setManualAz(v); setManual(manualD, v, manualFacing); }} />
            <Slider label="Facing" value={manualFacing} min={-90} max={90} step={1} unit="°" violet onChange={(v) => { setManualFacing(v); setManual(manualD, manualAz, v); }} />
            <button className="btn text-[11px]" onClick={() => releaseManual()}>Release → Auto</button>
          </>
        ) : (
          <div className="text-[10px] text-faint">
            Engine: <span className="mono text-dim">{t?.motion_state ?? "—"}</span>
            {" · "}
            <span className="mono text-dim">{(t?.motion_distance_m ?? 0).toFixed(2)} m</span>
          </div>
        )}
        <Slider label="Direct trim" value={p.proximity.direct_trim_db} min={-6} max={6} step={0.1} unit=" dB" onChange={(v) => setParam("proximity.direct_trim_db", v)} />
        <Slider label="Room coupling" value={p.proximity.room_coupling} min={0} max={1} step={0.01} onChange={(v) => setParam("proximity.room_coupling", v)} />
      </ModuleCard>

      <ModuleCard
        title="Room Space"
        desc="Early reflections + reverb tail for a believable room"
        enabled={p.roomspace.enabled}
        onToggle={(v) => setParam("roomspace.enabled", v)}
      >
        <Select<RoomProfile>
          label="Room"
          value={p.roomspace.room}
          onChange={(v) => setParam("roomspace.room", v)}
          options={[
            { value: "bedroom", label: "Bedroom" },
            { value: "home-office", label: "Home Office" },
            { value: "treated-studio", label: "Treated Studio" },
            { value: "small-living-room", label: "Small Living Room" },
          ]}
        />
        <Slider label="Level" value={p.roomspace.level} min={0} max={1} step={0.01} onChange={(v) => setParam("roomspace.level", v)} />
        <Slider label="Size" value={p.roomspace.size} min={0.5} max={1.6} step={0.01} unit="×" onChange={(v) => setParam("roomspace.size", v)} />
        <Slider label="Decay" value={p.roomspace.decay_scale} min={0.3} max={2.5} step={0.01} unit="×" onChange={(v) => setParam("roomspace.decay_scale", v)} />
        <Slider label="Early refl." value={p.roomspace.er_level} min={0} max={1} step={0.01} onChange={(v) => setParam("roomspace.er_level", v)} />
        <Slider label="Tail" value={p.roomspace.tail_level} min={0} max={1} step={0.01} onChange={(v) => setParam("roomspace.tail_level", v)} />
        <Slider label="Damping" value={p.roomspace.damping_hz} min={2000} max={16000} step={100} unit=" Hz" onChange={(v) => setParam("roomspace.damping_hz", v)} />
      </ModuleCard>

      <ModuleCard
        title="Ambience"
        desc="Room tone, fan noise, hum — the floor that makes it real"
        enabled={p.ambience.enabled}
        onToggle={(v) => setParam("ambience.enabled", v)}
      >
        <Select<AmbienceTexture>
          label="Texture"
          value={p.ambience.texture}
          onChange={(v) => setParam("ambience.texture", v)}
          options={[
            { value: "none", label: "None" },
            { value: "room-tone", label: "Room Tone" },
            { value: "computer-fan", label: "Computer Fan" },
            { value: "ventilation", label: "Ventilation" },
            { value: "electric-hum", label: "Electric Hum" },
          ]}
        />
        <Slider label="Level" value={p.ambience.level_db} min={-80} max={-20} step={0.5} unit=" dB" onChange={(v) => setParam("ambience.level_db", v)} />
        <Slider label="Brightness" value={p.ambience.brightness} min={0} max={1} step={0.01} onChange={(v) => setParam("ambience.brightness", v)} />
        <button
          className="btn text-[11px]"
          onClick={() => setParam("ambience.seed", Math.floor(Math.random() * 2147483647))}
        >
          Reseed noise ({p.ambience.seed})
        </button>
      </ModuleCard>

      <ModuleCard
        title="Humanize"
        desc="Recordist dynamics — drift, compression, noise floor"
        enabled={p.humanize.enabled}
        onToggle={(v) => setParam("humanize.enabled", v)}
      >
        <Slider label="Compression" value={p.humanize.compression} min={0} max={1} step={0.01} onChange={(v) => setParam("humanize.compression", v)} />
        <Slider label="Expander" value={p.humanize.expander} min={0} max={1} step={0.01} onChange={(v) => setParam("humanize.expander", v)} />
        <Slider label="Gain wander" value={p.humanize.wander} min={0} max={1} step={0.01} onChange={(v) => setParam("humanize.wander", v)} />
        <Slider label="Saturation" value={p.humanize.saturation} min={0} max={1} step={0.01} onChange={(v) => setParam("humanize.saturation", v)} />
      </ModuleCard>

      <ModuleCard
        title="Natural Motion"
        desc="Speech-aware speaker movement — DefReal's signature"
        enabled={p.motion.enabled}
        onToggle={(v) => setParam("motion.enabled", v)}
      >
        <Slider label="Activity" value={p.motion.activity} min={0} max={1} step={0.01} violet onChange={(v) => setParam("motion.activity", v)} />
        <Slider label="Range" value={p.motion.range} min={0} max={1} step={0.01} violet onChange={(v) => setParam("motion.range", v)} />
        <Slider label="Speed" value={p.motion.speed} min={0.5} max={2} step={0.01} unit="×" violet onChange={(v) => setParam("motion.speed", v)} />
        <Toggle label="Speech-aware" on={p.motion.speech_aware} onChange={(v) => setParam("motion.speech_aware", v)} title="Tie movement to voice activity" />
        <div className="flex flex-wrap gap-1.5 pt-1">
          {["lean-in", "lean-back", "turn-aside", "reposition", "return-to-neutral"].map((m) => (
            <button key={m} className="btn text-[10px]" onClick={() => triggerMotion(m)}>
              {m}
            </button>
          ))}
        </div>
      </ModuleCard>
    </div>
  );
}
