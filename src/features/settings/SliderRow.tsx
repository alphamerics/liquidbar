import { useEffect, useRef, useState } from "react";
import { GlassSlider } from "glass-ui";
import { GlassListItem } from "glass-ui";

interface SliderRowProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format?: (value: number) => string;
  onChange: (value: number) => void;
  /** Fires once when the user releases the thumb; for expensive changes. */
  onCommit?: (value: number) => void;
}

/** Live changes are applied to the app no faster than this — the settings
    round-trip (invoke → Rust apply → settings-changed broadcast) is far
    too heavy to run per pointer-move, which read as stutter on native
    resizes like the taskbar height. */
const LIVE_APPLY_INTERVAL_MS = 120;

export function SliderRow({
  label,
  value,
  min,
  max,
  step,
  format,
  onChange,
  onCommit,
}: SliderRowProps) {
  // While dragging, the thumb follows a local value so the slider stays
  // butter-smooth; the parent state (and the app) catch up throttled.
  const [dragValue, setDragValue] = useState<number | null>(null);
  const shown = dragValue ?? value;
  const lastSentRef = useRef(0);
  const pendingRef = useRef<number | null>(null);
  const timerRef = useRef(0);

  useEffect(
    () => () => {
      window.clearTimeout(timerRef.current);
    },
    [],
  );

  const apply = (next: number) => {
    pendingRef.current = next;
    const elapsed = Date.now() - lastSentRef.current;
    if (elapsed >= LIVE_APPLY_INTERVAL_MS) {
      lastSentRef.current = Date.now();
      pendingRef.current = null;
      onChange(next);
    } else {
      window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(() => {
        lastSentRef.current = Date.now();
        if (pendingRef.current != null) {
          const pending = pendingRef.current;
          pendingRef.current = null;
          onChange(pending);
        }
      }, LIVE_APPLY_INTERVAL_MS - elapsed);
    }
  };

  return (
    <GlassListItem label={label} className="settings-slider-row">
      <GlassSlider
        value={shown}
        min={min}
        max={max}
        step={step}
        label={label}
        onChange={(next) => {
          setDragValue(next);
          apply(next);
        }}
        onCommit={(next) => {
          setDragValue(null);
          pendingRef.current = null;
          window.clearTimeout(timerRef.current);
          lastSentRef.current = Date.now();
          onChange(next);
          onCommit?.(next);
        }}
      />
      <span className="settings-slider-value">
        {format ? format(shown) : shown.toFixed(2)}
      </span>
    </GlassListItem>
  );
}
