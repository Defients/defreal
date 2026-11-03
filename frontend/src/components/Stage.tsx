import React, { useEffect, useRef } from "react";
import { useStore } from "../store";
import { api } from "../ipc";

// stage geometry: mic at bottom-center; speaker roams the upper half-disc.
// mapping: screen angle -> azimuth (-70..70 deg), radius -> distance (0.1..2.4m)
const AZ_MAX = 70;
const D_MIN = 0.12;
const D_MAX = 2.4;
const THROTTLE_MS = 25; // ~40 Hz position sends while dragging

function toPos(px: number, py: number, cx: number, cy: number, scale: number) {
  const dx = px - cx;
  const dy = cy - py; // up is positive
  const distPx = Math.hypot(dx, dy);
  let az = (Math.atan2(dx, dy) * 180) / Math.PI; // 0 = straight ahead (up)
  az = Math.max(-AZ_MAX, Math.min(AZ_MAX, az));
  let d = distPx / scale;
  d = Math.max(D_MIN, Math.min(D_MAX, d));
  return { d, az };
}

function toXY(d: number, az: number, cx: number, cy: number, scale: number) {
  const a = (az * Math.PI) / 180;
  return [cx + Math.sin(a) * d * scale, cy - Math.cos(a) * d * scale] as const;
}

// ROOMLOGIC scene coords (meters, mic at origin, hallway at x>1.5)
const WALL_X = 1.5;
function sceneToXY(x: number, y: number, cx: number, cy: number, scale: number) {
  return [cx + y * scale, cy - x * scale] as const;
}
function xyToScene(px: number, py: number, cx: number, cy: number, scale: number) {
  return { x: (cy - py) / scale, y: (px - cx) / scale };
}

/** Interactive Live Presence Stage: draggable speaker vs. fixed mic. */
export function Stage() {
  const ref = useRef<HTMLCanvasElement>(null);
  const dragRef = useRef<{ active: boolean; lastSend: number; dog: boolean; emit: number | null }>({ active: false, lastSend: 0, dog: false, emit: null });
  const posRef = useRef({ d: 0.32, az: 0, f: 0 });
  const t = useStore((s) => s.telemetry);
  const motionMode = useStore((s) => s.motionMode);
  const stageDragStart = useStore((s) => s.stageDragStart);
  const stageDragMove = useStore((s) => s.stageDragMove);
  const stageDragEnd = useStore((s) => s.stageDragEnd);
  const setMotionMode = useStore((s) => s.setMotionMode);
  const acousticAction = useStore((s) => s.acousticAction);
  const snapshots = useStore((s) => s.snapshots);
  const recallSnapshot = useStore((s) => s.recallSnapshot);
  const saveSnapshot = useStore((s) => s.saveSnapshot);
  const deleteSnapshot = useStore((s) => s.deleteSnapshot);
  const reducedMotion = useStore((s) => s.reducedMotion);
  const [snapName, setSnapName] = React.useState("");

  const W = 460;
  const H = 340;
  const cx = W / 2;
  const cy = H - 52;
  const scale = 110;

  // current displayed position (UI-side; engine truth renders in canvas loop)
  const cur = () => posRef.current;

  const sendPos = (d: number, az: number) => {
    const now = performance.now();
    if (now - dragRef.current.lastSend < THROTTLE_MS) return;
    dragRef.current.lastSend = now;
    if (dragRef.current.emit != null) {
      return; // emitter drags use scene coords, handled in onPointerMove
    }
    if (dragRef.current.dog) {
      useStore.getState().dogDragMove(d, az);
    } else {
      stageDragMove(d, az);
      api.motionSetManual(d, az, posRef.current.f).catch(() => {});
    }
  };

  const pointerPos = (e: React.PointerEvent) => {
    const r = ref.current!.getBoundingClientRect();
    const x = ((e.clientX - r.left) / r.width) * W;
    const y = ((e.clientY - r.top) / r.height) * H;
    return toPos(x, y, cx, cy, scale);
  };

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    const st = useStore.getState();
    const r = ref.current!.getBoundingClientRect();
    const px = ((e.clientX - r.left) / r.width) * W;
    const py = ((e.clientY - r.top) / r.height) * H;
    // arbitration: door toggle > emitter drag > dog drag > speaker drag
    const tel = st.telemetry;
    const wallY = cy - WALL_X * scale;
    const doorL = cx - 0.45 * scale;
    const doorR = cx + 0.45 * scale;
    if (Math.abs(py - wallY) < 9 && px > doorL - 6 && px < doorR + 6 && st.scene.enabled) {
      st.setScene({ door_open: st.scene.door_open > 0.5 ? 0 : 1 });
      return;
    }
    for (const em of tel?.emitters ?? []) {
      const [ex, ey] = sceneToXY(em.x, em.y, cx, cy, scale);
      if (Math.hypot(px - ex, py - ey) < 16) {
        dragRef.current.emit = em.id;
        dragRef.current.active = true;
        return;
      }
    }
    let dogHit = false;
    if (st.dog.enabled && tel?.dog_enabled) {
      const [dx2, dy2] = toXY(tel.dog_distance_m, tel.dog_azimuth_deg, cx, cy, scale);
      dogHit = Math.hypot(px - dx2, py - dy2) < 20;
    }
    dragRef.current.dog = dogHit;
    dragRef.current.active = true;
    if (dogHit) {
      st.dogDragStart();
    } else {
      stageDragStart();
    }
    const p = pointerPos(e);
    posRef.current.d = p.d;
    posRef.current.az = p.az;
    sendPos(p.d, p.az);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragRef.current.active) return;
    if (dragRef.current.emit != null) {
      const now = performance.now();
      if (now - dragRef.current.lastSend < THROTTLE_MS) return;
      dragRef.current.lastSend = now;
      const r = ref.current!.getBoundingClientRect();
      const px = ((e.clientX - r.left) / r.width) * W;
      const py = ((e.clientY - r.top) / r.height) * H;
      const sp = xyToScene(px, py, cx, cy, scale);
      useStore.getState().updateEmitter(dragRef.current.emit, { x: sp.x, y: sp.y });
      return;
    }
    const p = pointerPos(e);
    posRef.current.d = p.d;
    posRef.current.az = p.az;
    sendPos(p.d, p.az);
  };
  const onPointerUp = () => {
    if (!dragRef.current.active) return;
    const wasDog = dragRef.current.dog;
    const wasEmit = dragRef.current.emit;
    dragRef.current.active = false;
    dragRef.current.dog = false;
    dragRef.current.emit = null;
    if (wasEmit != null) return;
    if (wasDog) {
      useStore.getState().dogDragEnd();
    } else {
      stageDragEnd();
    }
  };
  const nudge = (dd: number, daz: number) => {
    posRef.current.d = Math.max(D_MIN, Math.min(D_MAX, posRef.current.d + dd));
    posRef.current.az = Math.max(-AZ_MAX, Math.min(AZ_MAX, posRef.current.az + daz));
    api.motionSetManual(posRef.current.d, posRef.current.az, posRef.current.f).catch(() => {});
  };

  // render loop — applied state from telemetry unless mid-drag
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d")!;
    let raf = 0;

    const draw = () => {
      const st = useStore.getState();
      const tel = st.telemetry;
      ctx.clearRect(0, 0, W, H);

      // grid
      ctx.strokeStyle = "#15152a";
      for (let x = 0; x < W; x += 40) { ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, H); ctx.stroke(); }
      for (let y = 0; y < H; y += 40) { ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(W, y); ctx.stroke(); }

      // distance rings + azimuth guides
      for (const r of [0.25, 0.5, 1.0, 1.5, 2.0]) {
        ctx.strokeStyle = "#222240";
        ctx.setLineDash([3, 6]);
        ctx.beginPath();
        ctx.arc(cx, cy, r * scale, Math.PI, 0);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillStyle = "#4a4a6e";
        ctx.font = "9px monospace";
        ctx.fillText(`${r}m`, cx + r * scale - 8, cy - 4);
      }
      for (const a of [-60, -30, 0, 30, 60]) {
        const [gx, gy] = toXY(2.2, a, cx, cy, scale);
        ctx.strokeStyle = a === 0 ? "#26264a" : "#1b1b36";
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(gx, gy);
        ctx.stroke();
      }

      // applied (engine) position — or live drag position while dragging
      const appliedD = tel?.motion_distance_m ?? 0.32;
      const appliedAz = tel?.motion_azimuth_deg ?? 0;
      const dragging = dragRef.current.active;
      const d = dragging ? posRef.current.d : appliedD;
      const az = dragging ? posRef.current.az : appliedAz;
      const [sx, sy] = toXY(d, az, cx, cy, scale);

      // target ghost when engine is transitioning and we're not dragging
      if (!dragging && tel && tel.motion_progress < 0.999) {
        const [tx2, ty2] = toXY(tel.motion_target_m, tel.motion_target_azimuth_deg, cx, cy, scale);
        ctx.strokeStyle = "rgba(139,124,246,0.5)";
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.arc(tx2, ty2, 11, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.strokeStyle = "rgba(139,124,246,0.2)";
        ctx.beginPath();
        ctx.moveTo(sx, sy);
        ctx.lineTo(tx2, ty2);
        ctx.stroke();
      }

      // speaker glow + body
      const g = ctx.createRadialGradient(sx, sy, 0, sx, sy, 24);
      g.addColorStop(0, dragging ? "rgba(62,230,216,0.55)" : "rgba(62,230,216,0.35)");
      g.addColorStop(1, "rgba(62,230,216,0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(sx, sy, 24, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = dragging ? "#6ff0e4" : "#3ee6d8";
      ctx.beginPath();
      ctx.arc(sx, sy, 9, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "#0a0a12";
      ctx.lineWidth = 2;
      ctx.stroke();
      // facing tick
      const fAng = ((tel?.motion_facing_deg ?? 0) * Math.PI) / 180;
      ctx.strokeStyle = "#0a0a12";
      ctx.beginPath();
      ctx.moveTo(sx, sy);
      ctx.lineTo(sx + Math.sin(fAng) * 13, sy - Math.cos(fAng) * 13);
      ctx.stroke();

      // dog marker (if enabled): amber paw-dot, size-scaled
      if (st.dog.enabled && tel?.dog_enabled) {
        const dd = dragRef.current.dog && st.dogPos ? st.dogPos.d : tel.dog_distance_m;
        const daz = dragRef.current.dog && st.dogPos ? st.dogPos.az : tel.dog_azimuth_deg;
        const [gx, gy] = toXY(dd, daz, cx, cy, scale);
        const rDog = st.dog.size === "Small" ? 6 : st.dog.size === "Large" ? 11 : 8;
        const dg = ctx.createRadialGradient(gx, gy, 0, gx, gy, rDog * 2.4);
        dg.addColorStop(0, "rgba(232,176,74,0.45)");
        dg.addColorStop(1, "rgba(232,176,74,0)");
        ctx.fillStyle = dg;
        ctx.beginPath();
        ctx.arc(gx, gy, rDog * 2.4, 0, Math.PI * 2);
        ctx.fill();
        // paw-ish: body circle + toe dots
        ctx.fillStyle = "#e8b04a";
        ctx.beginPath();
        ctx.arc(gx, gy, rDog, 0, Math.PI * 2);
        ctx.fill();
        ctx.strokeStyle = "#0a0a12";
        ctx.lineWidth = 1.5;
        ctx.stroke();
        for (const [ox, oy] of [[-rDog * 0.7, -rDog * 0.9], [0, -rDog * 1.1], [rDog * 0.7, -rDog * 0.9]]) {
          ctx.beginPath();
          ctx.arc(gx + ox, gy + oy, rDog * 0.28, 0, Math.PI * 2);
          ctx.fill();
        }
        ctx.fillStyle = "rgba(232,176,74,0.9)";
        ctx.font = "9px monospace";
        const dstate = tel.dog_moving ? "walk" : tel.dog_state;
        ctx.fillText(`${st.dog.size.toLowerCase()} · ${dstate}`, gx + rDog + 4, gy + 3);
        // motion path to target
        if (tel.dog_moving) {
          ctx.strokeStyle = "rgba(232,176,74,0.25)";
          ctx.setLineDash([2, 4]);
          ctx.beginPath();
          ctx.arc(gx, gy, rDog + 4, 0, Math.PI * 2);
          ctx.stroke();
          ctx.setLineDash([]);
        }
      }

      // mic (fixed)
      ctx.save();
      ctx.translate(cx, cy);
      ctx.fillStyle = "#8b7cf6";
      ctx.beginPath();
      ctx.moveTo(0, -12);
      ctx.lineTo(9, 0);
      ctx.lineTo(0, 12);
      ctx.lineTo(-9, 0);
      ctx.closePath();
      ctx.fill();
      ctx.strokeStyle = "#0a0a12";
      ctx.stroke();
      ctx.fillStyle = "rgba(139,124,246,0.15)";
      ctx.beginPath();
      ctx.moveTo(0, 0);
      ctx.arc(0, 0, 36, -Math.PI / 2 - 0.55, -Math.PI / 2 + 0.55);
      ctx.closePath();
      ctx.fill();
      ctx.restore();

      // ROOMLOGIC: wall at x=1.5m -> horizontal line; door = gap segment.
      // (scene x = forward/up on stage, y = lateral)
      if (st.scene.enabled) {
        const wallY = cy - WALL_X * scale;
        const doorL = cx - 0.45 * scale;
        const doorR = cx + 0.45 * scale;
        const doorOpen = tel?.door_open ?? st.scene.door_open;
        ctx.strokeStyle = "#2c2c48";
        ctx.lineWidth = 3;
        ctx.beginPath(); ctx.moveTo(0, wallY); ctx.lineTo(doorL, wallY); ctx.stroke();
        ctx.beginPath(); ctx.moveTo(doorR, wallY); ctx.lineTo(W, wallY); ctx.stroke();
        ctx.lineWidth = 1;
        // door leaf: slides/swings by openness
        ctx.strokeStyle = doorOpen > 0.5 ? "#3ee6d8" : "#e86a6a";
        ctx.lineWidth = 2;
        ctx.beginPath();
        const leaf = doorR - doorL;
        ctx.moveTo(doorL, wallY);
        ctx.lineTo(doorL + leaf * (1 - doorOpen) * 0.92, wallY - leaf * doorOpen * 0.55);
        ctx.stroke();
        ctx.lineWidth = 1;
        ctx.fillStyle = "#8a8aa5";
        ctx.font = "9px monospace";
        ctx.fillText("HALLWAY", 6, wallY - 6);
        ctx.fillText("STUDIO", 6, wallY + 14);
        ctx.fillText(doorOpen > 0.5 ? "door open" : "door closed", doorR + 6, wallY - 4);

        // emitters (from engine telemetry — the applied scene state)
        for (const em of tel?.emitters ?? []) {
          const [ex, ey] = sceneToXY(em.x, em.y, cx, cy, scale);
          if (ey < -12) continue;
          const col = em.active ? "#e8b04a" : "#8a6a2a";
          ctx.fillStyle = col;
          ctx.beginPath();
          ctx.roundRect(ex - 6, ey - 6, 12, 12, 3);
          ctx.fill();
          ctx.strokeStyle = "#0a0a12";
          ctx.stroke();
          ctx.fillStyle = "rgba(232,176,74,0.9)";
          ctx.font = "9px monospace";
          ctx.fillText(em.cat, ex + 8, ey + 3);
        }
      }

      // labels
      ctx.fillStyle = "#8a8aa5";
      ctx.font = "11px monospace";
      ctx.fillText("MIC", cx + 16, cy + 4);
      ctx.fillStyle = "#3ee6d8";
      ctx.fillText(`${d.toFixed(2)}m  ${az >= 0 ? "+" : ""}${az.toFixed(0)}°`, sx + 13, sy + 4);

      // state chip
      const mode = tel?.motion_manual ? (dragging ? "DRAG" : st.motionMode.toUpperCase()) : "AUTO";
      const label = tel ? `${mode} · ${tel.motion_state.toUpperCase()}` : "—";
      ctx.fillStyle = "rgba(18,18,30,0.85)";
      ctx.strokeStyle = "#2c2c48";
      ctx.font = "10px monospace";
      const tw = ctx.measureText(label).width + 16;
      ctx.beginPath();
      ctx.roundRect(8, 8, tw, 20, 4);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = tel?.motion_manual ? "#e8b04a" : "#8b7cf6";
      ctx.fillText(label, 16, 22);

      // bypass dim overlay
      if (st.params?.bypass) {
        ctx.fillStyle = "rgba(7,7,12,0.55)";
        ctx.fillRect(0, 0, W, H);
        ctx.fillStyle = "#e8b04a";
        ctx.font = "11px monospace";
        ctx.fillText("BYPASS — position has no audible effect", cx - 130, cy - 60);
      }

      raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, [reducedMotion]);

  const ACTIONS = [
    ["lean-in", "Lean In"],
    ["lean-back", "Lean Back"],
    ["turn-away", "Turn Away"],
    ["across-room", "Across Room"],
    ["return", "Return"],
    ["freeze", "Freeze"],
  ] as const;

  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <div className="panel-title">Live Presence Stage</div>
        <div className="flex gap-1">
          {(["auto", "touch", "latch"] as const).map((m) => (
            <button
              key={m}
              onClick={() => setMotionMode(m)}
              className={`btn text-[10px] ${motionMode === m ? "btn-active" : ""}`}
              title={
                m === "auto"
                  ? "Automation moves; drag temporarily overrides"
                  : m === "touch"
                    ? "Drag, then release glides back to automation"
                    : "Drag stays put until you resume automation"
              }
            >
              {m.toUpperCase()}
            </button>
          ))}
        </div>
      </div>

      <canvas
        ref={ref}
        width={W}
        height={H}
        className="w-full h-auto rounded-lg border border-[var(--color-edge)] cursor-crosshair touch-none"
        role="application"
        aria-label={`Speaker position: ${cur().d.toFixed(2)} meters, ${cur().az.toFixed(0)} degrees. Drag or use arrow keys.`}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onKeyDown={(e) => {
          const step = e.shiftKey ? 0.02 : 0.08;
          if (e.key === "ArrowUp") nudge(step, 0);
          else if (e.key === "ArrowDown") nudge(-step, 0);
          else if (e.key === "ArrowLeft") nudge(0, -3);
          else if (e.key === "ArrowRight") nudge(0, 3);
          else if (e.key === "Home") { posRef.current = { d: 0.32, az: 0, f: 0 }; api.motionSetManual(0.32, 0, 0, 1.2).catch(() => {}); }
          else return;
          e.preventDefault();
        }}
      />
      <div className="flex justify-between text-[9px] text-faint mt-1">
        <span>drag speaker/dog/emitters · click door to toggle · arrows nudge</span>
        <span>
          {t ? `${(t.motion_distance_m ?? 0).toFixed(2)} m · ${(t.motion_azimuth_deg ?? 0).toFixed(0)}°` : "—"}
        </span>
      </div>

      {/* acoustic actions */}
      <div className="flex flex-wrap gap-1.5 mt-2">
        {ACTIONS.map(([id, label]) => (
          <button
            key={id}
            className={`btn text-[10px] ${id === "freeze" && t?.motion_manual ? "btn-active" : ""}`}
            onClick={() => acousticAction(id)}
          >
            {label}
          </button>
        ))}
      </div>

      {/* snapshots */}
      <div className="flex flex-wrap items-center gap-1.5 mt-2 pt-2 border-t border-[var(--color-edge)]">
        {snapshots.map((s) => (
          <div key={s.name} className="flex items-center">
            <button
              className="btn text-[10px] rounded-r-none"
              onClick={() => recallSnapshot(s.name)}
              title={`${s.distance_m.toFixed(2)}m ${s.azimuth_deg}° · ${s.transition_s}s · ${s.mode_after}`}
            >
              {s.name}
            </button>
            {!s.factory && (
              <button
                className="btn text-[10px] rounded-l-none border-l-0 px-1.5 btn-danger"
                onClick={() => deleteSnapshot(s.name)}
                title="Delete snapshot"
              >
                ×
              </button>
            )}
          </div>
        ))}
        <input
          className="bg-transparent border border-[var(--color-edge)] rounded px-1.5 py-1 text-[10px] w-20 text-dim"
          placeholder="save as…"
          value={snapName}
          onChange={(e) => setSnapName(e.target.value)}
        />
        <button
          className="btn text-[10px]"
          disabled={!snapName.trim()}
          onClick={() => {
            saveSnapshot(snapName.trim(), 1.2, "latch");
            setSnapName("");
          }}
        >
          + Save
        </button>
      </div>
    </div>
  );
}

/** Scrolling distance history ~30s (unchanged from v0.1). */
export function MotionHistory() {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d")!;
    let raf = 0;
    const draw = () => {
      const hist = useStore.getState().distanceHistory;
      const t = useStore.getState().telemetry;
      const w = canvas.width;
      const h = canvas.height;
      ctx.clearRect(0, 0, w, h);
      ctx.strokeStyle = "#1a1a2e";
      for (const d of [0.25, 0.5, 1.0, 1.5]) {
        const y = h - ((d - 0.1) / 2.2) * h;
        ctx.beginPath(); ctx.moveTo(0, y); ctx.lineTo(w, y); ctx.stroke();
        ctx.fillStyle = "#55556e";
        ctx.font = "9px monospace";
        ctx.fillText(`${d}m`, 4, y - 2);
      }
      if (hist.length > 1) {
        const now = hist[hist.length - 1].t;
        const span = 30000;
        ctx.strokeStyle = "#3ee6d8";
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        hist.forEach((p, i) => {
          const x = w - ((now - p.t) / span) * w;
          const y = h - ((p.d - 0.1) / 2.2) * h;
          if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        });
        ctx.stroke();
      }
      if (t) {
        ctx.fillStyle = "#8a8aa5";
        ctx.font = "10px monospace";
        ctx.fillText(
          `state: ${t.motion_state}   target: ${t.motion_target_m.toFixed(2)}m`,
          8, h - 6,
        );
      }
      raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, []);
  return <canvas ref={ref} width={440} height={100} className="w-full h-auto rounded-lg" aria-label="Motion distance history" />;
}
