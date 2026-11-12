import { useEffect, useRef, useState, type CSSProperties } from "react";

interface Props {
  /** 0 to 1, as reported by the engine. */
  value: number;
  onChange: (value: number) => void;
  color: string;
  muted?: boolean;
  label: string;
  className?: string;
}

/** How often a drag sends its position to the engine. */
const SEND_EVERY_MS = 40;

/**
 * A volume slider. While it is being dragged it shows the hand's position
 * and sends it to the engine at a steady rate; otherwise it follows the
 * engine.
 */
export function Fader({ value, onChange, color, muted, label, className }: Props) {
  const [dragging, setDragging] = useState<number | null>(null);
  const lastSent = useRef(0);
  const pending = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => () => clearTimeout(pending.current), []);

  const send = (v: number) => {
    clearTimeout(pending.current);
    const wait = SEND_EVERY_MS - (Date.now() - lastSent.current);
    if (wait <= 0) {
      lastSent.current = Date.now();
      onChange(v);
    } else {
      pending.current = setTimeout(() => {
        lastSent.current = Date.now();
        onChange(v);
      }, wait);
    }
  };

  const shown = dragging ?? value;
  const percent = Math.round(shown * 100);

  return (
    <input
      type="range"
      min={0}
      max={1}
      step={0.01}
      value={shown}
      aria-label={label}
      aria-valuetext={`${percent}%`}
      title={`${label}: ${percent}%`}
      className={`fader ${muted ? "is-muted" : ""} ${className ?? ""}`}
      style={{ "--c": color, "--v": `${percent}%` } as CSSProperties}
      onChange={(e) => {
        const v = Number(e.target.value);
        setDragging(v);
        send(v);
      }}
      onPointerUp={() => setDragging(null)}
      onBlur={() => setDragging(null)}
      onKeyUp={() => setDragging(null)}
    />
  );
}
