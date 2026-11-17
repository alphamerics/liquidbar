import { GlassList, GlassListItem } from "glass-ui";
import type { ReactNode } from "react";

interface SettingsGroupProps {
  title?: string;
  children: ReactNode;
  className?: string;
}

/** A titled group of rows, rendered as one rounded glass-ui list card. */
export function SettingsGroup({ title, children, className }: SettingsGroupProps) {
  return (
    <GlassList title={title} className={className}>
      {children}
    </GlassList>
  );
}

interface OptionRowProps {
  label: ReactNode;
  children: ReactNode;
  className?: string;
}

/** A single settings row: label on the left, control on the right. */
export function OptionRow({ label, children, className }: OptionRowProps) {
  return (
    <GlassListItem label={label} className={className}>
      {children}
    </GlassListItem>
  );
}
