import type { ReactNode } from "react";

interface SettingsGroupProps {
  title?: string;
  children: ReactNode;
  className?: string;
}

/** A titled group of rows, rendered as one rounded card. */
export function SettingsGroup({ title, children, className }: SettingsGroupProps) {
  return (
    <section className="settings-section">
      {title ? <h2 className="settings-section-title">{title}</h2> : null}
      <div className={`settings-card${className ? ` ${className}` : ""}`}>{children}</div>
    </section>
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
    <div className={`settings-row${className ? ` ${className}` : ""}`}>
      <span className="settings-row-label">{label}</span>
      {children}
    </div>
  );
}