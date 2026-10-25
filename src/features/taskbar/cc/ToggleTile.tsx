import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import { GlassCard } from "glass-ui";

/**
 * A quick-toggle pill (Wi-Fi, Bluetooth). The icon circle is its own button
 * that toggles the radio; everything else — text, chevron and the space
 * between — opens the sub-view.
 */
export function ToggleTile({
  on,
  label,
  sub,
  chevron,
  onOpen,
  onToggle,
  children,
}: {
  on: boolean;
  label: string;
  sub?: string;
  chevron?: boolean;
  onOpen?: () => void;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <GlassCard shape="pill">
      <div
        className="cc-tile-button"
        data-on={on}
        role="button"
        aria-label={label}
        title={label}
        onClick={onOpen}
      >
        <button
          type="button"
          className="cc-icon-circle"
          data-on={on}
          aria-pressed={on}
          aria-label={on ? `Turn ${label} off` : `Turn ${label} on`}
          title={on ? `Turn ${label} off` : `Turn ${label} on`}
          onClick={(event) => {
            event.stopPropagation();
            onToggle();
          }}
        >
          {children}
        </button>
        <span className="cc-tile-text">
          <span className="cc-tile-title">{label}</span>
          {sub !== undefined && <span className="cc-tile-sub">{sub}</span>}
        </span>
        {chevron && <ChevronRight size={18} className="cc-chevron" />}
      </div>
    </GlassCard>
  );
}
