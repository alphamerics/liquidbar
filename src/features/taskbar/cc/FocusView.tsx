import { BellOff, Check, MoonStar } from "lucide-react";
import type { DndState } from "../../../lib/shell";
import { Subview } from "./Subview";

/**
 * The Focus sub-view: the two Do not disturb modes behind the Focus pill's
 * arrow. "Do not disturb" stays on until turned off; "Sleep" arms the window
 * that ends at 8:00 AM — even across a shutdown, the native side reconciles
 * the window on boot.
 */
export function FocusView({
  state,
  onBack,
  onMode,
}: {
  state: DndState | null;
  onBack: () => void;
  onMode: (mode: "dnd" | "sleep") => void;
}) {
  const dndActive = state?.enabled === true && state.expiresAtMs === null;
  const sleepActive = state?.enabled === true && state.expiresAtMs !== null;
  return (
    <Subview title="Focus" onBack={onBack}>
      <div className="cc-net-row cc-hit" onClick={() => onMode("dnd")}>
        <span className="cc-net-circle">
          <BellOff size={16} className="cc-net-icon" />
        </span>
        <span className="cc-net-name">Do not disturb</span>
        {dndActive && <Check size={16} className="cc-net-check" />}
      </div>
      <div className="cc-net-row cc-hit" onClick={() => onMode("sleep")}>
        <span className="cc-net-circle">
          <MoonStar size={16} className="cc-net-icon" />
        </span>
        <span className="cc-net-flex">
          <span className="cc-net-name">Sleep</span>
          <span className="cc-net-process">Off at 8:00 AM</span>
        </span>
        {sleepActive && <Check size={16} className="cc-net-check" />}
      </div>
    </Subview>
  );
}
