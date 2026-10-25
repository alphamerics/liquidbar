import {
  BatteryCharging,
  BatteryFull,
  BatteryLow,
  Check,
  Clock,
} from "lucide-react";
import type { BatterySnapshot } from "../../../lib/shell";
import { Subview } from "./Subview";

/**
 * The Battery sub-view behind the battery pill's arrow: the charge meter,
 * the power source and the estimated remaining runtime (when Windows
 * reports one).
 */
export function BatteryView({
  battery,
  onBack,
}: {
  battery: BatterySnapshot | null;
  onBack: () => void;
}) {
  const percent = battery?.percent ?? null;
  const charging = battery?.charging ?? false;
  const low = battery?.hasBattery && percent !== null && percent <= 20;
  const Glyph = charging
    ? BatteryCharging
    : percent === null || low
      ? BatteryLow
      : BatteryFull;
  return (
    <Subview title="Battery" onBack={onBack} contentClassName="cc-batt">
      {battery === null || !battery.hasBattery ? (
        <span className="cc-empty">
          {battery === null ? "Reading the battery…" : "No battery detected"}
        </span>
      ) : (
        <>
          <div className="cc-net-row cc-batt-static">
            <span className="cc-net-circle">
              <Glyph size={16} className="cc-net-icon" />
            </span>
            <span className="cc-net-flex">
              <span className="cc-net-name">
                {percent === null ? "…" : `${percent}%`}
              </span>
              <span className="cc-net-process">{powerSourceLabel(battery)}</span>
            </span>
            {percent !== null && percent >= 100 && battery.acConnected && (
              <Check size={16} className="cc-net-check" />
            )}
          </div>
          {percent !== null && (
            <div className="cc-batt-meter" role="meter" aria-valuenow={percent}>
              <span
                className="cc-batt-fill"
                style={{ width: `${percent}%` }}
                data-charging={charging}
                data-low={low}
              />
            </div>
          )}
          <div className="cc-net-row cc-batt-static">
            <span className="cc-net-circle">
              <Clock size={16} className="cc-net-icon" />
            </span>
            <span className="cc-net-flex">
              <span className="cc-net-name">Time remaining</span>
              <span className="cc-net-process">{lifeLabel(battery)}</span>
            </span>
          </div>
        </>
      )}
    </Subview>
  );
}

/** The power source line: charging, plugged in or on battery. */
function powerSourceLabel(battery: BatterySnapshot): string {
  if (battery.charging) {
    return "Charging";
  }
  if (battery.acConnected) {
    return (battery.percent ?? 100) >= 100 ? "Fully charged" : "Plugged in";
  }
  return "On battery";
}

/** The estimated runtime: "≈ 2 h 14 min", or a placeholder when Windows
 * cannot estimate it (common right after boot or while charging). */
function lifeLabel(battery: BatterySnapshot): string {
  if (battery.lifeSeconds === null) {
    return battery.charging ? "Charging…" : "Not available";
  }
  const minutes = Math.round(battery.lifeSeconds / 60);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours > 0) {
    return `≈ ${hours} h ${rest} min`;
  }
  return `≈ ${rest} min`;
}
