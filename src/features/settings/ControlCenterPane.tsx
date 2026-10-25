import { useRef, useState } from "react";
import { RotateCcw, X } from "lucide-react";
import {
  CC_GRID_COLS,
  CC_GRID_ROWS,
  CC_CELL_GAP,
  CC_ROW_HEIGHT,
  CC_WIDGET_DEFS,
  CC_WIDGET_ICONS,
  DEFAULT_CC_LAYOUT,
  findFreeSpot,
  resolveDrop,
  sanitizeCcLayout,
  type CcDropPlacement,
  type CcLayoutItem,
  type CcWidgetId,
} from "../taskbar/cc/layout";
import type { GlassbarSettings } from "../../lib/shell";
import { SettingsGroup, OptionRow } from "./OptionRow";
import { ToggleSwitch } from "./ToggleSwitch";

/** Pointer-start bookkeeping kept outside React state (updates every move). */
interface DragStart {
  id: CcWidgetId;
  startClientX: number;
  startClientY: number;
  itemLeft: number;
  itemTop: number;
  grabCol: number;
  grabRow: number;
}

interface DragState {
  id: CcWidgetId;
  dx: number;
  dy: number;
  active: boolean;
  target: CcDropPlacement | null;
  /** Grid geometry captured at drag start, for the target highlight. */
  gridLeft: number;
  gridTop: number;
  colWidth: number;
}

/**
 * The Control Center settings pane: a live preview of the panel's widget
 * layout plus an edit mode that turns the preview into a drag-and-drop
 * editor on the panel's 4x6 grid (pills 2x1, panels 4x1, circles 1x1).
 * Every change saves into the shared settings JSON, which pushes the new
 * layout to the live panel.
 */
export function ControlCenterPane({
  settings,
  update,
}: {
  settings: GlassbarSettings;
  update: (patch: Partial<GlassbarSettings>) => void;
}) {
  const [editing, setEditing] = useState(false);
  const items = sanitizeCcLayout(settings.ccLayout);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const dragStart = useRef<DragStart | null>(null);
  const gridRef = useRef<HTMLDivElement | null>(null);

  const commit = (next: CcLayoutItem[]) => {
    update({ ccLayout: next });
  };

  const resetLayout = () => {
    setNotice(null);
    commit(DEFAULT_CC_LAYOUT.map((item) => ({ ...item })));
  };

  const removeItem = (id: CcWidgetId) => {
    // The Settings widget is permanent: movable, never removable.
    if (id === "settings") {
      return;
    }
    setNotice(null);
    commit(items.filter((item) => item.id !== id));
  };

  const beginDrag = (event: React.PointerEvent<HTMLDivElement>, item: CcLayoutItem) => {
    if (!editing || event.button !== 0) {
      return;
    }
    const grid = gridRef.current?.getBoundingClientRect();
    const itemRect = event.currentTarget.getBoundingClientRect();
    if (!grid) {
      return;
    }
    const def = CC_WIDGET_DEFS[item.id];
    const colWidth = (grid.width - (CC_GRID_COLS - 1) * CC_CELL_GAP) / CC_GRID_COLS;
    const pitchX = colWidth + CC_CELL_GAP;
    const pitchY = CC_ROW_HEIGHT + CC_CELL_GAP;
    // Which cell of the widget the grab started in, so the widget follows
    // the pointer without jumping.
    dragStart.current = {
      id: item.id,
      startClientX: event.clientX,
      startClientY: event.clientY,
      itemLeft: itemRect.left,
      itemTop: itemRect.top,
      grabCol: clamp(Math.floor((event.clientX - itemRect.left) / pitchX), 0, def.w - 1),
      grabRow: clamp(Math.floor((event.clientY - itemRect.top) / pitchY), 0, def.h - 1),
    };
    setDrag({
      id: item.id,
      dx: 0,
      dy: 0,
      active: false,
      target: null,
      gridLeft: grid.left,
      gridTop: grid.top,
      colWidth,
    });
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const moveDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const start = dragStart.current;
    if (!start || !drag || drag.id !== start.id) {
      return;
    }
    const dx = event.clientX - start.startClientX;
    const dy = event.clientY - start.startClientY;
    const active = drag.active || Math.hypot(dx, dy) > 4;
    if (!active) {
      return;
    }
    const pitchX = drag.colWidth + CC_CELL_GAP;
    const pitchY = CC_ROW_HEIGHT + CC_CELL_GAP;
    const col = Math.floor((event.clientX - drag.gridLeft) / pitchX);
    const row = Math.floor((event.clientY - drag.gridTop) / pitchY);
    const target = resolveDrop(
      items,
      start.id,
      col - start.grabCol,
      row - start.grabRow,
    );
    setDrag({ ...drag, dx, dy, active, target });
  };

  const endDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (dragStart.current && event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    dragStart.current = null;
    if (drag?.active && drag.target) {
      commitDrop(drag.id, drag.target);
    }
    setDrag(null);
  };

  const commitDrop = (id: CcWidgetId, placement: CcDropPlacement) => {
    const moving = items.find((item) => item.id === id);
    if (!moving) {
      return;
    }
    let next = items.map((item) =>
      item.id === id ? { ...item, x: placement.x, y: placement.y } : item,
    );
    if (placement.swapOf) {
      // Same-footprint widgets trade places.
      next = next.map((item) =>
        item.id === placement.swapOf ? { ...item, x: moving.x, y: moving.y } : item,
      );
    }
    setNotice(null);
    commit(next);
  };

  const dragging = drag?.active ? drag : null;
  const highlight = dragging?.target ?? null;
  const highlightDef = dragging && highlight ? CC_WIDGET_DEFS[dragging.id] : null;

  return (
    <>
      <SettingsGroup title="Edit mode">
        <OptionRow label="Edit the control center layout">
          <ToggleSwitch
            checked={editing}
            onChange={(value) => {
              setEditing(value);
              setNotice(null);
              setDrag(null);
              dragStart.current = null;
            }}
            label="Edit the control center layout"
          />
        </OptionRow>
        <OptionRow label="Restore the default layout">
          <div className="settings-button-row">
            <button
              type="button"
              className="settings-mini-button"
              onClick={resetLayout}
            >
              <RotateCcw size={13} strokeWidth={2} />
              Reset
            </button>
          </div>
        </OptionRow>
      </SettingsGroup>

      <div className="ccx-editor">
        <div className="ccx-grid-wrap" data-editing={editing}>
          <div className="ccx-cells" aria-hidden="true">
            {Array.from({ length: CC_GRID_COLS * CC_GRID_ROWS }, (_, index) => (
              <span key={index} />
            ))}
            {highlight && highlightDef && (
              <div
                className="ccx-target"
                style={{
                  left: highlight.x * (dragging!.colWidth + CC_CELL_GAP),
                  top: highlight.y * (CC_ROW_HEIGHT + CC_CELL_GAP),
                  width:
                    highlightDef.w * dragging!.colWidth +
                    (highlightDef.w - 1) * CC_CELL_GAP,
                  height:
                    highlightDef.h * CC_ROW_HEIGHT +
                    (highlightDef.h - 1) * CC_CELL_GAP,
                }}
              />
            )}
          </div>
          <div ref={gridRef} className="cc-layout ccx-grid" data-editing={editing}>
            {items.map((item) => {
              const def = CC_WIDGET_DEFS[item.id];
              const isDragging = dragging?.id === item.id;
              return (
                <div
                  key={item.id}
                  className="ccx-item"
                  data-shape={def.shape}
                  data-dragging={isDragging ? "true" : undefined}
                  style={{
                    gridColumn: `${item.x + 1} / span ${def.w}`,
                    gridRow: `${item.y + 1} / span ${def.h}`,
                    transform: isDragging
                      ? `translate(${dragging!.dx}px, ${dragging!.dy}px)`
                      : undefined,
                  }}
                  onPointerDown={(event) => beginDrag(event, item)}
                  onPointerMove={moveDrag}
                  onPointerUp={endDrag}
                  onPointerCancel={endDrag}
                >
                  <CcWidgetPreview id={item.id} />
                  {editing && item.id !== "settings" && (
                    <button
                      type="button"
                      className="ccx-remove"
                      aria-label={`Remove ${def.name}`}
                      onPointerDown={(event) => event.stopPropagation()}
                      onClick={() => removeItem(item.id)}
                    >
                      <X size={11} strokeWidth={2.5} />
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      <SettingsGroup title="Widgets">
        {Object.values(CC_WIDGET_DEFS).map((def) => {
          const Icon = CC_WIDGET_ICONS[def.id];
          // The Settings entry point is part of the panel's contract: it can
          // be moved anywhere on the grid but never removed.
          const permanent = def.id === "settings";
          return (
            <OptionRow
              key={def.id}
              label={
                <span className="ccx-widget-label">
                  <Icon size={15} strokeWidth={2.2} />
                  {def.name}
                </span>
              }
            >
              <ToggleSwitch
                checked={permanent || items.some((item) => item.id === def.id)}
                disabled={permanent}
                onChange={(checked) => {
                  setNotice(null);
                  if (checked) {
                    const spot = findFreeSpot(items, def);
                    if (!spot) {
                      setNotice("No free space in the grid — remove a widget first.");
                      return;
                    }
                    commit([...items, { id: def.id, x: spot.x, y: spot.y }]);
                  } else {
                    commit(items.filter((item) => item.id !== def.id));
                  }
                }}
                label={permanent ? `${def.name} (always on)` : `Place ${def.name}`}
              />
            </OptionRow>
          );
        })}
        {notice && (
          <div className="settings-row">
            <span className="ccx-notice">{notice}</span>
          </div>
        )}
      </SettingsGroup>

      <p className="settings-pane-note">
        Turn on edit mode to arrange the control center: drag widgets to move
        them (same-size widgets swap), press the × to remove one. The pill
        tiles span 2x1, panels 4x1 and circle buttons 1x1 on the panel's 4x6
        grid. On the panel, the Calendar pill opens a full calendar view when
        clicked. Changes apply to the panel immediately.
      </p>
    </>
  );
}

/** A static, non-interactive stand-in for a template widget, shown in the
 * settings preview (the real widgets live only on the panel's glass). */
function CcWidgetPreview({ id }: { id: CcWidgetId }) {
  const def = CC_WIDGET_DEFS[id];
  const Icon = CC_WIDGET_ICONS[id];
  if (def.shape === "circle") {
    return (
      <div className="ccx-preview ccx-preview-circle">
        <Icon size={26} strokeWidth={2.2} />
      </div>
    );
  }
  if (def.shape === "panel") {
    return (
      <div className="ccx-preview ccx-preview-panel">
        <span className="ccx-preview-title">{def.name}</span>
        <div className="ccx-preview-slider">
          <span style={{ width: "62%" }} />
        </div>
      </div>
    );
  }
  return (
    <div className="ccx-preview ccx-preview-pill">
      <span className="ccx-preview-icon">
        <Icon size={20} strokeWidth={2.2} />
      </span>
      <span className="ccx-preview-text">
        <span className="ccx-preview-title">{def.name}</span>
        <span className="ccx-preview-sub">
          {id === "calendar"
            ? new Date().toLocaleDateString(undefined, {
                month: "short",
                day: "numeric",
              })
            : id === "battery"
              ? "100%"
              : id === "exit"
                ? "Quit"
                : "Off"}
        </span>
      </span>
    </div>
  );
}

const clamp = (value: number, min: number, max: number) =>
  Math.min(Math.max(value, min), max);
