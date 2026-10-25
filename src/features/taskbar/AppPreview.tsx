import { useEffect, useId, useRef } from "react";
import type { CSSProperties } from "react";
import { activateWindow, setWindowPreview, type TaskbarApp } from "../../lib/shell";
import { useLiquidGlassSurface } from "glass-ui";
import { AppIcon } from "./AppIcon";

const PREVIEW_WIDTH = 264;
const VIEWPORT_MARGIN = 12;

interface AppPreviewProps {
  app: TaskbarApp;
  anchor: DOMRect;
  onHover: (hovering: boolean) => void;
}

/**
 * The hover preview popup: a liquid glass box floating above the app button
 * with a live DWM thumbnail of the app's window inside it. The image area's
 * rect is pushed to the native side, which hosts the actual thumbnail; the
 * glass frame and title row live in the webview.
 */
export function AppPreview({ app, anchor, onHover }: AppPreviewProps) {
  const previewId = useId();
  const imageRef = useRef<HTMLDivElement | null>(null);
  const hoverRef = useRef(onHover);
  hoverRef.current = onHover;

  // "preview:" prefix makes the renderer drive the shared persistent preview
  // surface (pre-warmed, re-anchored per open) instead of a new menu surface.
  useLiquidGlassSurface(
    true,
    `[data-app-preview="${previewId}"]`,
    20,
    `preview${previewId}`,
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    // The thumbnail popup sits above the webview and reports its own mouse
    // hover so the preview stays open while the cursor is over the image.
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<boolean>("preview-hover", (event) =>
        hoverRef.current(event.payload === true),
      ).then((fn) => {
        unlisten = fn;
      }),
    );
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    let frame = 0;
    const push = () => {
      const node = imageRef.current;
      if (!node) {
        return;
      }
      const rect = node.getBoundingClientRect();
      if (rect.width < 1 || rect.height < 1) {
        return;
      }
      const scale = window.devicePixelRatio || 1;
      void setWindowPreview(app.id, {
        x: rect.left * scale,
        y: rect.top * scale,
        width: rect.width * scale,
        height: rect.height * scale,
        // The thumbnail's corner radius matches the glass box's radius plus
        // the padding, scaled to physical pixels.
        radius: (20 + 7) * scale,
      });
    };
    const pushSettled = () => {
      if (frame) {
        return;
      }
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => {
          frame = 0;
          push();
        });
      });
    };
    pushSettled();
    const timer = window.setInterval(pushSettled, 500);
    return () => {
      if (frame) {
        cancelAnimationFrame(frame);
      }
      window.clearInterval(timer);
      void setWindowPreview(null, null);
    };
  }, [app.id]);

  const left = Math.min(
    Math.max(anchor.left + anchor.width / 2 - PREVIEW_WIDTH / 2, VIEWPORT_MARGIN),
    window.innerWidth - PREVIEW_WIDTH - VIEWPORT_MARGIN,
  );
  const style: CSSProperties = {
    left,
    bottom: window.innerHeight - anchor.top + 13,
    width: PREVIEW_WIDTH,
  };

  return (
    <div
      className="app-preview"
      data-app-preview={previewId}
      style={style}
      onPointerEnter={() => onHover(true)}
      onPointerLeave={() => onHover(false)}
      onClick={() => void activateWindow(app.id)}
    >
      <div className="app-preview-image" ref={imageRef}>
        <span className="app-icon app-preview-placeholder">
          <AppIcon app={app} color="#2779d8" />
        </span>
      </div>
      <div className="app-preview-title">
        <span className="app-icon app-preview-icon">
          <AppIcon app={app} color="#2779d8" />
        </span>
        <span className="app-preview-title-text">{app.title}</span>
      </div>
    </div>
  );
}
