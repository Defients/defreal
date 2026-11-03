import React from "react";

export function Slider(props: {
  label?: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit?: string;
  violet?: boolean;
  format?: (v: number) => string;
  onChange: (v: number) => void;
  disabled?: boolean;
}) {
  const { label, value, min, max, step = 0.01, unit, violet, format, onChange, disabled } = props;
  return (
    <div className="flex items-center gap-3">
      {label && (
        <div className="w-28 shrink-0 text-[11px] text-dim" title={label}>
          {label}
        </div>
      )}
      <input
        type="range"
        className={violet ? "violet flex-1" : "flex-1"}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        aria-label={label}
      />
      <div className="w-20 shrink-0 text-right text-[11px] mono text-dim">
        {format ? format(value) : `${value.toFixed(2)}${unit ?? ""}`}
      </div>
    </div>
  );
}

export function Toggle(props: {
  label: string;
  on: boolean;
  onChange: (v: boolean) => void;
  title?: string;
}) {
  return (
    <button
      className="flex items-center gap-2 text-left"
      onClick={() => props.onChange(!props.on)}
      title={props.title ?? props.label}
      aria-pressed={props.on}
    >
      <span className={`toggle ${props.on ? "on" : ""}`} role="switch" aria-checked={props.on} />
      <span className="text-[11px] text-dim">{props.label}</span>
    </button>
  );
}

export function Select<T extends string>(props: {
  label?: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="flex items-center gap-3">
      {props.label && <div className="w-28 shrink-0 text-[11px] text-dim">{props.label}</div>}
      <select
        className="select flex-1"
        value={props.value}
        onChange={(e) => props.onChange(e.target.value as T)}
        aria-label={props.label}
      >
        {props.options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </div>
  );
}

export function Knob(props: {
  value: number; // 0..1
  onChange: (v: number) => void;
  size?: number;
  label?: string;
}) {
  const size = props.size ?? 150;
  const v = Math.min(1, Math.max(0, props.value));
  const angle = -135 + v * 270;
  const dragRef = React.useRef<{ y0: number; v0: number } | null>(null);

  const onPointerDown = (e: React.PointerEvent) => {
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    dragRef.current = { y0: e.clientY, v0: v };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    if (!dragRef.current) return;
    const dv = (dragRef.current.y0 - e.clientY) / 180;
    props.onChange(Math.min(1, Math.max(0, dragRef.current.v0 + dv)));
  };
  const onPointerUp = () => (dragRef.current = null);
  const onWheel = (e: React.WheelEvent) => {
    props.onChange(Math.min(1, Math.max(0, v - Math.sign(e.deltaY) * 0.03)));
  };

  const r = size / 2 - 14;
  const cx = size / 2;
  const cy = size / 2;
  // arc from -135deg to +135deg
  const arc = (a0: number, a1: number, rr: number) => {
    const p = (a: number) => [
      cx + rr * Math.cos(((a - 90) * Math.PI) / 180),
      cy + rr * Math.sin(((a - 90) * Math.PI) / 180),
    ];
    const [x0, y0] = p(a0);
    const [x1, y1] = p(a1);
    const large = a1 - a0 > 180 ? 1 : 0;
    return `M ${x0} ${y0} A ${rr} ${rr} 0 ${large} 1 ${x1} ${y1}`;
  };

  return (
    <div
      className="flex flex-col items-center select-none"
      style={{ width: size }}
      role="slider"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(v * 100)}
      aria-label={props.label ?? "realism"}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "ArrowUp" || e.key === "ArrowRight") props.onChange(Math.min(1, v + 0.03));
        if (e.key === "ArrowDown" || e.key === "ArrowLeft") props.onChange(Math.max(0, v - 0.03));
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onWheel={onWheel}
    >
      <svg width={size} height={size} className="cursor-ns-resize">
        <path d={arc(-135, 135, r)} fill="none" stroke="var(--color-edge)" strokeWidth={6} strokeLinecap="round" />
        {v > 0.001 && (
          <path
            d={arc(-135, angle, r)}
            fill="none"
            stroke="url(#realismGrad)"
            strokeWidth={6}
            strokeLinecap="round"
          />
        )}
        <defs>
          <linearGradient id="realismGrad" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0%" stopColor="#5a4fae" />
            <stop offset="60%" stopColor="#8b7cf6" />
            <stop offset="100%" stopColor="#3ee6d8" />
          </linearGradient>
        </defs>
        {/* pointer */}
        <g transform={`rotate(${angle} ${cx} ${cy})`}>
          <line
            x1={cx}
            y1={cy - r + 2}
            x2={cx}
            y2={cy - r + 12}
            stroke="#e6e6f0"
            strokeWidth={3}
            strokeLinecap="round"
          />
        </g>
        <circle cx={cx} cy={cy} r={r - 22} fill="var(--color-panel-2)" stroke="var(--color-edge)" />
        <text
          x={cx}
          y={cy + 2}
          textAnchor="middle"
          dominantBaseline="middle"
          fill="var(--color-cyan)"
          fontSize={size * 0.22}
          fontFamily="var(--font-mono)"
          fontWeight={700}
        >
          {Math.round(v * 100)}
        </text>
        <text
          x={cx}
          y={cy + size * 0.16}
          textAnchor="middle"
          fill="var(--color-text-faint)"
          fontSize={10}
          letterSpacing={2}
        >
          %
        </text>
      </svg>
      {props.label && <div className="text-[10px] tracking-[0.2em] text-faint mt-1">{props.label}</div>}
    </div>
  );
}

export function Meter(props: { label: string; rmsDb: number; peakDb: number; clipped?: boolean }) {
  const norm = (db: number) => Math.min(1, Math.max(0, (db + 60) / 60));
  const rms = norm(props.rmsDb);
  const peak = norm(props.peakDb);
  return (
    <div>
      <div className="flex justify-between text-[10px] text-faint mb-1">
        <span>{props.label}</span>
        <span className="mono">
          {props.rmsDb <= -99 ? "-∞" : props.rmsDb.toFixed(1)} dB
        </span>
      </div>
      <div className="meter-track">
        <div className="meter-fill" style={{ transform: `scaleX(${rms})` }} />
        {peak > 0.001 && <div className="meter-peak" style={{ left: `${peak * 100}%` }} />}
      </div>
    </div>
  );
}
