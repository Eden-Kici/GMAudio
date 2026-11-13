import { Children, useEffect, useRef, useState, type ReactNode } from "react";
import { useSizes } from "../layout";

/**
 * A thin bar to drag. Reports how far the pointer moved since the last
 * report, in pixels along its direction; double-click asks for a reset.
 */
export function Divider({
  direction,
  onDrag,
  onReset,
  label,
  className,
}: {
  /** "x": a vertical bar dragged left and right. "y": a horizontal bar dragged up and down. */
  direction: "x" | "y";
  onDrag: (delta: number) => void;
  onReset?: () => void;
  label: string;
  className?: string;
}) {
  const [held, setHeld] = useState(false);
  const last = useRef(0);
  const drag = useRef(onDrag);
  drag.current = onDrag;
  useEffect(() => {
    if (!held) return;
    const move = (e: PointerEvent) => {
      const at = direction === "x" ? e.clientX : e.clientY;
      if (at !== last.current) drag.current(at - last.current);
      last.current = at;
    };
    const drop = () => setHeld(false);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", drop);
    window.addEventListener("pointercancel", drop);
    // Keep the resize cursor and stop text being selected while dragging.
    const { cursor, userSelect } = document.body.style;
    document.body.style.cursor = direction === "x" ? "col-resize" : "row-resize";
    document.body.style.userSelect = "none";
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", drop);
      window.removeEventListener("pointercancel", drop);
      document.body.style.cursor = cursor;
      document.body.style.userSelect = userSelect;
    };
  }, [held, direction]);

  return (
    <div
      role="separator"
      aria-orientation={direction === "x" ? "vertical" : "horizontal"}
      aria-label={label}
      tabIndex={0}
      title={`${label}. Drag to resize${onReset ? "; double-click to reset" : ""}.`}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        last.current = direction === "x" ? e.clientX : e.clientY;
        setHeld(true);
      }}
      onDoubleClick={onReset}
      onKeyDown={(e) => {
        const step =
          direction === "x" ? (e.key === "ArrowRight" ? 16 : e.key === "ArrowLeft" ? -16 : 0) : e.key === "ArrowDown" ? 16 : e.key === "ArrowUp" ? -16 : 0;
        if (!step) return;
        e.preventDefault();
        e.stopPropagation();
        onDrag(step);
      }}
      className={`group relative z-10 shrink-0 touch-none ${direction === "x" ? "w-px cursor-col-resize" : "h-px cursor-row-resize"} bg-line ${className ?? ""}`}
    >
      {/* A wider strip to catch the pointer, and the highlight. */}
      <span className={`absolute ${direction === "x" ? "inset-y-0 -left-1 w-[9px]" : "inset-x-0 -top-1 h-[9px]"}`} />
      <span
        className={`absolute bg-accent transition-opacity ${held ? "opacity-100" : "opacity-0 group-hover:opacity-70 group-focus-visible:opacity-100"} ${
          direction === "x" ? "inset-y-0 -left-px w-[3px]" : "inset-x-0 -top-px h-[3px]"
        }`}
      />
    </div>
  );
}

/**
 * Children side by side (or stacked) with a bar between each pair to drag.
 * How the space is shared is remembered under `id`.
 */
export function Split({
  id,
  direction = "x",
  min = 180,
  className,
  children,
}: {
  id: string;
  direction?: "x" | "y";
  /** The smallest a part can be dragged to, in pixels. */
  min?: number;
  className?: string;
  children: ReactNode;
}) {
  const parts = Children.toArray(children);
  const box = useRef<HTMLDivElement>(null);
  const even = parts.map(() => 1);
  // Each number of parts keeps its own sizes, so adding a pane does not scramble them.
  const [shares, setShares] = useSizes(`${id}:${parts.length}`, even);
  const total = shares.reduce((a, b) => a + b, 0);

  const drag = (at: number, delta: number) => {
    const size = direction === "x" ? box.current?.clientWidth : box.current?.clientHeight;
    if (!size) return;
    const least = Math.min((min / size) * total, total / parts.length);
    const pair = shares[at] + shares[at + 1];
    const first = Math.min(pair - least, Math.max(least, shares[at] + (delta / size) * total));
    setShares(shares.map((s, i) => (i === at ? first : i === at + 1 ? pair - first : s)));
  };

  return (
    <div ref={box} className={`flex min-h-0 min-w-0 ${direction === "x" ? "flex-row" : "flex-col"} ${className ?? ""}`}>
      {parts.map((part, i) => (
        <div key={i} className="contents">
          {i > 0 && <Divider direction={direction} onDrag={(d) => drag(i - 1, d)} onReset={() => setShares(even)} label="Resize" />}
          <div className="flex min-h-0 min-w-0 flex-col" style={{ flex: `${shares[i]} 1 0` }}>
            {part}
          </div>
        </div>
      ))}
    </div>
  );
}
