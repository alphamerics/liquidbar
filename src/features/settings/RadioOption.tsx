import type { ReactNode } from "react";

interface RadioGroupProps {
  children: ReactNode;
}

export function RadioGroup({ children }: RadioGroupProps) {
  return (
    <div role="radiogroup" className="settings-radio-group">
      {children}
    </div>
  );
}

interface RadioOptionProps {
  checked: boolean;
  onSelect: () => void;
  label?: ReactNode;
  /** When set, the radio renders as a filled color swatch (Apple tint dots). */
  color?: string;
  title?: string;
}

export function RadioOption({ checked, onSelect, label, color, title }: RadioOptionProps) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      aria-label={title ?? (typeof label === "string" ? label : undefined)}
      title={title}
      className="settings-radio-option"
      data-checked={checked}
      data-swatch={color ? true : undefined}
      style={color ? { "--swatch": color } as React.CSSProperties : undefined}
      onClick={onSelect}
    >
      <span className="settings-radio" data-checked={checked} />
      {label}
    </button>
  );
}