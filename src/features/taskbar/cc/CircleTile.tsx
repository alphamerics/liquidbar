import type { ReactNode } from "react";
import { GlassCard } from "glass-ui";

/** A full-circle quick toggle (airplane mode, night light). */
export function CircleTile({
  on,
  label,
  onClick,
  children,
}: {
  on: boolean;
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <GlassCard shape="circle" className="cc-circle-tile">
      <button
        type="button"
        className="cc-circle-button"
        data-on={on}
        aria-pressed={on}
        aria-label={label}
        title={label}
        onClick={onClick}
      >
        {children}
      </button>
    </GlassCard>
  );
}
