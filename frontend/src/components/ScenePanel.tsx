import { useStore } from "../store";
import type { EmitterCat } from "../types";

const CATS: { id: EmitterCat; label: string; hint: string }[] = [
  { id: "Fan", label: "Computer fan", hint: "continuous soft airflow" },
  { id: "Vent", label: "Ventilation", hint: "room air texture" },
  { id: "Traffic", label: "Distant traffic", hint: "exterior rumble — put it in the hallway" },
  { id: "Keys", label: "Keyboard", hint: "short typing bursts" },
  { id: "Chair", label: "Office chair", hint: "occasional creaks" },
  { id: "Desk", label: "Desk contact", hint: "taps and bumps" },
  { id: "Footsteps", label: "Footsteps", hint: "short walks — try the hallway" },
];

const ACTS = ["Quiet", "Natural", "Busy"] as const;

/** SOUNDSTAGE panel: enable, door, add/list/mute/remove emitters. */
export function ScenePanel() {
  const scene = useStore((s) => s.scene);
  const emitters = useStore((s) => s.emitters);
  const setScene = useStore((s) => s.setScene);
  const addEmitter = useStore((s) => s.addEmitter);
  const updateEmitter = useStore((s) => s.updateEmitter);
  const removeEmitter = useStore((s) => s.removeEmitter);
  const tel = useStore((s) => s.telemetry);

  return (
    <div className="panel">
      <div className="flex items-center justify-between mb-2">
        <div className="panel-title">SoundStage — Room Environment</div>
        <button
          className={`btn text-[10px] ${scene.enabled ? "btn-active" : ""}`}
          onClick={() => setScene({ enabled: !scene.enabled })}
        >
          {scene.enabled ? "ON" : "OFF"}
        </button>
      </div>

      {scene.enabled && (
        <>
          <div className="flex items-center gap-2 mb-2">
            <span className="text-[10px] text-dim w-16">Door</span>
            <button
              className={`btn text-[10px] ${scene.door_open > 0.5 ? "btn-active" : ""}`}
              onClick={() => setScene({ door_open: 1 })}
            >
              Open
            </button>
            <button
              className={`btn text-[10px] ${scene.door_open <= 0.5 ? "btn-active" : ""}`}
              onClick={() => setScene({ door_open: 0 })}
            >
              Closed
            </button>
            <span className="text-[9px] text-faint">
              click the door on the stage to toggle
            </span>
          </div>

          <div className="text-[10px] text-dim mb-1">Add sound source</div>
          <div className="flex flex-wrap gap-1.5 mb-2">
            {CATS.map((c) => (
              <button
                key={c.id}
                className="btn text-[10px]"
                title={c.hint}
                disabled={emitters.length >= 8}
                onClick={() => addEmitter(c.id, c.id === "Traffic" || c.id === "Footsteps" ? 2.2 : 0.9, 0.4)}
              >
                + {c.label}
              </button>
            ))}
          </div>

          {emitters.length > 0 && (
            <div className="border-t border-[var(--color-edge)] pt-2 flex flex-col gap-1.5">
              {emitters.map((e) => {
                const live = tel?.emitters.find((t) => t.id === e.id);
                return (
                  <div key={e.id} className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px]">
                    <span className={e.muted ? "text-faint line-through w-16" : "text-dim w-16"}>
                      {e.cat}
                    </span>
                    <span className="text-faint w-14">{live?.room ?? "—"}</span>
                    <div className="flex gap-0.5">
                      {ACTS.map((a) => (
                        <button
                          key={a}
                          className={`btn text-[9px] px-1 ${e.activity === a ? "btn-active" : ""}`}
                          onClick={() => updateEmitter(e.id, { activity: a })}
                        >
                          {a[0]}
                        </button>
                      ))}
                    </div>
                    <input
                      type="range"
                      min={-48}
                      max={-6}
                      step={1}
                      value={e.gain_db}
                      className="w-16"
                      title={`${e.gain_db} dB`}
                      onChange={(ev) => updateEmitter(e.id, { gain_db: Number(ev.target.value) })}
                    />
                    <button
                      className={`btn text-[9px] px-1.5 ${e.muted ? "btn-active" : ""}`}
                      onClick={() => updateEmitter(e.id, { muted: !e.muted })}
                    >
                      mute
                    </button>
                    <button
                      className="btn text-[9px] px-1.5 btn-danger"
                      onClick={() => removeEmitter(e.id)}
                    >
                      ×
                    </button>
                  </div>
                );
              })}
            </div>
          )}
          {emitters.length === 0 && (
            <div className="text-[9px] text-faint">
              No emitters yet — add one, then drag it on the stage. Sources past
              the wall are in the hallway; the door controls how much gets through.
            </div>
          )}
        </>
      )}
    </div>
  );
}
