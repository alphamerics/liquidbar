import { GlassSwitch } from "glass-ui";

interface ToggleSwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}

/** The settings toggle: glass-ui's liquid-glass switch with the shared
    control API (label-driven a11y, disabled state). */
export function ToggleSwitch({ checked, onChange, label, disabled }: ToggleSwitchProps) {
  return (
    <GlassSwitch
      checked={checked}
      onChange={onChange}
      label={label}
      disabled={disabled}
    />
  );
}
