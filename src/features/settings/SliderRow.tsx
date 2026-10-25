import * as Slider from "@radix-ui/react-slider";

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
  return (
    <div className="settings-row settings-slider-row">
      <span className="settings-row-label">{label}</span>
      <Slider.Root
        className="settings-slider"
        value={[value]}
        min={min}
        max={max}
        step={step}
        aria-label={label}
        onValueChange={(values) => onChange(values[0])}
        onValueCommit={onCommit ? (values) => onCommit(values[0]) : undefined}
      >
        <Slider.Track className="settings-slider-track">
          <Slider.Range className="settings-slider-range" />
        </Slider.Track>
        <Slider.Thumb className="settings-slider-thumb" aria-label={label} />
      </Slider.Root>
      <span className="settings-slider-value">
        {format ? format(value) : value.toFixed(2)}
      </span>
    </div>
  );
}