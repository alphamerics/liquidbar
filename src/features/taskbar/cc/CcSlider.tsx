import * as SliderPrimitive from "@radix-ui/react-slider";
import { useEffect, useRef, useState } from "react";

/**
 * A volume slider that owns its dragging state so background refreshes never
 * yank the handle mid-drag; pushes each move through `onChange` and settles
 * with `onCommit`.
 */
export function CcSlider({
  value,
  onChange,
  onCommit,
}: {
  value: number;
  onChange: (value: number) => void;
  onCommit: (value: number) => void;
}) {
  const [local, setLocal] = useState(value);
  const dragging = useRef(false);
  // While not dragging, follow the external value.
  useEffect(() => {
    if (!dragging.current) {
      setLocal(value);
    }
  }, [value]);
  return (
    <SliderPrimitive.Root
      className="cc-slider"
      value={[local]}
      max={100}
      step={1}
      aria-label="Level"
      onPointerDown={() => {
        dragging.current = true;
      }}
      onValueChange={(values) => {
        const next = values[0] ?? 0;
        setLocal(next);
        onChange(next);
      }}
      onValueCommit={(values) => {
        dragging.current = false;
        onCommit(values[0] ?? 0);
      }}
    >
      <SliderPrimitive.Track className="cc-slider-track">
        <SliderPrimitive.Range className="cc-slider-range" />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb className="cc-slider-thumb" />
    </SliderPrimitive.Root>
  );
}
