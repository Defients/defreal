import React, { useEffect, useRef } from "react";
import { useStore } from "../store";

/** Top-down view: mic at center, speaker orbits/leans around it.
 *  Driven entirely by real motion-engine state. */
export function ProximityViz() {
  const ref = useRef<HTMLCanvasElement>(null);
  const reducedMotion = useStore((s) => s.reducedMotion);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d")!;
    let raf = 0;

    const draw = () => {
      const t = useStore.getState().telemetry;
      const w = canvas.width;
      const h = canvas.height;
      ctx.clearRect(0, 0, w, h);

      // room grid
      ctx.strokeStyle = "#1a1a2e";
      ctx.lineWidth = 1;
      for (let x = 0; x < w; x += 40) {
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, h);
        ctx.stroke();
      }
      for (let y = 0; y < h; y += 40) {
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }

      const cx = w / 2;
      const cy = h / 2 + 20;
      const scale = 110; // px per meter

      // distance rings (0.25 / 0.5 / 1 / 2 m)
      ctx.strokeStyle = "#23233c";
      ctx.setLineDash([3, 5]);
      for (const r of [0.25, 0.5, 1.0, 2.0]) {
        ctx.beginPath();
        ctx.arc(cx, cy, r * scale, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.setLineDash([]);

      const dist = t?.motion_distance_m ?? 0.32;
      const target = t?.motion_target_m ?? dist;
      const prog = t?.motion_progress ?? 1;

      // speaker position: derive azimuth from history? we only have distance+state.
      // Azimuth is implicit; show distance radially + a subtle drift from state.
      const azimuth = 0; // mic axis is "up"
      const ang = ((azimuth - 90) * Math.PI) / 180;
      const sx = cx + Math.cos(ang) * dist * scale;
      const sy = cy + Math.sin(ang) * dist * scale;
      const tx = cx + Math.cos(ang) * target * scale;
      const ty = cy + Math.sin(ang) * target * scale;

      // target marker
      if (prog < 0.999) {
        ctx.strokeStyle = "rgba(139,124,246,0.55)";
        ctx.setLineDash([4, 4]);
        ctx.beginPath();
        ctx.arc(tx, ty, 12, 0, Math.PI * 2);
        ctx.stroke();
        ctx.setLineDash([]);
        // travel line
        ctx.strokeStyle = "rgba(139,124,246,0.25)";
        ctx.beginPath();
        ctx.moveTo(sx, sy);
        ctx.lineTo(tx, ty);
        ctx.stroke();
      }

      // speaker glow
      const g = ctx.createRadialGradient(sx, sy, 0, sx, sy, 26);
      g.addColorStop(0, "rgba(62,230,216,0.5)");
      g.addColorStop(1, "rgba(62,230,216,0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(sx, sy, 26, 0, Math.PI * 2);
      ctx.fill();

      // speaker head (circle + facing tick)
      ctx.fillStyle = "#3ee6d8";
      ctx.beginPath();
      ctx.arc(sx, sy, 10, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "#0a0a12";
      ctx.lineWidth = 2;
      ctx.stroke();

      // mic: diamond + polar wedge pointing up
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
      // cardioid-ish pickup wedge
      ctx.fillStyle = "rgba(139,124,246,0.14)";
      ctx.beginPath();
      ctx.moveTo(0, 0);
      ctx.arc(0, 0, 34, -Math.PI / 2 - 0.6, -Math.PI / 2 + 0.6);
      ctx.closePath();
      ctx.fill();
      ctx.restore();

      // labels
      ctx.fillStyle = "#8a8aa5";
      ctx.font = "11px var(--font-mono)";
      ctx.fillText("MIC", cx + 16, cy + 4);
      ctx.fillStyle = "#3ee6d8";
      ctx.fillText(`${dist.toFixed(2)} m`, sx + 16, sy + 4);

      // state chip
      if (t) {
        ctx.fillStyle = "rgba(18,18,30,0.85)";
        ctx.strokeStyle = "#2c2c48";
        const label = t.motion_manual ? "MANUAL" : t.motion_state.toUpperCase();
        ctx.font = "10px var(--font-mono)";
        const tw = ctx.measureText(label).width + 16;
        ctx.beginPath();
        ctx.roundRect(8, 8, tw, 20, 4);
        ctx.fill();
        ctx.stroke();
        ctx.fillStyle = t.motion_manual ? "#e8b04a" : "#8b7cf6";
        ctx.fillText(label, 16, 22);
      }

      if (!reducedMotion) raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, [reducedMotion]);

  return (
    <canvas
      ref={ref}
      width={440}
      height={330}
      className="w-full h-auto rounded-lg"
      aria-label="Speaker/mic proximity visualization"
    />
  );
}

/** Scrolling distance history ~30s. */
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
      // axes
      ctx.strokeStyle = "#1a1a2e";
      for (const d of [0.25, 0.5, 1.0, 1.5]) {
        const y = h - ((d - 0.1) / 2.2) * h;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
        ctx.fillStyle = "#55556e";
        ctx.font = "9px var(--font-mono)";
        ctx.fillText(`${d}m`, 4, y - 2);
      }
      if (hist.length > 1) {
        const now = hist[hist.length - 1].t;
        const span = 30000; // 30s window
        ctx.strokeStyle = "#3ee6d8";
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        hist.forEach((p, i) => {
          const x = w - ((now - p.t) / span) * w;
          const y = h - ((p.d - 0.1) / 2.2) * h;
          if (i === 0) ctx.moveTo(x, y);
          else ctx.lineTo(x, y);
        });
        ctx.stroke();
      }
      // current state label
      if (t) {
        ctx.fillStyle = "#8a8aa5";
        ctx.font = "10px var(--font-mono)";
        ctx.fillText(`state: ${t.motion_state}   target: ${t.motion_target_m.toFixed(2)}m`, 8, h - 6);
      }
      raf = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(raf);
  }, []);

  return (
    <canvas
      ref={ref}
      width={440}
      height={110}
      className="w-full h-auto rounded-lg"
      aria-label="Motion distance history"
    />
  );
}
