import { useEffect, useRef, useState } from "react";
import {
  activateWindow,
  closeApp,
  launchPinned,
  type TaskbarApp,
} from "../../lib/shell";
import { LiquidDropdown, type LiquidDropdownItem } from "glass-ui";
import { AppPreview } from "./AppPreview";
import { AppIcon } from "./AppIcon";

/** A running window of the app, for the right-click dropdown's window list. */
export interface AppWindowInstance {
  id: number;
  title: string;
  isActive: boolean;
  isMinimized: boolean;
}

const appColors = ["#2779d8", "#23a5f2", "#1ed760", "#f6c343", "#8d75e8", "#ef6b73"];

const HOVER_DELAY_MS = 750;
const CLOSE_GRACE_MS = 350;

interface AppTaskbarButtonProps {
  app: TaskbarApp;
  /** Every window of this app (stacking groups them under one button). */
  instances: TaskbarApp[];
  /** Present for pinned apps that aren't running: clicking launches this. */
  launchPath?: string | null;
  position: number;
  isPinned: boolean;
  onTogglePin: (app: { id: number; processName: string }, pinned: boolean) => void;
}

export function AppTaskbarButton({
  app,
  instances,
  launchPath,
  position,
  isPinned,
  onTogglePin,
}: AppTaskbarButtonProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [preview, setPreview] = useState<DOMRect | null>(null);
  const buttonRef = useRef<HTMLButtonElement | null>(null);
  const hoverTimer = useRef(0);
  const closeTimer = useRef(0);
  // Pinned placeholders (closed apps) have no window to preview or close.
  const isRunning = app.id !== -1;

  useEffect(
    () => () => {
      window.clearTimeout(hoverTimer.current);
      window.clearTimeout(closeTimer.current);
    },
    [],
  );

  const clearHoverTimer = () => window.clearTimeout(hoverTimer.current);
  const clearCloseTimer = () => window.clearTimeout(closeTimer.current);

  const schedulePreview = () => {
    if (!isRunning) {
      return;
    }
    clearHoverTimer();
    hoverTimer.current = window.setTimeout(() => {
      const rect = buttonRef.current?.getBoundingClientRect();
      if (rect) {
        setPreview(rect);
      }
    }, HOVER_DELAY_MS);
  };

  const scheduleClose = () => {
    clearHoverTimer();
    clearCloseTimer();
    closeTimer.current = window.setTimeout(() => setPreview(null), CLOSE_GRACE_MS);
  };

  const hidePreview = () => {
    clearHoverTimer();
    clearCloseTimer();
    setPreview(null);
  };

  const activate = () => {
    if (launchPath) {
      void launchPinned(launchPath);
    } else {
      // Click toggle: a visible window minimizes, a minimized one restores.
      void activateWindow(app.id, true);
    }
  };

  const menuItems: LiquidDropdownItem[] = [
    // One row per window instance: clicking focuses and raises it. Pinned
    // placeholders (nothing running) skip the list.
    ...(isRunning
      ? instances.map<LiquidDropdownItem>((instance) => ({
          label: instance.title || app.title,
          onSelect: () => void activateWindow(instance.id),
        }))
      : []),
    ...(isRunning
      ? [
          { label: "", kind: "separator" as const },
          {
            label: "Show all windows",
            onSelect: () => {
              for (const instance of instances) {
                void activateWindow(instance.id);
              }
            },
          },
          { label: "", kind: "separator" as const },
        ]
      : []),
    {
      label: isPinned ? "Unpin from taskbar" : "Pin to taskbar",
      onSelect: () => onTogglePin(app, isPinned),
    },
    ...(isRunning ? [{ label: "Close program", onSelect: () => void closeApp(app.id) }] : []),
  ];

  return (
    <>
      <LiquidDropdown
        open={menuOpen}
        onOpenChange={(open) => {
          if (open) {
            hidePreview();
          }
          setMenuOpen(open);
        }}
        align="center"
        width={250}
        borderRadius={18}
        spring
        items={menuItems}
        trigger={
          <button
            ref={buttonRef}
            className={`taskbar-button app-taskbar-button ${app.isActive ? "is-active" : ""}`}
            aria-label={app.title}
            title={app.title}
            data-app-entry=""
            data-app-name={app.processName}
            data-app-id={app.id}
            data-app-side={isPinned ? "pinned" : "running"}
            onClick={() => {
              hidePreview();
              activate();
            }}
            onPointerDown={(event) => {
              // The app menu opens on right-click only; keep left-click for
              // activating the window instead of toggling the menu open.
              if (event.button === 0) {
                event.preventDefault();
              }
            }}
            onPointerEnter={(event) => {
              if (event.pointerType === "touch") {
                return;
              }
              clearCloseTimer();
              schedulePreview();
            }}
            onPointerLeave={scheduleClose}
            onContextMenu={(event) => {
              event.preventDefault();
              event.stopPropagation();
              hidePreview();
              setMenuOpen(true);
            }}
          >
            <span className="app-icon">
              <AppIcon app={app} color={appColors[position % appColors.length]} />
            </span>
            {/* Only running apps get the running indicator dot. */}
            {isRunning && <span className="app-indicator" />}
          </button>
        }
      />
      {preview && (
        <AppPreview
          app={app}
          anchor={preview}
          onHover={(hovering) => (hovering ? clearCloseTimer() : scheduleClose())}
        />
      )}
    </>
  );
}