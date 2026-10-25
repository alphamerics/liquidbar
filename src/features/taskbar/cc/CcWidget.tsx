import { BatteryCharging, BatteryFull, BatteryLow, ChevronRight, Power, Volume1, Volume2, VolumeX } from "lucide-react";
import type { AudioSnapshot, BatterySnapshot } from "../../../lib/shell";
import {
  CC_WIDGET_DEFS,
  CC_WIDGET_ICONS,
  type CcLayoutItem,
  type CcWidgetId,
} from "./layout";
import { CircleTile } from "./CircleTile";
import { GlassCard } from "glass-ui";
import { CcSlider } from "./CcSlider";
import { ToggleTile } from "./ToggleTile";

/**
 * Everything a rendered widget needs from the panel: live state plus the
 * actions behind each tile's buttons.
 */
export interface CcWidgetContext {
  wifiOn: boolean | null;
  btOn: boolean | null;
  /** The Focus pill's label ("On", "Off", "Until 8:00 AM", ...). */
  dndLabel: string;
  dndEnabled: boolean;
  airplane: boolean;
  nightLight: boolean;
  audio: AudioSnapshot | null;
  /** The battery state; null until the first poll answers. */
  battery: BatterySnapshot | null;
  onOpenView: (
    view: "wifi" | "bluetooth" | "tray" | "sound" | "focus" | "calendar" | "battery",
  ) => void;
  onToggleRadio: (key: "wifi" | "bluetooth") => void;
  onToggleDnd: () => void;
  onToggleReal: (key: "airplane" | "nightLight") => void;
  onOpenSettings: () => void;
  onExit: () => void;
  onMaster: (value: number) => void;
  onMute: () => void;
}

/** Renders one template widget with the panel's live state. */
export function CcWidgetItem({ id, ctx }: { id: CcWidgetId; ctx: CcWidgetContext }) {
  switch (id) {
    case "wifi":
      return (
        <ToggleTile
          on={ctx.wifiOn ?? false}
          label="Wi-Fi"
          sub={ctx.wifiOn === null ? "…" : ctx.wifiOn ? "On" : "Off"}
          chevron
          onOpen={() => ctx.onOpenView("wifi")}
          onToggle={() => ctx.onToggleRadio("wifi")}
        >
          {WifiIcon()}
        </ToggleTile>
      );
    case "bluetooth":
      return (
        <ToggleTile
          on={ctx.btOn ?? false}
          label="Bluetooth"
          sub={ctx.btOn === null ? "…" : ctx.btOn ? "On" : "Off"}
          chevron
          onOpen={() => ctx.onOpenView("bluetooth")}
          onToggle={() => ctx.onToggleRadio("bluetooth")}
        >
          {BluetoothIcon()}
        </ToggleTile>
      );
    case "focus":
      return (
        <ToggleTile
          on={ctx.dndEnabled}
          label="Focus"
          sub={ctx.dndLabel}
          chevron
          onOpen={() => ctx.onOpenView("focus")}
          onToggle={ctx.onToggleDnd}
        >
          {FocusIcon()}
        </ToggleTile>
      );
    case "calendar":
      return (
        <ToggleTile
          on={false}
          label="Calendar"
          sub={calendarDayLabel()}
          chevron
          onOpen={() => ctx.onOpenView("calendar")}
          onToggle={() => ctx.onOpenView("calendar")}
        >
          {CalendarIcon()}
        </ToggleTile>
      );
    case "airplane":
      return (
        <CircleTile
          on={ctx.airplane}
          label="Airplane mode"
          onClick={() => ctx.onToggleReal("airplane")}
        >
          {AirplaneIcon()}
        </CircleTile>
      );
    case "night-light":
      return (
        <CircleTile
          on={ctx.nightLight}
          label="Night light"
          onClick={() => ctx.onToggleReal("nightLight")}
        >
          {NightLightIcon()}
        </CircleTile>
      );
    case "tray":
      return (
        <CircleTile on={false} label="Tray icons" onClick={() => ctx.onOpenView("tray")}>
          {TrayIcon()}
        </CircleTile>
      );
    case "sound":
      return (
        <GlassCard className="cc-slider-card">
          <span className="cc-card-title">Sound</span>
          <div className="cc-slider-row">
            <button
              type="button"
              className="cc-slider-icon"
              aria-label={ctx.audio?.masterMuted ? "Unmute" : "Mute"}
              onClick={ctx.onMute}
            >
              {ctx.audio?.masterMuted ? <VolumeX size={17} /> : <Volume2 size={17} />}
            </button>
            <CcSlider
              value={(ctx.audio?.master ?? 0.5) * 100}
              onChange={ctx.onMaster}
              onCommit={ctx.onMaster}
            />
            <span className="cc-slider-icon cc-static-icon">
              {(ctx.audio?.master ?? 0.5) > 0.5 ? <Volume2 size={17} /> : <Volume1 size={17} />}
            </span>
            <button
              type="button"
              className="cc-open cc-hit"
              aria-label="Open sound settings"
              onClick={() => ctx.onOpenView("sound")}
            >
              <ChevronRight size={18} className="cc-chevron" />
            </button>
          </div>
        </GlassCard>
      );
    case "battery": {
      const battery = ctx.battery;
      const charging = battery?.charging ?? false;
      const low = (battery?.percent ?? 100) <= 20;
      const BatteryGlyph = charging
        ? BatteryCharging
        : battery?.percent === null || battery?.percent === undefined || low
          ? BatteryLow
          : BatteryFull;
      return (
        <GlassCard shape="pill">
          <div
            className="cc-tile-button"
            role="button"
            aria-label="Battery"
            title="Battery"
            onClick={() => ctx.onOpenView("battery")}
          >
            <span
              className="cc-icon-circle"
              data-on={charging}
              style={{ cursor: "default" }}
            >
              <BatteryGlyph size={22} strokeWidth={2.2} />
            </span>
            <span className="cc-tile-text">
              <span className="cc-tile-title">Battery</span>
              <span className="cc-tile-sub">{batterySub(battery)}</span>
            </span>
            <ChevronRight size={18} className="cc-chevron" />
          </div>
        </GlassCard>
      );
    }
    case "exit":
      return (
        <CircleTile on={false} label="Exit Glassbar" onClick={ctx.onExit}>
          <Power size={28} strokeWidth={2.2} />
        </CircleTile>
      );
    case "settings":
      return (
        <CircleTile on={false} label="Settings" onClick={ctx.onOpenSettings}>
          {SettingsIcon()}
        </CircleTile>
      );
  }
}

/** The battery pill's status line. */
function batterySub(battery: BatterySnapshot | null): string {
  if (battery === null) {
    return "…";
  }
  if (!battery.hasBattery) {
    return "Not available";
  }
  const percent =
    battery.percent === null ? "…" : battery.percent >= 100 ? "Full" : `${battery.percent}%`;
  if (battery.charging) {
    return `${percent} · Charging`;
  }
  return battery.acConnected ? `${percent} · Plugged in` : percent;
}

/** The pill's date line: e.g. "Mon, Oct 12". */
function calendarDayLabel() {
  return new Date().toLocaleDateString(undefined, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
}

/** The panel's main view: every placed widget on the 4x6 layout grid. */
export function CcMainGrid({
  layout,
  ctx,
}: {
  layout: CcLayoutItem[];
  ctx: CcWidgetContext;
}) {
  return (
    <div className="cc-layout">
      {layout.map((item) => {
        const def = CC_WIDGET_DEFS[item.id];
        return (
          <div
            key={item.id}
            className="cc-cell"
            data-shape={def.shape}
            style={{
              gridColumn: `${item.x + 1} / span ${def.w}`,
              gridRow: `${item.y + 1} / span ${def.h}`,
            }}
          >
            <CcWidgetItem id={item.id} ctx={ctx} />
          </div>
        );
      })}
    </div>
  );
}

// The icon helpers keep the JSX above shallow; sizes match the original tiles.
function WifiIcon() {
  const Icon = CC_WIDGET_ICONS.wifi;
  return <Icon size={22} strokeWidth={2.2} />;
}

function BluetoothIcon() {
  const Icon = CC_WIDGET_ICONS.bluetooth;
  return <Icon size={22} strokeWidth={2.2} />;
}

function FocusIcon() {
  const Icon = CC_WIDGET_ICONS.focus;
  return <Icon size={22} strokeWidth={2.2} />;
}

function AirplaneIcon() {
  const Icon = CC_WIDGET_ICONS.airplane;
  return <Icon size={28} strokeWidth={2.2} />;
}

function NightLightIcon() {
  const Icon = CC_WIDGET_ICONS["night-light"];
  return <Icon size={28} strokeWidth={2.2} />;
}

function TrayIcon() {
  const Icon = CC_WIDGET_ICONS.tray;
  return <Icon size={28} strokeWidth={2.2} />;
}

function CalendarIcon() {
  const Icon = CC_WIDGET_ICONS.calendar;
  return <Icon size={22} strokeWidth={2.2} />;
}

function SettingsIcon() {
  const Icon = CC_WIDGET_ICONS.settings;
  return <Icon size={28} strokeWidth={2.2} />;
}
