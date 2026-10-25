import {
  AppWindowMac,
  Battery,
  Bluetooth,
  CalendarDays,
  Cog,
  Moon,
  MoonStar,
  Plane,
  Power,
  Volume2,
  Wifi,
  type LucideIcon,
} from "lucide-react";

/** The control-center panel is a fixed 4x6 grid; widgets snap to it. */
export const CC_GRID_COLS = 4;
export const CC_GRID_ROWS = 6;
export const CC_CELL_GAP = 10;
/** One grid row's height; pills fill it, circles center inside it. */
export const CC_ROW_HEIGHT = 78;

/** Every template widget: a pill spans 2x1, a panel 4x1, a circle 1x1. */
export type CcWidgetId =
  | "wifi"
  | "bluetooth"
  | "focus"
  | "calendar"
  | "airplane"
  | "night-light"
  | "tray"
  | "sound"
  | "settings"
  | "battery"
  | "exit";

export type CcWidgetShape = "pill" | "circle" | "panel";

export interface CcWidgetDef {
  id: CcWidgetId;
  name: string;
  shape: CcWidgetShape;
  /** Grid footprint in cells. */
  w: number;
  h: number;
}

export const CC_WIDGET_DEFS: Record<CcWidgetId, CcWidgetDef> = {
  wifi: { id: "wifi", name: "Wi-Fi", shape: "pill", w: 2, h: 1 },
  bluetooth: { id: "bluetooth", name: "Bluetooth", shape: "pill", w: 2, h: 1 },
  focus: { id: "focus", name: "Focus", shape: "pill", w: 2, h: 1 },
  calendar: { id: "calendar", name: "Calendar", shape: "pill", w: 2, h: 1 },
  airplane: { id: "airplane", name: "Airplane mode", shape: "circle", w: 1, h: 1 },
  "night-light": { id: "night-light", name: "Night light", shape: "circle", w: 1, h: 1 },
  tray: { id: "tray", name: "Tray icons", shape: "circle", w: 1, h: 1 },
  sound: { id: "sound", name: "Sound", shape: "panel", w: 4, h: 1 },
  settings: { id: "settings", name: "Settings", shape: "circle", w: 1, h: 1 },
  battery: { id: "battery", name: "Battery", shape: "pill", w: 2, h: 1 },
  exit: { id: "exit", name: "Exit Glassbar", shape: "circle", w: 1, h: 1 },
};

export const CC_WIDGET_ICONS: Record<CcWidgetId, LucideIcon> = {
  wifi: Wifi,
  bluetooth: Bluetooth,
  focus: MoonStar,
  calendar: CalendarDays,
  airplane: Plane,
  "night-light": Moon,
  tray: AppWindowMac,
  sound: Volume2,
  settings: Cog,
  battery: Battery,
  exit: Power,
};

/** One placed widget: its identity and top-left grid cell. */
export interface CcLayoutItem {
  id: CcWidgetId;
  x: number;
  y: number;
}

/** Mirrors the panel's classic arrangement (also the Rust-side default). */
export const DEFAULT_CC_LAYOUT: CcLayoutItem[] = [
  { id: "wifi", x: 0, y: 0 },
  { id: "bluetooth", x: 2, y: 0 },
  { id: "focus", x: 0, y: 1 },
  { id: "calendar", x: 2, y: 1 },
  { id: "airplane", x: 0, y: 2 },
  { id: "night-light", x: 1, y: 2 },
  { id: "tray", x: 2, y: 2 },
  { id: "settings", x: 3, y: 2 },
  { id: "sound", x: 0, y: 3 },
  { id: "battery", x: 0, y: 4 },
  { id: "exit", x: 2, y: 4 },
];

/** Where a dragged widget should land: its cell and, for same-footprint
 * swaps, the widget it displaces. */
export interface CcDropPlacement {
  x: number;
  y: number;
  swapOf?: CcWidgetId;
}

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);

/** Whether a `w` x `h` box at (x, y) intersects the placed item. */
function overlaps(item: CcLayoutItem, w: number, h: number, x: number, y: number) {
  const def = CC_WIDGET_DEFS[item.id];
  return x < item.x + def.w && item.x < x + w && y < item.y + def.h && item.y < y + h;
}

function collidesWith(
  items: CcLayoutItem[],
  w: number,
  h: number,
  x: number,
  y: number,
  skipId?: CcWidgetId,
) {
  return items.some((item) => item.id !== skipId && overlaps(item, w, h, x, y));
}

/** Turns stored layout data into a valid layout: drops unknown ids,
 * non-numeric coordinates, out-of-bounds and overlapping placements; a
 * non-list falls back to the default arrangement. */
export function sanitizeCcLayout(stored: unknown): CcLayoutItem[] {
  if (!Array.isArray(stored)) {
    return DEFAULT_CC_LAYOUT.map((item) => ({ ...item }));
  }
  const seen = new Set<CcWidgetId>();
  const items: CcLayoutItem[] = [];
  for (const entry of stored) {
    if (!entry || typeof entry !== "object") {
      continue;
    }
    const candidate = entry as { id?: unknown; x?: unknown; y?: unknown };
    const id = candidate.id;
    if (typeof id !== "string" || !(id in CC_WIDGET_DEFS) || seen.has(id as CcWidgetId)) {
      continue;
    }
    const def = CC_WIDGET_DEFS[id as CcWidgetId];
    const x = Math.round(Number(candidate.x));
    const y = Math.round(Number(candidate.y));
    if (!Number.isFinite(x) || !Number.isFinite(y)) {
      continue;
    }
    if (x < 0 || y < 0 || x + def.w > CC_GRID_COLS || y + def.h > CC_GRID_ROWS) {
      continue;
    }
    if (collidesWith(items, def.w, def.h, x, y)) {
      continue;
    }
    seen.add(id as CcWidgetId);
    items.push({ id: id as CcWidgetId, x, y });
  }
  return items;
}

/** The first free spot that fits the widget's footprint, scanning top-left
 * to bottom-right. */
export function findFreeSpot(
  items: CcLayoutItem[],
  def: CcWidgetDef,
): { x: number; y: number } | null {
  for (let y = 0; y <= CC_GRID_ROWS - def.h; y++) {
    for (let x = 0; x <= CC_GRID_COLS - def.w; x++) {
      if (!collidesWith(items, def.w, def.h, x, y)) {
        return { x, y };
      }
    }
  }
  return null;
}

/**
 * Resolves a drop: the clamped desired cell if free, a same-footprint swap
 * when a single matching widget sits there, otherwise the nearest free cell
 * (Manhattan distance). Falls back to the widget's original spot.
 */
export function resolveDrop(
  items: CcLayoutItem[],
  id: CcWidgetId,
  desiredX: number,
  desiredY: number,
): CcDropPlacement {
  const def = CC_WIDGET_DEFS[id];
  const moving = items.find((item) => item.id === id);
  const others = items.filter((item) => item.id !== id);
  const x = clamp(Math.round(desiredX), 0, CC_GRID_COLS - def.w);
  const y = clamp(Math.round(desiredY), 0, CC_GRID_ROWS - def.h);
  if (!collidesWith(others, def.w, def.h, x, y)) {
    return { x, y };
  }
  const occupant = others.find((item) => overlaps(item, def.w, def.h, x, y));
  if (
    occupant &&
    others.filter((item) => overlaps(item, def.w, def.h, x, y)).length === 1 &&
    CC_WIDGET_DEFS[occupant.id].w === def.w &&
    CC_WIDGET_DEFS[occupant.id].h === def.h &&
    moving
  ) {
    return { x, y, swapOf: occupant.id };
  }
  let best: { x: number; y: number } | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let cy = 0; cy <= CC_GRID_ROWS - def.h; cy++) {
    for (let cx = 0; cx <= CC_GRID_COLS - def.w; cx++) {
      if (collidesWith(others, def.w, def.h, cx, cy)) {
        continue;
      }
      const distance = Math.abs(cx - x) + Math.abs(cy - y);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = { x: cx, y: cy };
      }
    }
  }
  if (best) {
    return best;
  }
  return moving ? { x: moving.x, y: moving.y } : { x: 0, y: 0 };
}
